use super::MediaInput;
use crate::SearchError;
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const READ_CHUNK: u64 = 256 * 1024;

enum ByteSource {
    File(PathBuf),
    Http {
        client: reqwest::Client,
        url: String,
    },
}

#[derive(Default)]
pub struct ReadState {
    pub bytes_read: AtomicU64,
    pub requests: AtomicU64,
    pub failure: Mutex<Option<String>>,
}

pub struct ReadGateway {
    pub url: String,
    pub size: u64,
    pub state: Arc<ReadState>,
    shutdown: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl ReadGateway {
    pub async fn start(input: &MediaInput, budget: u64) -> Result<Self, SearchError> {
        let (source, size) = match input {
            MediaInput::LocalFile { path } => {
                let path = path.canonicalize()?;
                let metadata = tokio::fs::metadata(&path).await?;
                if !metadata.is_file() || metadata.len() == 0 {
                    return Err(SearchError::InvalidRequest(
                        "Media input must be a nonempty regular file".into(),
                    ));
                }
                (ByteSource::File(path), metadata.len())
            }
            MediaInput::DownloadService { url, size_bytes } => {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|error| SearchError::Runtime(error.to_string()))?;
                (
                    ByteSource::Http {
                        client,
                        url: url.clone(),
                    },
                    *size_bytes,
                )
            }
        };
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/media", listener.local_addr()?);
        let state = Arc::new(ReadState::default());
        let task_state = state.clone();
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let task = tokio::spawn(async move {
            loop {
                let work = async {
                    let (mut socket, _) = listener.accept().await?;
                    serve(&mut socket, &source, size, budget, &task_state).await
                };
                tokio::select! {
                    _ = stop.cancelled() => break,
                    result = work => if let Err(error) = result {
                        // Client disconnects during seeks are normal, source failures are recorded separately.
                        if error.kind() != io::ErrorKind::BrokenPipe && error.kind() != io::ErrorKind::ConnectionReset {
                            let mut failure = task_state.failure.lock().unwrap();
                            failure.get_or_insert_with(|| error.to_string());
                        }
                    }
                }
            }
        });
        Ok(Self {
            url,
            size,
            state,
            shutdown,
            task: Some(task),
        })
    }

    pub async fn stop(mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for ReadGateway {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn serve(
    socket: &mut TcpStream,
    source: &ByteSource,
    size: u64,
    budget: u64,
    state: &ReadState,
) -> io::Result<()> {
    let mut header = Vec::new();
    // Read only the header. No request body, persistent connection or pipelining is supported.
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 8192 {
            return reply(socket, 400, "Bad Request").await;
        }
        header.push(socket.read_u8().await?);
    }
    let text = String::from_utf8_lossy(&header);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    let parts: Vec<_> = first.split_whitespace().collect();
    if parts.len() != 3 || parts[1] != "/media" || !["GET", "HEAD"].contains(&parts[0]) {
        return reply(socket, 404, "Not Found").await;
    }
    if parts[0] == "HEAD" {
        return socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n").as_bytes()).await;
    }
    let range = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("range"))
        .map(|(_, value)| value.trim());
    let (start, requested_end) = match range {
        Some(value) => {
            let Some((start, end)) = value
                .strip_prefix("bytes=")
                .and_then(|value| value.split_once('-'))
            else {
                return reply(socket, 416, "Range Not Satisfiable").await;
            };
            let Ok(start) = start.parse::<u64>() else {
                return reply(socket, 416, "Range Not Satisfiable").await;
            };
            let end = if end.is_empty() {
                size - 1
            } else {
                match end.parse::<u64>() {
                    Ok(end) => end,
                    Err(_) => return reply(socket, 416, "Range Not Satisfiable").await,
                }
            };
            (start, end)
        }
        None => (0, size - 1),
    };
    if start >= size || requested_end < start {
        return reply(socket, 416, "Range Not Satisfiable").await;
    }
    let remaining = budget.saturating_sub(state.bytes_read.load(Ordering::Relaxed));
    if remaining == 0 {
        state
            .failure
            .lock()
            .unwrap()
            .get_or_insert("read_budget_exceeded".into());
        return reply(socket, 503, "Read Budget Exceeded").await;
    }
    let end = requested_end
        .min(size - 1)
        .min(start.saturating_add(READ_CHUNK.min(remaining)) - 1);
    let count = end - start + 1;
    state.requests.fetch_add(1, Ordering::Relaxed);
    // Reserve the full range before I/O: even a failed read consumes budget.
    state.bytes_read.fetch_add(count, Ordering::Relaxed);
    let begun = std::time::Instant::now();
    let read = read_range(source, start, end, size).await;
    if std::env::var_os("TORRENT_SEARCH_TRACE_READS").is_some() {
        eprintln!("read {start} +{count} {} ms", begun.elapsed().as_millis());
    }
    let data = match read {
        Ok(data) => data,
        Err(error) => {
            state
                .failure
                .lock()
                .unwrap()
                .get_or_insert_with(|| format!("source_read_failed: {error}"));
            return reply(socket, 502, "Source Read Failed").await;
        }
    };
    socket.write_all(format!("HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\nContent-Length: {count}\r\nContent-Range: bytes {start}-{end}/{size}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    socket.write_all(&data).await
}

async fn read_range(source: &ByteSource, start: u64, end: u64, size: u64) -> io::Result<Vec<u8>> {
    let count = (end - start + 1) as usize;
    match source {
        ByteSource::File(path) => {
            let mut file = tokio::fs::File::open(path).await?;
            file.seek(io::SeekFrom::Start(start)).await?;
            let mut data = vec![0; count];
            file.read_exact(&mut data).await?;
            Ok(data)
        }
        ByteSource::Http { client, url } => {
            let mut response = client
                .get(url)
                .header("Range", format!("bytes={start}-{end}"))
                .header("Accept-Encoding", "identity")
                .send()
                .await
                .map_err(io::Error::other)?;
            let expected = format!("bytes {start}-{end}/{size}");
            if response.status() != 206
                || response
                    .headers()
                    .get("content-range")
                    .and_then(|value| value.to_str().ok())
                    != Some(expected.as_str())
                || response.content_length() != Some(count as u64)
                || response.headers().contains_key("content-encoding")
            {
                return Err(io::Error::other(
                    "Downloader must return exact uncompressed 206 ranges with Content-Length and stable total size",
                ));
            }
            let mut data = Vec::with_capacity(count);
            while let Some(chunk) = response.chunk().await.map_err(io::Error::other)? {
                if data.len() + chunk.len() > count {
                    return Err(io::Error::other("Range body exceeds requested size"));
                }
                data.extend_from_slice(&chunk);
            }
            if data.len() != count {
                return Err(io::Error::other("Incomplete range body"));
            }
            Ok(data)
        }
    }
}

async fn reply(socket: &mut TcpStream, code: u16, reason: &str) -> io::Result<()> {
    socket
        .write_all(
            format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropping_gateway_closes_the_listener_even_if_caller_drops_its_future() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bytes.bin");
        std::fs::write(&path, [1u8; 64]).unwrap();
        let gateway = ReadGateway::start(&MediaInput::LocalFile { path }, 65536)
            .await
            .unwrap();
        let address = gateway
            .url
            .trim_start_matches("http://")
            .trim_end_matches("/media")
            .to_owned();
        drop(gateway);
        tokio::task::yield_now().await;
        assert!(TcpStream::connect(address).await.is_err());
    }
}
