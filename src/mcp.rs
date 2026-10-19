//! MCP over stdio: each agent operation is a tool, so an MCP client calls
//! torrentx directly instead of writing request files.
//!
//! The server lives as long as its client, so it keeps what `serve` keeps:
//! sources loaded and one warm torrent session whose finished torrents are
//! parked for fast retries. Tool results default to a compact view that
//! keeps every verdict, failure and reference but drops the per-token
//! evidence an agent rarely reads; `view: "full"` returns the agent response
//! unchanged.

use crate::{
    SearchError, SearchSource,
    agent::{AgentRequest, AgentResponse, AgentStatus, execute_with_pool},
    torrent::{TorrentPool, TorrentRuntime},
};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

/// Protocol versions this server speaks; the newest is offered by default.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const REAP_INTERVAL: Duration = Duration::from_secs(30);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_MAX_RESULTS: usize = 20;
const DEFAULT_MAX_FILES: usize = 200;
/// A line longer than this is answered with a parse error and skipped.
const MAX_LINE_BYTES: usize = crate::agent::MAX_REQUEST_BYTES as usize;

const INSTRUCTIONS: &str = "torrentx searches torrent sites, lists the files inside a torrent, and \
inspects the real streams of a file with ffprobe while reading only its headers from peers.\n\
\n\
Start with `find` for a question like \"series X at 1080p, best quality\": it searches, inspects one \
typical file of the leading matches and compares their actual streams. You decide what \"best\" \
means through constraints and preferences; nothing else is assumed.\n\
\n\
Two kinds of facts come back, and they must not be mixed up:\n\
- Claimed: anything under search results (resolution, codec, format, audio) is read from release \
names. A name can be wrong.\n\
- Verified: streams in a completed inspection come from the file itself.\n\
When an inspection failed (`inspection_status` or `status` failed, or a `failure` such as \
probe_timeout), nothing about that file's streams is known: say it was not inspected, and retry \
with a larger `inspection_limits.timeout_seconds` (this server keeps the partial download, so a \
retry resumes). Never present claimed or remembered facts as inspection results.\n\
\n\
Seeders are as reported by the site when it was searched; few seeders make inspection slow.";

pub struct McpConfig {
    pub sources: Vec<Arc<dyn SearchSource>>,
    pub ffprobe: PathBuf,
    pub runtime: TorrentRuntime,
    /// Stop rqbit after this long without torrent work.
    pub idle: Duration,
}

struct Shared {
    sources: Vec<Arc<dyn SearchSource>>,
    ffprobe: PathBuf,
    runtime: TorrentRuntime,
    pool: TorrentPool,
    /// In-flight tool calls by JSON-RPC id, for `notifications/cancelled`.
    calls: Mutex<HashMap<String, CancellationToken>>,
    shutdown: CancellationToken,
    replies: mpsc::UnboundedSender<Value>,
}

/// Serve MCP on `input`/`output` until the client closes `input`.
pub async fn serve<R, W>(config: McpConfig, input: R, output: W) -> Result<(), SearchError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (replies, mut outgoing) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut output = output;
        while let Some(message) = outgoing.recv().await {
            let mut line = serde_json::to_vec(&message).unwrap_or_default();
            line.push(b'\n');
            if output.write_all(&line).await.is_err() || output.flush().await.is_err() {
                break;
            }
        }
    });
    let shared = Arc::new(Shared {
        sources: config.sources,
        ffprobe: config.ffprobe,
        pool: TorrentPool::new(config.runtime.clone(), config.idle),
        runtime: config.runtime,
        calls: Mutex::new(HashMap::new()),
        shutdown: CancellationToken::new(),
        replies,
    });
    let reaper = {
        let shared = shared.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(REAP_INTERVAL);
            loop {
                ticks.tick().await;
                shared.pool.reap_idle().await;
            }
        })
    };

    let mut tasks = tokio::task::JoinSet::new();
    let mut input = input;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut input)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_LINE_BYTES && !line.ends_with(b"\n") {
            // Skip the rest of an oversized line.
            let mut rest = Vec::new();
            input.read_until(b'\n', &mut rest).await?;
            shared.send(error_reply(Value::Null, -32700, "Message too large"));
            continue;
        }
        while tasks.try_join_next().is_some() {}
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(text) {
            Ok(message) => message,
            Err(error) => {
                shared.send(error_reply(
                    Value::Null,
                    -32700,
                    &format!("Parse error: {error}"),
                ));
                continue;
            }
        };
        if let Some(call) = shared.dispatch(message) {
            tasks.spawn(call);
        }
    }
    // The client is gone: cancel what runs, let it return, then stop rqbit.
    shared.shutdown.cancel();
    let drained = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
    }
    reaper.abort();
    let stopped = shared.pool.shutdown().await;
    drop(shared);
    let _ = writer.await;
    stopped.map_err(|error| {
        SearchError::Runtime(format!("Torrent session shutdown: {}", error.message))
    })
}

impl Shared {
    fn send(&self, message: Value) {
        let _ = self.replies.send(message);
    }

    /// Answer a message at once, or return the tool call to run concurrently.
    fn dispatch(
        self: &Arc<Self>,
        message: Value,
    ) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response from the client; this server sends no requests.
            if message.get("id").is_none() {
                self.send(error_reply(Value::Null, -32600, "Invalid request"));
            }
            return None;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            if method == "notifications/cancelled"
                && let Some(request) = params.get("requestId")
                && let Some(token) = self.calls.lock().unwrap().get(&request.to_string())
            {
                token.cancel();
            }
            return None;
        };
        match method {
            "initialize" => {
                let requested = params.get("protocolVersion").and_then(Value::as_str);
                let version = requested
                    .filter(|v| PROTOCOL_VERSIONS.contains(v))
                    .unwrap_or(PROTOCOL_VERSIONS[0]);
                self.send(result_reply(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name": "torrentx", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": INSTRUCTIONS,
                    }),
                ));
                None
            }
            "ping" => {
                self.send(result_reply(id, json!({})));
                None
            }
            "tools/list" => {
                self.send(result_reply(id, json!({ "tools": tools() })));
                None
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                if !TOOL_NAMES.contains(&name) {
                    self.send(error_reply(id, -32602, &format!("Unknown tool: {name}")));
                    return None;
                }
                let name = name.to_owned();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                let token = self.shutdown.child_token();
                let key = id.to_string();
                self.calls
                    .lock()
                    .unwrap()
                    .insert(key.clone(), token.clone());
                let shared = self.clone();
                Some(async move {
                    let result = shared.call(&name, arguments, token).await;
                    shared.calls.lock().unwrap().remove(&key);
                    shared.send(result_reply(id, result));
                })
            }
            _ => {
                self.send(error_reply(
                    id,
                    -32601,
                    &format!("Unknown method: {method}"),
                ));
                None
            }
        }
    }

    async fn call(&self, tool: &str, arguments: Value, cancel: CancellationToken) -> Value {
        let Value::Object(mut arguments) = arguments else {
            return tool_error("Tool arguments must be a JSON object");
        };
        let view = match arguments.remove("view") {
            None => View::Compact,
            Some(Value::String(v)) if v == "compact" => View::Compact,
            Some(Value::String(v)) if v == "full" => View::Full,
            Some(_) => return tool_error("view must be \"compact\" or \"full\""),
        };
        let mut limit = |key: &str, default: usize| match arguments.remove(key) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|n| *n >= 1)
                .map(|n| n as usize)
                .ok_or_else(|| format!("{key} must be a positive integer")),
        };
        let (max_results, max_files) = match (
            limit("max_results", DEFAULT_MAX_RESULTS),
            limit("max_files", DEFAULT_MAX_FILES),
        ) {
            (Ok(results), Ok(files)) => (results, files),
            (Err(message), _) | (_, Err(message)) => return tool_error(&message),
        };
        arguments.insert("operation".into(), json!(tool));
        arguments.insert("schema_version".into(), json!(1));
        let response = match serde_json::from_value::<AgentRequest>(Value::Object(arguments)) {
            Err(error) => AgentResponse::failure(error.into()),
            Ok(request) => {
                let sources = if request.requires_sources() {
                    self.sources.clone()
                } else {
                    vec![]
                };
                execute_with_pool(
                    request,
                    sources,
                    cancel,
                    &self.ffprobe,
                    &self.runtime,
                    Some(&self.pool),
                )
                .await
            }
        };
        let failed = matches!(
            response.status,
            AgentStatus::Error | AgentStatus::Failed | AgentStatus::Cancelled
        );
        let mut value = serde_json::to_value(&response)
            .unwrap_or_else(|error| json!({"status": "error", "error": error.to_string()}));
        if view == View::Compact {
            compact_response(tool, &mut value, max_results, max_files);
        }
        let text = serde_json::to_string(&value).unwrap_or_default();
        json!({"content": [{"type": "text", "text": text}], "isError": failed})
    }
}

#[derive(PartialEq)]
enum View {
    Compact,
    Full,
}

fn result_reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_error(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

const TOOL_NAMES: [&str; 5] = [
    "find",
    "search",
    "torrent_files",
    "inspect_torrent",
    "inspect_media",
];

fn tools() -> Value {
    let view = json!({
        "type": "string", "enum": ["compact", "full"],
        "description": "compact (default) keeps verdicts, failures and references; full is the complete response with per-token evidence and is much larger."
    });
    let max_results = json!({
        "type": "integer", "minimum": 1,
        "description": "Search results shown in the compact view (default 20); the summary still counts all."
    });
    let search = json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "category": {"type": "string", "enum": ["all", "anime", "books", "games", "movies", "music", "pictures", "software", "tv"], "description": "Default all."},
            "sources": {"type": "array", "items": {"type": "string"}, "description": "Source ids; empty or omitted searches every source."}
        },
        "required": ["query"]
    });
    let languages = "ISO 639-1 codes such as en, ja, ar.";
    let analysis = json!({
        "type": "object",
        "description": "Criteria read from release names. These are claims, not inspection.",
        "properties": {
            "target": {
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "aliases": {"type": "array", "items": {"type": "string"}, "description": "Other names the work is released under."},
                    "year": {"type": "integer"},
                    "season": {"type": "integer"},
                    "part": {"type": "integer"},
                    "episodes": {"type": "object", "properties": {"start": {"type": "integer"}, "end": {"type": "integer"}}}
                },
                "required": ["title"]
            },
            "constraints": {
                "type": "object",
                "description": "Required; a name that contradicts one is excluded, one that does not say is uncertain.",
                "properties": {
                    "resolutions": {"type": "array", "items": {"type": "integer", "enum": [480, 576, 720, 1080, 1440, 2160, 4320]}},
                    "codecs": {"type": "array", "items": {"type": "string", "enum": ["h264", "hevc", "av1", "vp9", "mpeg2", "xvid"]}},
                    "audio_languages": {"type": "array", "items": {"type": "string"}, "description": languages},
                    "coverage": {"type": "string", "enum": ["single_episode", "episode_batch", "season_pack", "multi_season_pack", "complete_claim"]},
                    "formats": {"type": "array", "items": {"type": "string"}, "description": "Accepted source formats only, e.g. [\"remux\"]: remux, bluray, webdl, webrip, hdtv, hdrip, dvdrip, screener, telecine, telesync, cam."},
                    "reject_formats": {"type": "array", "items": {"type": "string"}, "description": "Default rejects cam, telesync, telecine, screener; [] allows all."},
                    "min_size_bytes": {"type": "integer"},
                    "max_size_bytes": {"type": "integer"},
                    "min_seeders": {"type": "integer"}
                }
            },
            "preferences": {
                "type": "object",
                "description": "Ordered lists, best first; they rank and never exclude.",
                "properties": {
                    "resolutions": {"type": "array", "items": {"type": "integer"}},
                    "codecs": {"type": "array", "items": {"type": "string"}},
                    "audio_languages": {"type": "array", "items": {"type": "string"}},
                    "coverage": {"type": "array", "items": {"type": "string"}},
                    "formats": {"type": "array", "items": {"type": "string"}}
                }
            }
        },
        "required": ["target"]
    });
    let media = json!({
        "type": "object",
        "description": "Criteria checked on the inspected streams themselves.",
        "properties": {
            "constraints": {
                "type": "object",
                "properties": {
                    "min_video_width": {"type": "integer"},
                    "min_video_height": {"type": "integer"},
                    "video_codecs": {"type": "array", "items": {"type": "string"}},
                    "audio_languages": {"type": "array", "items": {"type": "string"}, "description": languages},
                    "subtitle_languages": {"type": "array", "items": {"type": "string"}},
                    "min_file_size_bytes": {"type": "integer"},
                    "max_file_size_bytes": {"type": "integer"},
                    "min_video_bitrate_bps": {"type": "integer"},
                    "max_video_bitrate_bps": {"type": "integer"}
                }
            },
            "preferences": {
                "type": "object",
                "properties": {
                    "video_codecs": {"type": "array", "items": {"type": "string"}},
                    "audio_languages": {"type": "array", "items": {"type": "string"}},
                    "subtitle_languages": {"type": "array", "items": {"type": "string"}},
                    "rank_by": {"type": "array", "items": {"type": "string", "enum": ["video_codecs", "audio_languages", "subtitle_languages", "smaller_file", "larger_file", "higher_video_bitrate", "lower_video_bitrate", "higher_video_pixels"]}, "description": "Order in which criteria decide."}
                }
            }
        }
    });
    let search_limits = json!({
        "type": "object",
        "properties": {
            "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 120, "description": "Per source, default 20."},
            "parallel_sources": {"type": "integer", "minimum": 1, "maximum": 16},
            "results_per_source": {"type": "integer", "minimum": 1, "maximum": 1000}
        }
    });
    let torrent_limits = json!({
        "type": "object",
        "properties": {"timeout_seconds": {"type": "integer", "description": "Reading a torrent's metadata, default 90."}}
    });
    let inspection_limits = json!({
        "type": "object",
        "properties": {
            "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 120, "description": "Per file, default 60, at most 120. Slow peers need more; a retry in the same session resumes the partial download."},
            "max_read_bytes": {"type": "integer", "description": "Default 16 MiB."}
        }
    });
    let reference = json!({
        "type": "object",
        "description": "A search result's `reference` as returned, e.g. {\"kind\": \"magnet\", \"uri\": \"magnet:?xt=urn:btih:...\"}. A pasted magnet needs only kind and uri.",
        "properties": {
            "kind": {"type": "string", "enum": ["magnet", "http"]},
            "uri": {"type": "string"}
        },
        "required": ["kind", "uri"]
    });
    json!([
        {
            "name": "find",
            "title": "Find and verify releases",
            "description": "Search, then inspect one typical file of each leading eligible result and compare their actual streams, in one call. Candidates come back best first by your media preferences, each with inspection_status; a failed candidate has no verified streams. `search` holds the analyzed search for alternatives. Takes one to several minutes: each candidate waits for its torrent's metadata (torrent_limits, default 90 s) and then its inspection (inspection_limits, default 60 s).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": search,
                    "analysis": analysis,
                    "media": media,
                    "inspect": {
                        "type": "object",
                        "properties": {
                            "candidates": {"type": "integer", "minimum": 0, "maximum": 8, "description": "Results to inspect, default 3; 0 only searches."},
                            "include_uncertain": {"type": "boolean", "description": "Also inspect results whose names leave a constraint undecided."},
                            "parallel": {"type": "integer", "minimum": 1, "maximum": 4}
                        }
                    },
                    "limits": search_limits,
                    "torrent_limits": torrent_limits,
                    "inspection_limits": inspection_limits,
                    "view": view,
                    "max_results": max_results
                },
                "required": ["search", "analysis"]
            }
        },
        {
            "name": "search",
            "title": "Search torrents",
            "description": "Search torrent sites and judge each result's name against your target, constraints and preferences. Everything here is claimed by release names; nothing is inspected. Results are ranked: eligibility, preferences, availability, quality.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "search": search,
                    "analysis": analysis,
                    "limits": search_limits,
                    "view": view,
                    "max_results": max_results
                },
                "required": ["search", "analysis"]
            }
        },
        {
            "name": "torrent_files",
            "title": "List a torrent's files",
            "description": "Read a torrent's file list from peers (no file data). The compact view lists video and audio files with their ids; pass ids to inspect_torrent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "reference": reference,
                    "limits": torrent_limits,
                    "view": view,
                    "max_files": {"type": "integer", "minimum": 1, "description": "Media files listed in the compact view (default 200)."}
                },
                "required": ["reference"]
            }
        },
        {
            "name": "inspect_torrent",
            "title": "Inspect files in a torrent",
            "description": "Inspect the real streams (codec, resolution, bit depth, audio and subtitle tracks, bitrate) of files in a torrent with ffprobe, reading only headers from peers. Give file_ids from torrent_files, or sample_files to let it choose (1 = a typical episode). Each inspection either has `media` (verified) or a `failure` (nothing known about that file).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "reference": reference,
                    "file_ids": {"type": "array", "items": {"type": "integer"}, "description": "Ids from torrent_files; at most 32."},
                    "sample_files": {"type": "integer", "minimum": 1, "maximum": 32, "description": "Used when file_ids is empty; default 1."},
                    "parallel": {"type": "integer", "minimum": 1, "maximum": 8},
                    "analysis": media,
                    "limits": torrent_limits,
                    "inspection_limits": inspection_limits,
                    "view": view
                },
                "required": ["reference"]
            }
        },
        {
            "name": "inspect_media",
            "title": "Inspect a local media file",
            "description": "Inspect the streams of a media file on this computer with ffprobe.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "media": {
                        "type": "object",
                        "properties": {
                            "input": {
                                "type": "object",
                                "properties": {
                                    "kind": {"type": "string", "enum": ["local_file"]},
                                    "path": {"type": "string"}
                                },
                                "required": ["kind", "path"]
                            },
                            "limits": inspection_limits
                        },
                        "required": ["input"]
                    },
                    "view": view
                },
                "required": ["media"]
            }
        }
    ])
}

/// Shrink a response for an agent's context without hiding any verdict.
fn compact_response(tool: &str, response: &mut Value, max_results: usize, max_files: usize) {
    if let Some(result) = response.get_mut("result").filter(|r| !r.is_null()) {
        match tool {
            "search" => *result = compact_search(result, max_results),
            "find" => *result = compact_find(result, max_results),
            "torrent_files" | "inspect_torrent" => {
                if let Some(torrent) = result.get_mut("torrent").filter(|t| !t.is_null()) {
                    let files = if tool == "torrent_files" {
                        max_files
                    } else {
                        0
                    };
                    *torrent = compact_torrent(torrent, files);
                }
                if let Some(Value::Array(inspections)) = result.get_mut("inspections") {
                    for inspection in inspections {
                        if let Some(media) = inspection.get_mut("media").filter(|m| !m.is_null()) {
                            *media = compact_media(media);
                        }
                    }
                }
                if let Some(media) = result.get_mut("media").filter(|m| !m.is_null()) {
                    *media = compact_media(media);
                }
            }
            "inspect_media" => *result = compact_media(result),
            _ => {}
        }
    }
    tidy(response);
}

fn compact_search(report: &Value, max_results: usize) -> Value {
    let results = report["results"].as_array().map_or(&[][..], Vec::as_slice);
    let shown: Vec<Value> = results
        .iter()
        .take(max_results)
        .enumerate()
        .map(|(index, result)| {
            let analysis = &result["analysis"];
            let mut claims = Map::new();
            for field in [
                "year",
                "season",
                "part",
                "episodes",
                "resolution",
                "video_codec",
                "source_format",
                "coverage",
                "audio_language",
                "subtitle_language",
            ] {
                let fact = &analysis[field];
                if fact["state"].as_str().is_none_or(|s| s == "unknown") {
                    continue;
                }
                let values: Vec<Value> = fact["claims"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .map(|claim| claim["value"].clone())
                    .collect();
                claims.insert(
                    field.into(),
                    match values.as_slice() {
                        [one] => one.clone(),
                        _ => Value::Array(values),
                    },
                );
            }
            if let Some(Value::Object(quality)) = analysis.get("quality") {
                for (key, value) in quality {
                    claims.insert(key.clone(), value.clone());
                }
            }
            let problems: Vec<String> = result["evaluations"]
                .as_array()
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .filter(|e| e["state"] != "match")
                .map(|e| {
                    format!(
                        "{}: {} ({})",
                        e["field"].as_str().unwrap_or("?"),
                        e["state"].as_str().unwrap_or("?"),
                        e["reason_code"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            let observations = result["observations"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            let size = observations
                .iter()
                .filter_map(|o| o["size_bytes"].as_u64())
                .max();
            let sources: Vec<&Value> = observations.iter().map(|o| &o["source_id"]).collect();
            let ranking = &result["ranking"];
            json!({
                "index": index,
                "title": result["title"],
                "reference": result["reference"],
                "size_bytes": size,
                "seeders": ranking["highest_reported_seeders"],
                "sources": sources,
                "eligibility": result["eligibility"],
                "problems": problems,
                "claimed": claims,
                "preference_score": ranking["preference_score"],
                "quality_score": ranking["quality_score"],
                "availability": ranking["availability"],
            })
        })
        .collect();
    // Failed sources grouped by kind: several dns_failed or connection_reset
    // at once point at the network, not at the sites.
    let mut failures = Map::new();
    for source in report["sources"].as_array().map_or(&[][..], Vec::as_slice) {
        if let Some(kind) = source["failure_kind"].as_str() {
            let ids = failures.entry(kind).or_insert_with(|| json!([]));
            if let Value::Array(ids) = ids {
                ids.push(source["source_id"].clone());
            }
        }
    }
    json!({
        "query": report["query"],
        "category": report["category"],
        "partial": report["partial"],
        "summary": report["summary"],
        "source_failures": failures,
        "sources": report["sources"],
        "results_shown": shown.len(),
        "results_total": results.len(),
        "note": "Values under `claimed` come from release names, not from inspecting files.",
        "results": shown,
    })
}

/// Each candidate carries its own evidence: the streams read from its file and
/// the conditions that file did not meet. A candidate whose file was not read
/// gets no position, so nothing in the reply ranks what was never inspected.
fn compact_find(report: &Value, max_results: usize) -> Value {
    let compared = report["comparison"]["results"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let candidates: Vec<Value> = report["candidates"]
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|candidate| {
            let evidence = compared.iter().find(|c| c["id"] == candidate["id"]);
            let media = evidence.map(|c| &c["inspection"]["result"]["media"]);
            let verified = media.is_some_and(|m| !m.is_null());
            let mut compact = json!({
                "search_index": candidate["search_index"],
                "title": candidate["title"],
                "reference": candidate["reference"],
                "inspection_status": candidate["inspection_status"],
                "inspection_error": candidate["inspection_error"],
            });
            if candidate["attempts"].as_u64().is_some_and(|n| n > 1) {
                compact["attempts"] = candidate["attempts"].clone();
            }
            if let (true, Some(media), Some(evidence)) = (verified, media, evidence) {
                let assessment = &evidence["assessment"];
                let unmet: Vec<Value> = assessment["evaluations"]
                    .as_array()
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .filter(|e| e["state"] != "match")
                    .map(|e| {
                        json!({
                            "field": e["field"],
                            "state": e["state"],
                            "expected": e["expected"],
                            "observed": e["observed"],
                            "reason_code": e["reason_code"],
                        })
                    })
                    .collect();
                compact["position"] = candidate["position"].clone();
                compact["eligibility"] = candidate["eligibility"].clone();
                compact["file"] = candidate["file"].clone();
                compact["verified"] = compact_media(media);
                compact["unmet"] = json!(unmet);
            }
            compact
        })
        .collect();
    let comparison = &report["comparison"];
    json!({
        "candidates": candidates,
        "ranking_criteria": comparison["ranking_criteria"],
        "note": "`verified` was read from each file with ffprobe. A candidate without it \
                 was not inspected: nothing about its streams is known, and the search \
                 claims for it are only what its release name says.",
        "search": compact_search(&report["search"], max_results),
    })
}

/// What ffprobe read from a file, without read accounting and evidence labels.
fn compact_media(media: &Value) -> Value {
    let all = media["streams"].as_array().map_or(&[][..], Vec::as_slice);
    // Attachments are fonts for styled subtitles: count them, do not list them.
    let attachments = all.iter().filter(|s| s["kind"] == "attachment").count();
    let streams: Vec<Value> = all
        .iter()
        .filter(|s| s["kind"] != "attachment")
        .map(compact_stream)
        .collect();
    json!({
        "attachments": (attachments > 0).then_some(attachments),
        "container": media["format"]["name"],
        "duration_seconds": media["format"]["duration_seconds"],
        "file_size_bytes": media["format"]["file_size_bytes"],
        "overall_bitrate_bps": media["format"]["overall_bitrate_bps"]["value"],
        "streams": streams,
    })
}

/// The stream facts an agent compares; measured values lose their `basis`.
fn compact_stream(stream: &Value) -> Value {
    let mut compact = Map::new();
    for key in [
        "index",
        "kind",
        "codec",
        "profile",
        "width",
        "height",
        "component_bit_depth",
        "frame_rate",
        "hdr_signals",
        "sample_rate_hz",
        "sample_bit_depth",
        "channels",
        "channel_layout",
        "language",
        "title",
        "bitrate_bps",
        "dispositions",
    ] {
        let value = &stream[key];
        let value = match value.get("basis") {
            Some(_) => value["value"].clone(),
            None => value.clone(),
        };
        compact.insert(key.into(), value);
    }
    Value::Object(compact)
}

fn compact_torrent(torrent: &Value, max_files: usize) -> Value {
    let files = torrent["files"].as_array().map_or(&[][..], Vec::as_slice);
    let media: Vec<&Value> = files
        .iter()
        .filter(|f| matches!(f["media_kind"].as_str(), Some("video" | "audio")))
        .collect();
    let total: u64 = files.iter().filter_map(|f| f["size_bytes"].as_u64()).sum();
    let mut compact = json!({
        "name": torrent["name"],
        "info_hash_v1": torrent["info_hash_v1"],
        "reference": torrent["resolved_reference"],
        "file_count": files.len(),
        "media_file_count": media.len(),
        "total_size_bytes": total,
    });
    if max_files > 0 {
        compact["files"] = json!(media.iter().take(max_files).collect::<Vec<_>>());
        if media.len() > max_files {
            compact["files_omitted"] = json!(media.len() - max_files);
        }
    }
    compact
}

/// Drop nulls, empty lists and container tags; list only set dispositions.
fn tidy(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("tags");
            if let Some(Value::Object(dispositions)) = map.get("dispositions") {
                let set: Vec<Value> = dispositions
                    .iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .map(|(k, _)| json!(k))
                    .collect();
                map.insert("dispositions".into(), Value::Array(set));
            }
            for child in map.values_mut() {
                tidy(child);
            }
            map.retain(|_, v| {
                !(v.is_null()
                    || v.as_array().is_some_and(Vec::is_empty)
                    || v.as_object().is_some_and(Map::is_empty))
            });
        }
        Value::Array(items) => items.iter_mut().for_each(tidy),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_keeps_meaning_and_drops_noise() {
        let mut value = json!({
            "a": null, "b": [], "c": {"d": null},
            "stream": {"codec": "hevc", "tags": {"BPS": "1"},
                       "dispositions": {"default": true, "forced": false}},
            "zero": 0, "no": false
        });
        tidy(&mut value);
        assert_eq!(
            value,
            json!({"stream": {"codec": "hevc", "dispositions": ["default"]}, "zero": 0, "no": false})
        );
    }

    #[test]
    fn compact_find_ranks_only_what_was_read() {
        let report = json!({
            "search": {"results": []},
            "candidates": [
                {"id": "a", "search_index": 0, "title": "A", "position": 1,
                 "eligibility": "excluded", "inspection_status": "completed", "file": "a.mkv"},
                {"id": "b", "search_index": 1, "title": "B", "position": null,
                 "inspection_status": "failed",
                 "inspection_error": {"code": "torrent_service_timeout", "message": "m"}}
            ],
            "comparison": {"ranking_criteria": ["video_codecs"], "results": [
                {"id": "b", "position": 2, "inspection": {"result": {}},
                 "assessment": {"evaluations": []}},
                {"id": "a", "position": 1,
                 "inspection": {"result": {"media": {
                     "format": {"name": "matroska", "file_size_bytes": 9,
                                "overall_bitrate_bps": {"value": 5, "basis": "container"}},
                     "streams": [{"index": 0, "kind": "video", "codec": "hevc", "height": 720,
                                  "side_data": [1],
                                  "component_bit_depth": {"value": 10, "basis": "pixel_format"}}]}}},
                 "assessment": {"evaluations": [
                     {"field": "min_video_height", "state": "mismatch", "expected": 800,
                      "observed": 720, "reason_code": "below_minimum", "reason": "long"},
                     {"field": "inspection_complete", "state": "match"}]}}
            ]}
        });
        let mut compact = compact_find(&report, 20);
        tidy(&mut compact);
        let [read, unread] = compact["candidates"].as_array().unwrap().as_slice() else {
            panic!("two candidates")
        };
        assert_eq!(read["position"], 1);
        assert_eq!(read["verified"]["overall_bitrate_bps"], 5);
        assert_eq!(
            read["verified"]["streams"],
            json!([{"index": 0, "kind": "video", "codec": "hevc", "height": 720,
                    "component_bit_depth": 10}])
        );
        assert_eq!(
            read["unmet"],
            json!([{"field": "min_video_height", "state": "mismatch", "expected": 800,
                    "observed": 720, "reason_code": "below_minimum"}])
        );
        assert!(unread.get("position").is_none() && unread.get("verified").is_none());
        assert_eq!(
            unread["inspection_error"]["code"],
            "torrent_service_timeout"
        );
        assert!(compact.get("comparison").is_none());
    }

    #[test]
    fn compact_inspections_keep_streams_and_failures_only() {
        let mut response = json!({"status": "partial", "result": {
        "torrent": {"name": "T", "files": []},
        "inspections": [
            {"file_id": 1, "media": {
                "evidence_source": "ffprobe_media_headers", "read_requests": 1,
                "format": {"name": "matroska", "file_size_bytes": 9,
                           "overall_bitrate_bps": {"value": 5, "basis": "ffprobe_format"}},
                "streams": [{"index": 1, "kind": "audio", "codec": "aac",
                             "sample_rate_hz": 48000,
                             "sample_bit_depth": {"basis": "unknown"},
                             "payload_size_bytes": {"basis": "unknown"},
                             "color_space": "bt709"},
                            {"index": 2, "kind": "attachment", "codec": "ttf"}],
                "limitations": ["visual_quality_not_measured"]}},
            {"file_id": 2, "failure": {"code": "probe_timeout", "message": "m"}}
        ]}});
        compact_response("inspect_torrent", &mut response, 20, 200);
        let inspections = &response["result"]["inspections"];
        assert_eq!(
            inspections[0]["media"],
            json!({"attachments": 1, "container": "matroska", "file_size_bytes": 9, "overall_bitrate_bps": 5,
                   "streams": [{"index": 1, "kind": "audio", "codec": "aac",
                                "sample_rate_hz": 48000}]})
        );
        assert_eq!(inspections[1]["failure"]["code"], "probe_timeout");
    }

    #[test]
    fn every_listed_tool_is_callable() {
        let listed: Vec<String> = tools()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(listed, TOOL_NAMES);
    }
}
