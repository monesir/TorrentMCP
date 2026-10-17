//! Controlled WAV containers exercise I/O contracts; real multi-track MKV evidence is in verification/.
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use torrent_search::media::{
    MediaInput, MediaInspectionLimits, MediaInspectionRequest, StreamSelection, inspect_media,
};

fn probe() -> PathBuf {
    std::env::var_os("TORRENT_SEARCH_TEST_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| "ffprobe".into())
}

fn wav() -> Vec<u8> {
    // Longer than ffprobe's analysis window, so a successful probe need not read the whole file.
    let data_size = 8u32 * 1024 * 1024;
    let mut data = Vec::new();
    data.extend(b"RIFF");
    data.extend((36 + data_size).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(48000u32.to_le_bytes());
    data.extend(192000u32.to_le_bytes());
    data.extend(4u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(data_size.to_le_bytes());
    data.resize(44 + data_size as usize, 0);
    data
}

fn request(input: MediaInput) -> MediaInspectionRequest {
    MediaInspectionRequest {
        input,
        limits: MediaInspectionLimits::default(),
        selection: StreamSelection::default(),
    }
}

// Independent mock downloader: no dependency on the production gateway implementation.
async fn downloader(
    data: Arc<Vec<u8>>,
    mode: &'static str,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/file", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut delayed = false;
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            if mode == "stall" {
                std::future::pending::<()>().await;
            }
            if mode == "slow_first_piece" && !delayed {
                delayed = true;
                tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            }
            let header = String::from_utf8(header).unwrap();
            let range = header
                .lines()
                .find_map(|line| {
                    line.strip_prefix("range: ")
                        .or_else(|| line.strip_prefix("Range: "))
                })
                .unwrap();
            let (start, end) = range
                .strip_prefix("bytes=")
                .unwrap()
                .split_once('-')
                .unwrap();
            let start: usize = start.parse().unwrap();
            let end: usize = end.parse().unwrap();
            let response = if mode == "ignore_range" {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                )
            } else {
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                    end - start + 1,
                    data.len()
                )
            };
            let _ = socket.write_all(response.as_bytes()).await;
            if mode != "ignore_range" {
                let _ = socket.write_all(&data[start..=end]).await;
            }
        }
    });
    (url, task)
}

#[tokio::test]
async fn waits_for_a_cold_first_piece_within_the_requested_deadline() {
    let data = Arc::new(wav());
    let (url, task) = downloader(data.clone(), "slow_first_piece").await;
    let mut input = request(MediaInput::DownloadService {
        url,
        size_bytes: data.len() as u64,
    });
    input.limits.timeout_seconds = 15;
    let outcome = inspect_media(input, &probe(), CancellationToken::new())
        .await
        .unwrap();
    task.abort();
    assert!(outcome.failure_code.is_none(), "{:?}", outcome.diagnostic);
    assert_eq!(outcome.report.unwrap().streams[0].channels, Some(2));
}

#[tokio::test]
async fn local_probe_reads_real_container_metadata_and_keeps_unknown_language() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("audio with spaces.wav");
    std::fs::write(&file, wav()).unwrap();
    let mut input = request(MediaInput::LocalFile { path: file });
    input.selection.audio_languages = vec!["ar".into()];
    let outcome = inspect_media(input, &probe(), CancellationToken::new())
        .await
        .unwrap();
    assert!(outcome.failure_code.is_none(), "{:?}", outcome.diagnostic);
    let report = outcome.report.unwrap();
    assert_eq!(report.streams.len(), 1);
    assert_eq!(report.streams[0].codec.as_deref(), Some("pcm_s16le"));
    assert_eq!(report.streams[0].channels, Some(2));
    assert_eq!(report.streams[0].bitrate_bps.value, Some(1536000));
    assert!(report.streams[0].language.is_none());
    assert_eq!(report.selection.unclassified_stream_indices, vec![0]);
    assert_eq!(report.selection.missing_audio_languages, vec!["ar"]);
    assert!(report.read_bytes_charged < report.format.file_size_bytes);
    assert!(!report.selection.changes_applied);
}

#[tokio::test]
async fn reads_ranges_from_download_service_without_loading_the_full_file() {
    let data = Arc::new(wav());
    let (url, task) = downloader(data.clone(), "ranges").await;
    let result = inspect_media(
        request(MediaInput::DownloadService {
            url,
            size_bytes: data.len() as u64,
        }),
        &probe(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    task.abort();
    assert!(result.failure_code.is_none(), "{:?}", result.diagnostic);
    let report = result.report.unwrap();
    assert_eq!(report.streams[0].sample_rate_hz, Some(48000));
    assert!(report.read_bytes_charged < data.len() as u64);
    assert!(report.read_requests >= 1);
}

#[tokio::test]
async fn enforces_read_budget_and_reports_partial_metadata_instead_of_complete() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("budget.wav");
    std::fs::write(&file, wav()).unwrap();
    let mut input = request(MediaInput::LocalFile { path: file });
    input.limits.max_read_bytes = 65536;
    let outcome = inspect_media(input, &probe(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        outcome.failure_code.as_deref(),
        Some("read_budget_exceeded")
    );
    if let Some(report) = outcome.report {
        assert!(report.read_bytes_charged <= 65536);
    }
}

#[tokio::test]
async fn rejects_downloader_that_ignores_ranges() {
    let data = Arc::new(wav());
    let (url, task) = downloader(data.clone(), "ignore_range").await;
    let outcome = inspect_media(
        request(MediaInput::DownloadService {
            url,
            size_bytes: data.len() as u64,
        }),
        &probe(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    task.abort();
    assert_eq!(outcome.failure_code.as_deref(), Some("source_read_failed"));
}

#[tokio::test]
async fn timeout_and_cancellation_stop_stalled_source_reads() {
    for cancelled in [false, true] {
        let data = Arc::new(wav());
        let (url, task) = downloader(data.clone(), "stall").await;
        let mut input = request(MediaInput::DownloadService {
            url,
            size_bytes: data.len() as u64,
        });
        input.limits.timeout_seconds = 1;
        let token = CancellationToken::new();
        let signal = token.clone();
        let signal_task = tokio::spawn(async move {
            if cancelled {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                signal.cancel();
            }
        });
        let outcome = inspect_media(input, &probe(), token).await.unwrap();
        signal_task.await.unwrap();
        task.abort();
        assert_eq!(
            outcome.failure_code.as_deref(),
            Some(if cancelled {
                "cancelled"
            } else {
                "probe_timeout"
            })
        );
    }
}

#[test]
fn rejects_public_urls_hostnames_and_unknown_request_fields() {
    for url in [
        "https://example.org/file",
        "http://localhost/file",
        "http://127.0.0.1@evil.test/file",
        "file:///video.mkv",
    ] {
        assert!(
            request(MediaInput::DownloadService {
                url: url.into(),
                size_bytes: 100
            })
            .validate()
            .is_err()
        );
    }
    let invalid = json!({"input":{"kind":"local_file","path":"file.mkv"}, "limits":{"max_reed_bytes": 65536}});
    assert!(serde_json::from_value::<MediaInspectionRequest>(invalid).is_err());
}
