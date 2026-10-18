//! A long-running local service. Sources are loaded once, and torrent
//! operations share one rqbit session, so metadata, DHT state and peer
//! connections carry over from one request to the next.
//!
//! Transport: plain HTTP/1.1 on 127.0.0.1 with one request per connection.
//! A bearer token is generated per run and published in a discovery file
//! that only the current user can read. Requests that carry an `Origin`
//! header, or whose `Host` is not the loopback address, are refused, so a
//! web page cannot reach the service.
use crate::{
    SearchError, SearchSource,
    agent::{AgentRequest, AgentResponse, AgentStatus, MAX_REQUEST_BYTES, execute_with_pool},
    torrent::{TorrentPool, TorrentRuntime},
};
use serde::{Deserialize, Serialize};
use std::{
    hash::{BuildHasher, RandomState},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

pub const SCHEMA_VERSION: u32 = 1;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_CONNECTIONS: usize = 32;
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_TIMEOUT: Duration = Duration::from_secs(60);
const REAP_INTERVAL: Duration = Duration::from_secs(15);
/// How long a stop waits for cancelled requests to return their reports.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Contents of the discovery file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonInfo {
    pub schema_version: u32,
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub version: String,
    /// The paths and options the daemon started with. A client forwards a
    /// request only when its own configuration is identical, so paths given
    /// explicitly on the command line are always honoured.
    pub configuration: serde_json::Value,
}

/// `TORRENT_SEARCH_DAEMON_FILE` overrides the default location (tests,
/// side-by-side installs).
pub fn discovery_path() -> PathBuf {
    if let Some(path) = std::env::var_os("TORRENT_SEARCH_DAEMON_FILE") {
        return path.into();
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("torrent-search").join("daemon.json")
}

pub fn read_discovery(path: &Path) -> Option<DaemonInfo> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub struct DaemonConfig {
    pub sources: Vec<Arc<dyn SearchSource>>,
    pub ffprobe: PathBuf,
    pub runtime: TorrentRuntime,
    /// Stop rqbit after this long without a torrent request; it restarts on demand.
    pub idle: Duration,
    pub configuration: serde_json::Value,
    pub discovery: PathBuf,
}

struct Shared {
    sources: Vec<Arc<dyn SearchSource>>,
    ffprobe: PathBuf,
    runtime: TorrentRuntime,
    pool: TorrentPool,
    token: String,
    port: u16,
    started: Instant,
    served: AtomicU64,
    active: AtomicU64,
    shutdown: CancellationToken,
}

/// Run until `shutdown` is cancelled or a client posts `/v1/shutdown`.
/// `ready` is called once the discovery file is written.
pub async fn serve(
    config: DaemonConfig,
    shutdown: CancellationToken,
    ready: impl FnOnce(&DaemonInfo),
) -> Result<(), SearchError> {
    if let Some(running) = DaemonClient::discover(&config.discovery).await {
        return Err(SearchError::Runtime(format!(
            "A daemon is already running (pid {}, port {})",
            running.info.pid, running.info.port
        )));
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let info = DaemonInfo {
        schema_version: SCHEMA_VERSION,
        pid: std::process::id(),
        port,
        token: new_token(),
        version: env!("CARGO_PKG_VERSION").into(),
        configuration: config.configuration,
    };
    write_discovery(&config.discovery, &info)?;
    let shared = Arc::new(Shared {
        sources: config.sources,
        ffprobe: config.ffprobe,
        pool: TorrentPool::new(config.runtime.clone(), config.idle),
        runtime: config.runtime,
        token: info.token.clone(),
        port,
        started: Instant::now(),
        served: AtomicU64::new(0),
        active: AtomicU64::new(0),
        shutdown,
    });
    ready(&info);

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
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        let accepted = tokio::select! {
            _ = shared.shutdown.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        while tasks.try_join_next().is_some() {}
        let stream = match accepted {
            Ok((stream, _)) => stream,
            // A client that reset before accept; avoid spinning on a persistent error.
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let shared = shared.clone();
        match limit.clone().try_acquire_owned() {
            Ok(permit) => {
                tasks.spawn(async move {
                    handle(stream, &shared).await;
                    drop(permit);
                });
            }
            Err(_) => {
                tasks.spawn(async move {
                    let mut stream = stream;
                    respond(&mut stream, 503, &error_body("Too many connections")).await;
                });
            }
        }
    }
    drop(listener);
    // In-flight requests hold child tokens of `shutdown` and return their
    // partial reports; give them time to finish before rqbit stops.
    let drained = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
    }
    reaper.abort();
    let stopped = shared.pool.shutdown().await;
    if read_discovery(&config.discovery).is_some_and(|current| current.token == info.token) {
        let _ = std::fs::remove_file(&config.discovery);
    }
    stopped.map_err(|error| {
        SearchError::Runtime(format!("Torrent session shutdown: {}", error.message))
    })
}

fn new_token() -> String {
    // RandomState is keyed from the OS random source once per process; the
    // clock and pid only separate the four draws.
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    (0..4u8)
        .map(|round| {
            format!(
                "{:016x}",
                RandomState::new().hash_one((round, nanos, std::process::id()))
            )
        })
        .collect()
}

fn write_discovery(path: &Path, info: &DaemonInfo) -> Result<(), SearchError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let partial = path.with_extension("json.partial");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write;
        let mut file = options.open(&partial)?;
        file.write_all(&serde_json::to_vec_pretty(info)?)?;
    }
    std::fs::rename(&partial, path)?;
    Ok(())
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

async fn handle(mut stream: TcpStream, shared: &Shared) {
    let request = match read_request(&mut stream).await {
        Ok(request) => request,
        Err((status, message)) => {
            respond(&mut stream, status, &error_body(message)).await;
            return;
        }
    };
    if let Err((status, message)) = authorize(&request, shared) {
        respond(&mut stream, status, &error_body(message)).await;
        return;
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/v1/health") => {
            let body = serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "uptime_ms": shared.started.elapsed().as_millis() as u64,
                "requests_served": shared.served.load(Ordering::Relaxed),
                "active_requests": shared.active.load(Ordering::Relaxed),
                "sources": shared.sources.len(),
                "torrent_session": shared.pool.running().await,
                "parked_torrents": shared.pool.parked().await,
            });
            respond(
                &mut stream,
                200,
                &serde_json::to_vec(&body).unwrap_or_default(),
            )
            .await;
        }
        ("POST", "/v1/agent") => agent(stream, request.body, shared).await,
        ("POST", "/v1/shutdown") => {
            respond(&mut stream, 200, br#"{"status":"stopping"}"#).await;
            shared.shutdown.cancel();
        }
        (_, "/v1/health" | "/v1/agent" | "/v1/shutdown") => {
            respond(&mut stream, 405, &error_body("Method not allowed")).await;
        }
        _ => respond(&mut stream, 404, &error_body("Not found")).await,
    }
}

fn authorize(request: &Request, shared: &Shared) -> Result<(), (u16, &'static str)> {
    // Browsers always send Origin on cross-site POSTs and on fetches; local
    // clients do not.
    if request.header("origin").is_some() {
        return Err((403, "Browser requests are not accepted"));
    }
    // A DNS-rebinding page reaches 127.0.0.1 under its own host name.
    let host = request.header("host").unwrap_or_default();
    let port = shared.port.to_string();
    let loopback = host
        .strip_prefix("127.0.0.1:")
        .or_else(|| host.strip_prefix("localhost:"))
        .is_some_and(|value| value == port);
    if !loopback {
        return Err((403, "Host must be the loopback address"));
    }
    let token = request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !constant_time_eq(token.as_bytes(), shared.token.as_bytes()) {
        return Err((401, "Missing or wrong token"));
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn agent(stream: TcpStream, body: Vec<u8>, shared: &Shared) {
    shared.active.fetch_add(1, Ordering::Relaxed);
    let (mut reader, mut writer) = stream.into_split();
    let cancel = shared.shutdown.child_token();
    // The client closing its connection abandons the request.
    let watch = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let mut buffer = [0u8; 256];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            cancel.cancel();
        })
    };
    let response = match serde_json::from_slice::<AgentRequest>(&body) {
        Err(error) => AgentResponse::failure(error.into()),
        Ok(request) => {
            execute_with_pool(
                request,
                shared.sources.clone(),
                cancel,
                &shared.ffprobe,
                &shared.runtime,
                Some(&shared.pool),
            )
            .await
        }
    };
    watch.abort();
    let body = serde_json::to_vec(&response).unwrap_or_else(|error| {
        serde_json::to_vec(&AgentResponse::failure(error.into())).unwrap_or_default()
    });
    respond(&mut writer, 200, &body).await;
    shared.served.fetch_add(1, Ordering::Relaxed);
    shared.active.fetch_sub(1, Ordering::Relaxed);
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, (u16, &'static str)> {
    let mut buffer = Vec::with_capacity(1024);
    let end = tokio::time::timeout(HEADER_TIMEOUT, async {
        let mut chunk = [0u8; 2048];
        loop {
            if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                return Ok(end);
            }
            if buffer.len() > MAX_HEADER_BYTES {
                return Err((431, "Headers too large"));
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return Err((400, "Connection closed")),
                Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            }
        }
    })
    .await
    .map_err(|_| (408, "Request timeout"))??;
    let head = std::str::from_utf8(&buffer[..end]).map_err(|_| (400, "Headers are not text"))?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(path), Some(version)) = (start.next(), start.next(), start.next())
    else {
        return Err((400, "Malformed request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err((400, "HTTP/1.x only"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let (key, value) = line.split_once(':').ok_or((400, "Malformed header"))?;
        headers.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    let mut request = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body: Vec::new(),
    };
    if request.header("transfer-encoding").is_some() {
        return Err((411, "Send Content-Length, not chunked bodies"));
    }
    let length: u64 = match request.header("content-length") {
        None => 0,
        Some(value) => value.parse().map_err(|_| (400, "Bad Content-Length"))?,
    };
    if length > MAX_REQUEST_BYTES {
        return Err((413, "Request exceeds 16 MiB"));
    }
    let length = length as usize;
    let mut body = buffer.split_off(end + 4);
    if body.len() > length {
        return Err((400, "Body longer than Content-Length"));
    }
    let missing = length - body.len();
    body.resize(length, 0);
    let offset = length - missing;
    tokio::time::timeout(BODY_TIMEOUT, stream.read_exact(&mut body[offset..]))
        .await
        .map_err(|_| (408, "Request timeout"))?
        .map_err(|_| (400, "Connection closed"))?;
    request.body = body;
    Ok(request)
}

fn error_body(message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap_or_default()
}

async fn respond(stream: &mut (impl AsyncWrite + Unpin), status: u16, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.shutdown().await;
}

/// Talks to a running daemon.
pub struct DaemonClient {
    info: DaemonInfo,
    http: reqwest::Client,
    base: String,
}

impl DaemonClient {
    pub fn new(info: DaemonInfo) -> Result<Self, SearchError> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .build()
            .map_err(|error| SearchError::Runtime(error.to_string()))?;
        let base = format!("http://127.0.0.1:{}", info.port);
        Ok(Self { info, http, base })
    }

    /// The daemon named in the discovery file, if it answers.
    pub async fn discover(path: &Path) -> Option<Self> {
        let info = read_discovery(path)?;
        if info.schema_version != SCHEMA_VERSION {
            return None;
        }
        let client = Self::new(info).ok()?;
        client.health().await.ok()?;
        Some(client)
    }

    pub fn info(&self) -> &DaemonInfo {
        &self.info
    }

    pub async fn health(&self) -> Result<serde_json::Value, SearchError> {
        let response = self
            .http
            .get(format!("{}/v1/health", self.base))
            .bearer_auth(&self.info.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(runtime)?;
        Self::json(response).await
    }

    /// Forward one request body; the reply is the daemon's AgentResponse JSON.
    /// Dropping the future closes the connection, which cancels the request.
    pub async fn agent(&self, request: Vec<u8>) -> Result<Vec<u8>, SearchError> {
        let response = self
            .http
            .post(format!("{}/v1/agent", self.base))
            .bearer_auth(&self.info.token)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request)
            .send()
            .await
            .map_err(runtime)?;
        let status = response.status();
        let body = response.bytes().await.map_err(runtime)?;
        if !status.is_success() {
            return Err(SearchError::Runtime(format!(
                "Daemon answered {status}: {}",
                String::from_utf8_lossy(&body)
            )));
        }
        Ok(body.to_vec())
    }

    pub async fn shutdown(&self) -> Result<(), SearchError> {
        let response = self
            .http
            .post(format!("{}/v1/shutdown", self.base))
            .bearer_auth(&self.info.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(runtime)?;
        Self::json(response).await.map(|_| ())
    }

    async fn json(response: reqwest::Response) -> Result<serde_json::Value, SearchError> {
        let status = response.status();
        let body = response.bytes().await.map_err(runtime)?;
        if !status.is_success() {
            return Err(SearchError::Runtime(format!(
                "Daemon answered {status}: {}",
                String::from_utf8_lossy(&body)
            )));
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

fn runtime(error: reqwest::Error) -> SearchError {
    SearchError::Runtime(format!("Daemon: {error}"))
}

/// The process exit code for a forwarded response body.
pub fn exit_code(response: &serde_json::Value) -> u8 {
    response
        .get("status")
        .cloned()
        .and_then(|status| serde_json::from_value::<AgentStatus>(status).ok())
        .map_or(1, AgentStatus::exit_code)
}
