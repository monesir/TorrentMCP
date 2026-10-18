//! TorboxFirst against a local fake of the TorBox API.
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use torrent_search::{
    TorrentReference,
    media::MediaInput,
    torrent::{
        TorboxClient, TorboxFirst, TorboxKey, TorrentCatalog, TorrentFailure, TorrentFile,
        TorrentMetadata, TorrentService, TorrentServiceRun,
    },
};

const CACHED: &str = "1111111111111111111111111111111111111111";
const SIZE: u64 = 5000;

#[derive(Default)]
struct Api {
    requests: Mutex<Vec<String>>,
    deleted: AtomicUsize,
    /// Whether the file link answers ranges with 206.
    ranges: bool,
}

/// Serve one HTTP request per connection, like the API's answers.
async fn fake_api(api: Arc<Api>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let link = format!("{base}/file");
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let api = api.clone();
            let link = link.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 65536];
                let mut read = 0;
                let head_end = loop {
                    let n = socket.read(&mut buffer[read..]).await.unwrap();
                    read += n;
                    if let Some(end) = buffer[..read].windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                    if n == 0 {
                        return;
                    }
                };
                let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while read < head_end + length {
                    read += socket.read(&mut buffer[read..]).await.unwrap();
                }
                let line = head.lines().next().unwrap().to_owned();
                let target = line.split(' ').nth(1).unwrap().to_owned();
                api.requests.lock().unwrap().push(line.clone());
                let authorized = head.contains("Bearer key");
                let (status, extra, body) = if target.starts_with("/file") {
                    if api.ranges {
                        (
                            206,
                            format!("Content-Range: bytes 0-0/{SIZE}\r\n"),
                            "x".to_owned(),
                        )
                    } else {
                        (200, String::new(), "x".to_owned())
                    }
                } else if !authorized {
                    (
                        401,
                        String::new(),
                        r#"{"success":false,"detail":"Bad token"}"#.to_owned(),
                    )
                } else if target.starts_with("/torrents/checkcached") {
                    let data = if target.contains(CACHED) {
                        format!(r#"{{"{CACHED}":{{"name":"Show","size":{SIZE}}}}}"#)
                    } else {
                        "{}".into()
                    };
                    (
                        200,
                        String::new(),
                        format!(r#"{{"success":true,"data":{data}}}"#),
                    )
                } else if target.starts_with("/torrents/createtorrent") {
                    (
                        200,
                        String::new(),
                        r#"{"success":true,"data":{"torrent_id":42}}"#.into(),
                    )
                } else if target.starts_with("/torrents/mylist") {
                    (
                        200,
                        String::new(),
                        format!(
                            r#"{{"success":true,"data":{{"id":42,"hash":"{CACHED}","download_present":true,"files":[{{"id":8,"name":"Show/Season/E01.mkv","size":{SIZE}}},{{"id":5,"name":"Show/Season/E02.mkv","size":{SIZE}}}]}}}}"#
                        ),
                    )
                } else if target.starts_with("/torrents/requestdl") {
                    assert!(
                        target.contains("file_id=5"),
                        "matched by path, not numbering"
                    );
                    (
                        200,
                        String::new(),
                        format!(r#"{{"success":true,"data":"{link}"}}"#),
                    )
                } else if target.starts_with("/torrents/controltorrent") {
                    api.deleted.fetch_add(1, Ordering::SeqCst);
                    (200, String::new(), r#"{"success":true,"data":null}"#.into())
                } else {
                    (404, String::new(), "{}".into())
                };
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

#[derive(Default)]
struct Peers {
    opened: AtomicUsize,
    closed: AtomicUsize,
}

#[async_trait]
impl TorrentService for Peers {
    fn description(&self) -> TorrentServiceRun {
        TorrentServiceRun {
            backend: "peers".into(),
            working_directory: PathBuf::new(),
            started: true,
            stopped: false,
            shutdown_error: None,
            notes: vec![],
        }
    }
    async fn metadata(&self, _: &TorrentReference) -> Result<TorrentMetadata, TorrentFailure> {
        unreachable!("metadata is not needed here")
    }
    async fn open_file(
        &self,
        _: &TorrentCatalog,
        _: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(MediaInput::LocalFile {
            path: "peers".into(),
        })
    }
    async fn close_file(&self, _: &TorrentCatalog, _: &TorrentFile) -> Result<(), TorrentFailure> {
        self.closed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), TorrentFailure> {
        Ok(())
    }
}

fn catalog(hash: &str) -> (TorrentCatalog, TorrentFile) {
    let file = TorrentFile {
        id: 1,
        path: "Season/E02.mkv".into(),
        size_bytes: SIZE,
        media_kind: "video".into(),
    };
    let reference = TorrentReference::Magnet {
        uri: format!("magnet:?xt=urn:btih:{hash}"),
        info_hash_v1: Some(hash.into()),
    };
    let torrent = TorrentCatalog {
        schema_version: 1,
        source_reference: reference.clone(),
        resolved_reference: reference,
        info_hash_v1: hash.into(),
        name: Some("Show".into()),
        files: vec![
            TorrentFile {
                id: 0,
                path: "Season/E01.mkv".into(),
                size_bytes: SIZE,
                media_kind: "video".into(),
            },
            file.clone(),
        ],
    };
    (torrent, file)
}

async fn service<'a>(peers: &'a Peers, api: Arc<Api>, key: &str) -> TorboxFirst<'a> {
    let base = fake_api(api).await;
    TorboxFirst::new(
        peers,
        TorboxClient::with_base(TorboxKey::new(key).unwrap(), &base).unwrap(),
    )
}

#[tokio::test]
async fn a_cached_torrent_is_read_from_torbox_and_removed_after() {
    let api = Arc::new(Api {
        ranges: true,
        ..Default::default()
    });
    let peers = Peers::default();
    let torbox = service(&peers, api.clone(), "key").await;
    let (torrent, file) = catalog(CACHED);
    let input = torbox.open_file(&torrent, &file).await.unwrap();
    assert!(
        matches!(&input, MediaInput::RemoteFile { size_bytes, .. } if *size_bytes == SIZE),
        "{input:?}"
    );
    assert_eq!(peers.opened.load(Ordering::SeqCst), 0);
    torbox.close_file(&torrent, &file).await.unwrap();
    assert_eq!(
        peers.closed.load(Ordering::SeqCst),
        0,
        "peers never held it"
    );
    assert_eq!(
        api.deleted.load(Ordering::SeqCst),
        1,
        "the torrent this run added"
    );
    assert!(torbox.description().backend.starts_with("torbox+"));
    assert!(torbox.description().notes.is_empty());
}

#[tokio::test]
async fn uncached_unranged_or_unauthorized_fall_back_to_peers_with_a_reason() {
    for (hash, ranges, key, reason) in [
        (
            "2222222222222222222222222222222222222222",
            true,
            "key",
            "not cached",
        ),
        (CACHED, false, "key", "byte ranges"),
        (CACHED, true, "wrong", "torbox_unauthorized"),
    ] {
        let api = Arc::new(Api {
            ranges,
            ..Default::default()
        });
        let peers = Peers::default();
        let torbox = service(&peers, api.clone(), key).await;
        let (torrent, file) = catalog(hash);
        let input = torbox.open_file(&torrent, &file).await.unwrap();
        assert!(matches!(input, MediaInput::LocalFile { .. }), "{reason}");
        torbox.close_file(&torrent, &file).await.unwrap();
        assert_eq!(peers.opened.load(Ordering::SeqCst), 1, "{reason}");
        assert_eq!(peers.closed.load(Ordering::SeqCst), 1, "{reason}");
        let notes = torbox.description().notes;
        assert!(
            notes.iter().any(|n| n.contains(reason)),
            "{reason}: {notes:?}"
        );
        assert!(
            notes
                .iter()
                .all(|n| !n.contains("key") || n.contains("torbox_unauthorized")),
            "notes never carry the key"
        );
        torbox.shutdown().await.unwrap();
    }
}
