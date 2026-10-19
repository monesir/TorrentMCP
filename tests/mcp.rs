//! The MCP server against an in-memory client, and the `torrentx mcp` binary.
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader, Lines};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SearchSource, SourceDescriptor,
    SourceObservation, SourceOutcome, SourceReport, SourceStatus, TorrentReference,
    mcp::{McpConfig, serve},
    torrent::TorrentRuntime,
};

struct Source(SourceDescriptor);

fn result(title: &str, id: u64, seeders: u64) -> SearchResult {
    let hash = format!("{id:040x}");
    SearchResult {
        title: title.into(),
        reference: TorrentReference::Magnet {
            uri: format!("magnet:?xt=urn:btih:{hash}"),
            info_hash_v1: Some(hash),
        },
        observations: vec![SourceObservation {
            source_id: "fixture".into(),
            title: title.into(),
            raw_title: None,
            site_url: "https://example.org".into(),
            description_url: None,
            size_bytes: Some(4 << 30),
            seeders: Some(seeders),
            leechers: None,
            published_unix: None,
            observed_at_unix: 1,
        }],
    }
}

#[async_trait]
impl SearchSource for Source {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.0
    }
    async fn search(
        &self,
        _: &SearchRequest,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> SourceOutcome {
        let results: Vec<_> = (0..30)
            .map(|i| {
                result(
                    &format!("Shogun S01E{:02} 1080p WEB-DL x265", i + 1),
                    i + 1,
                    50 + i,
                )
            })
            .chain([result("Shogun S01 720p HDTV", 99, 500)])
            .collect();
        SourceOutcome {
            report: SourceReport {
                source_id: "fixture".into(),
                status: SourceStatus::Completed,
                elapsed_ms: 1,
                result_count: results.len(),
                limited: false,
                error: None,
                queried_category: None,
                failure_kind: None,
            },
            results,
        }
    }
    async fn resolve(
        &self,
        _: &str,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        unreachable!()
    }
}

struct Client {
    to_server: tokio::io::DuplexStream,
    from_server: Lines<AsyncBufReader<tokio::io::DuplexStream>>,
    server: tokio::task::JoinHandle<Result<(), SearchError>>,
    _cache: tempfile::TempDir,
}

impl Client {
    fn start() -> Self {
        let cache = tempfile::tempdir().unwrap();
        let config = McpConfig {
            sources: vec![Arc::new(Source(SourceDescriptor {
                id: "fixture".into(),
                name: "fixture".into(),
                site_url: "https://example.org".into(),
                categories: vec!["all".into()],
                resolves_http: false,
                scope: Default::default(),
                language: None,
                alternate_of: None,
                retired: None,
            }))],
            ffprobe: "missing-ffprobe".into(),
            runtime: TorrentRuntime {
                rqbit: "missing-rqbit".into(),
                cache_directory: cache.path().into(),
                download_limit_bytes_per_second: None,
            },
            idle: Duration::from_secs(60),
        };
        let (to_server, server_in) = tokio::io::duplex(1 << 20);
        let (server_out, from_server) = tokio::io::duplex(1 << 20);
        let server = tokio::spawn(serve(config, AsyncBufReader::new(server_in), server_out));
        Self {
            to_server,
            from_server: AsyncBufReader::new(from_server).lines(),
            server,
            _cache: cache,
        }
    }

    async fn send(&mut self, message: Value) {
        let mut line = serde_json::to_vec(&message).unwrap();
        line.push(b'\n');
        self.to_server.write_all(&line).await.unwrap();
    }

    async fn receive(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(30), self.from_server.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        let reply = self.receive().await;
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    async fn call(&mut self, id: u64, tool: &str, arguments: Value) -> (bool, Value) {
        let reply = self
            .request(
                id,
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
            )
            .await;
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap();
        (
            result["isError"].as_bool().unwrap(),
            serde_json::from_str(text).unwrap_or_else(|_| json!(text)),
        )
    }

    async fn close(self) {
        drop(self.to_server);
        tokio::time::timeout(Duration::from_secs(30), self.server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn initializes_lists_tools_and_answers_protocol_errors() {
    let mut client = Client::start();
    let init = client
        .request(
            1,
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "1"}}),
        )
        .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "torrentx");
    assert!(
        init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("nothing about that file's streams is known")
    );
    // Notifications get no reply; the next reply belongs to the next request.
    client
        .send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await;
    let old = client
        .request(2, "initialize", json!({"protocolVersion": "1999-01-01"}))
        .await;
    assert_eq!(old["result"]["protocolVersion"], "2025-06-18");

    let tools = client.request(3, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "find",
            "search",
            "torrent_files",
            "inspect_torrent",
            "inspect_media"
        ]
    );
    for tool in tools["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    assert_eq!(
        client.request(4, "ping", json!({})).await["result"],
        json!({})
    );
    assert_eq!(
        client.request(5, "resources/list", json!({})).await["error"]["code"],
        -32601
    );
    assert_eq!(
        client
            .request(6, "tools/call", json!({"name": "nope", "arguments": {}}))
            .await["error"]["code"],
        -32602
    );
    client.to_server.write_all(b"{not json\n").await.unwrap();
    assert_eq!(client.receive().await["error"]["code"], -32700);
    client.close().await;
}

#[tokio::test]
async fn search_is_compact_by_default_and_full_on_request() {
    let mut client = Client::start();
    let arguments = json!({
        "search": {"query": "shogun"},
        "analysis": {"target": {"title": "Shogun", "season": 1},
                     "constraints": {"resolutions": [1080]}}
    });
    let (is_error, compact) = client.call(1, "search", arguments.clone()).await;
    assert!(!is_error);
    assert_eq!(compact["status"], "completed");
    let result = &compact["result"];
    assert_eq!(result["results_total"], 31);
    assert_eq!(result["results_shown"], 20);
    assert_eq!(result["summary"]["excluded"], 1);
    let first = &result["results"][0];
    assert_eq!(first["eligibility"], "eligible");
    assert_eq!(first["claimed"]["resolution"], 1080);
    assert_eq!(first["claimed"]["video_codec"], "hevc");
    assert!(
        first["reference"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("magnet:")
    );
    let hdtv = result["results"]
        .as_array()
        .unwrap()
        .iter()
        .chain([&Value::Null])
        .find(|r| r["title"] == "Shogun S01 720p HDTV");
    // Excluded results rank last, past the first 20.
    assert!(hdtv.is_none());

    let mut more = arguments.clone();
    more["max_results"] = json!(40);
    let (_, all) = client.call(2, "search", more).await;
    let excluded = all["result"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["title"] == "Shogun S01 720p HDTV")
        .unwrap();
    assert_eq!(excluded["eligibility"], "excluded");
    assert!(
        excluded["problems"][0]
            .as_str()
            .unwrap()
            .starts_with("resolution: mismatch")
    );

    let mut full = arguments;
    full["view"] = json!("full");
    let (_, full) = client.call(3, "search", full).await;
    assert_eq!(full["result"]["results"].as_array().unwrap().len(), 31);
    assert!(full["result"]["results"][0]["analysis"]["titles"].is_array());
    client.close().await;
}

#[tokio::test]
async fn failures_are_tool_errors_with_their_reason() {
    let mut client = Client::start();
    let (is_error, response) = client
        .call(1, "search", json!({"search": {"query": "x"}}))
        .await;
    assert!(is_error, "missing analysis is an invalid request");
    assert_eq!(response["error"]["code"], "invalid_request");

    // A pasted magnet needs no info_hash_v1; without rqbit the read fails
    // and says why.
    let (is_error, response) = client
        .call(
            2,
            "inspect_torrent",
            json!({"reference": {"kind": "magnet",
                                 "uri": format!("magnet:?xt=urn:btih:{}", "b".repeat(40))}}),
        )
        .await;
    assert!(is_error);
    assert_ne!(response["error"]["code"], "invalid_request", "{response}");

    let (is_error, response) = client
        .call(
            3,
            "search",
            json!({"search": {"query": "x"}, "view": "tiny"}),
        )
        .await;
    assert!(is_error);
    assert!(response.as_str().unwrap().contains("view"));
    client.close().await;
}

#[test]
fn the_binary_speaks_mcp_on_stdio() {
    let directory = tempfile::tempdir().unwrap();
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let mut server = Command::new(env!("CARGO_BIN_EXE_torrentx"))
        .arg("--assets")
        .arg(assets)
        .args([
            "--python",
            "missing-python",
            "--rqbit",
            "missing-rqbit",
            "--ffprobe",
            "missing-ffprobe",
        ])
        .arg("--torrent-cache")
        .arg(directory.path().join("cache"))
        .arg("mcp")
        .current_dir(directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = server.stdin.take().unwrap();
    let mut output = BufReader::new(server.stdout.take().unwrap());
    let mut exchange = |message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = exchange(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                   "clientInfo": {"name": "t", "version": "1"}}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    let listed = exchange(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 5);
    drop(input);
    let status = server.wait().unwrap();
    assert!(status.success(), "closing stdin stops the server cleanly");
}
