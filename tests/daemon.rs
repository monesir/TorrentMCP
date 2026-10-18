use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    daemon::{DaemonClient, DaemonConfig, read_discovery, serve},
    torrent::TorrentRuntime,
};

fn analyze_request() -> Value {
    json!({"schema_version":1,"operation":"analyze",
        "analysis":{"target":{"title":"Nirvana in Fire","year":2015}},
        "report":{"schema_version":1,"query":"nirvana in fire","category":"all",
            "partial":false,"sources":[],"results":[]}})
}

fn config(directory: &Path) -> DaemonConfig {
    DaemonConfig {
        sources: vec![],
        ffprobe: "missing-ffprobe".into(),
        runtime: TorrentRuntime {
            rqbit: "missing-rqbit".into(),
            cache_directory: directory.join("cache"),
            download_limit_bytes_per_second: None,
        },
        idle: Duration::from_secs(60),
        configuration: json!({"test": true}),
        discovery: directory.join("daemon.json"),
    }
}

/// Send raw bytes and return the status line.
fn raw(port: u16, request: String) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply.lines().next().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn serves_authorized_loopback_requests_and_stops_cleanly() {
    let directory = tempfile::tempdir().unwrap();
    let discovery = directory.path().join("daemon.json");
    let shutdown = CancellationToken::new();
    let (ready, started) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(
        config(directory.path()),
        shutdown.clone(),
        move |info| {
            ready.send(info.clone()).unwrap();
        },
    ));
    let info = started.await.unwrap();
    assert_eq!(read_discovery(&discovery).as_ref(), Some(&info));
    assert_eq!(info.token.len(), 64);

    let client = DaemonClient::discover(&discovery).await.unwrap();
    let health = client.health().await.unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["torrent_session"], false);

    let body = client
        .agent(serde_json::to_vec(&analyze_request()).unwrap())
        .await
        .unwrap();
    let response: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(torrent_search::daemon::exit_code(&response), 0);
    let invalid: Value =
        serde_json::from_slice(&client.agent(b"{}".to_vec()).await.unwrap()).unwrap();
    assert_eq!(invalid["status"], "error");
    assert_eq!(invalid["error"]["code"], "invalid_request");
    assert_eq!(client.health().await.unwrap()["requests_served"], 2);

    // A second daemon on the same discovery file refuses to start.
    let second = serve(config(directory.path()), CancellationToken::new(), |_| {}).await;
    assert!(second.unwrap_err().to_string().contains("already running"));

    let port = info.port;
    let token = info.token.clone();
    let checks = tokio::task::spawn_blocking(move || {
        let request = |path: &str, method: &str, host: &str, extra: &str| {
            raw(
                port,
                format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: 0\r\n\r\n"),
            )
        };
        let loopback = format!("127.0.0.1:{port}");
        let auth = format!("Authorization: Bearer {token}\r\n");
        [
            request("/v1/health", "GET", &loopback, ""),
            request("/v1/health", "GET", &loopback, "Authorization: Bearer wrong\r\n"),
            request("/v1/health", "GET", &loopback, &format!("{auth}Origin: http://evil.example\r\n")),
            request("/v1/health", "GET", &format!("evil.example:{port}"), &auth),
            request("/v1/agent", "GET", &loopback, &auth),
            request("/v1/missing", "GET", &loopback, &auth),
            request("/v1/health", "GET", &format!("localhost:{port}"), &auth),
            raw(
                port,
                format!("POST /v1/agent HTTP/1.1\r\nHost: {loopback}\r\n{auth}Content-Length: 99999999999\r\n\r\n"),
            ),
        ]
    })
    .await
    .unwrap();
    let codes: Vec<&str> = checks
        .iter()
        .map(|line| line.split(' ').nth(1).unwrap_or_default())
        .collect();
    assert_eq!(
        codes,
        ["401", "401", "403", "403", "405", "404", "200", "413"]
    );

    client.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!discovery.exists(), "discovery file is removed on stop");
}

#[tokio::test]
async fn abandoned_partial_request_does_not_block_others() {
    let directory = tempfile::tempdir().unwrap();
    let shutdown = CancellationToken::new();
    let (ready, started) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(
        config(directory.path()),
        shutdown.clone(),
        move |info| {
            ready.send(info.clone()).unwrap();
        },
    ));
    let info = started.await.unwrap();
    // A partial body never completes: the connection closes and the
    // daemon goes on serving others.
    let port = info.port;
    let token = info.token.clone();
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .write_all(
                format!("POST /v1/agent HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Length: 100\r\n\r\n{{")
                    .as_bytes(),
            )
            .unwrap();
    })
    .await
    .unwrap();
    let client = DaemonClient::new(info).unwrap();
    assert_eq!(client.health().await.unwrap()["status"], "ok");
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

struct Daemon {
    child: Child,
    discovery: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cli(directory: &Path, discovery: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_torrent-search"));
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    command
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
        .arg(directory.join("cache"))
        .args(arguments)
        .env("TORRENT_SEARCH_DAEMON_FILE", discovery)
        .current_dir(directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn agent(directory: &Path, discovery: &Path, extra: &[&str]) -> (i32, Value) {
    let mut arguments = vec!["agent"];
    arguments.extend_from_slice(extra);
    let mut process = cli(directory, discovery, &arguments)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&analyze_request()).unwrap())
        .unwrap();
    let output = process.wait_with_output().unwrap();
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn status(directory: &Path, discovery: &Path) -> Value {
    let output = cli(directory, discovery, &["daemon", "status"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_forwards_to_a_matching_daemon_and_stops_it() {
    let directory = tempfile::tempdir().unwrap();
    let discovery = directory.path().join("daemon.json");
    let not_running = cli(directory.path(), &discovery, &["daemon", "status"])
        .output()
        .unwrap();
    assert_eq!(not_running.status.code(), Some(1));

    let mut daemon = Daemon {
        child: cli(directory.path(), &discovery, &["serve"])
            .spawn()
            .unwrap(),
        discovery: discovery.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while read_discovery(&daemon.discovery).is_none() {
        assert!(Instant::now() < deadline, "daemon did not start");
        assert!(daemon.child.try_wait().unwrap().is_none(), "daemon exited");
        std::thread::sleep(Duration::from_millis(100));
    }

    let (code, response) = agent(directory.path(), &discovery, &[]);
    assert_eq!((code, &response["status"]), (0, &json!("completed")));
    assert_eq!(status(directory.path(), &discovery)["requests_served"], 1);

    // Opting out, or a different configuration, runs in this process.
    let (code, _) = agent(directory.path(), &discovery, &["--no-daemon"]);
    assert_eq!(code, 0);
    let (code, _) = agent(
        directory.path(),
        &discovery,
        &["--source-backend", "python"],
    );
    assert_eq!(code, 0);
    assert_eq!(status(directory.path(), &discovery)["requests_served"], 1);

    let stop = cli(directory.path(), &discovery, &["daemon", "stop"])
        .output()
        .unwrap();
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    assert!(!discovery.exists());
    let deadline = Instant::now() + Duration::from_secs(30);
    while daemon.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "daemon process did not exit");
        std::thread::sleep(Duration::from_millis(100));
    }
}
