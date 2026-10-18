//! Exercise identity and lifecycle rules independently of rqbit's HTTP implementation.
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchError, SearchLimits, SearchRequest, SearchSource, SourceDescriptor, SourceOutcome,
    TorrentReference,
    media::{MediaInput, MediaInspectionLimits, StreamSelection},
    torrent::*,
};

fn reference() -> TorrentReference {
    TorrentReference::Magnet {
        uri: format!("magnet:?xt=urn:btih:{}", "1".repeat(40)),
        info_hash_v1: Some("1".repeat(40)),
    }
}
fn files() -> Vec<TorrentFile> {
    vec![
        TorrentFile {
            id: 0,
            path: "Season/E01.wav".into(),
            size_bytes: 44 + 2 * 1024 * 1024,
            media_kind: "audio".into(),
        },
        TorrentFile {
            id: 1,
            path: "Season/E02.wav".into(),
            size_bytes: 44 + 4 * 1024 * 1024,
            media_kind: "audio".into(),
        },
        TorrentFile {
            id: 2,
            path: "Season/readme.txt".into(),
            size_bytes: 10,
            media_kind: "other".into(),
        },
    ]
}
fn catalog() -> TorrentCatalog {
    TorrentCatalog {
        schema_version: 1,
        source_reference: reference(),
        resolved_reference: reference(),
        info_hash_v1: "1".repeat(40),
        name: Some("Season".into()),
        files: files(),
    }
}
fn wav() -> Vec<u8> {
    let length = 2u32 * 1024 * 1024;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + length).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(192000u32.to_le_bytes());
    b.extend(4u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(length.to_le_bytes());
    b.resize(44 + length as usize, 0);
    b
}
fn probe() -> PathBuf {
    std::env::var_os("TORRENT_SEARCH_TEST_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| "ffprobe".into())
}

struct Service {
    stopped: bool,
    opened: Arc<AtomicUsize>,
    metadata_calls: usize,
    path: PathBuf,
    mode: &'static str,
    shutdown_flag: Arc<AtomicBool>,
}
impl Service {
    fn new(path: PathBuf, mode: &'static str) -> Self {
        Self {
            stopped: false,
            opened: Arc::new(AtomicUsize::new(usize::MAX)),
            metadata_calls: 0,
            path,
            mode,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}
#[async_trait]
impl TorrentService for Service {
    fn description(&self) -> TorrentServiceRun {
        TorrentServiceRun {
            backend: "independent-test-service".into(),
            working_directory: PathBuf::new(),
            started: true,
            stopped: self.stopped,
            shutdown_error: None,
        }
    }
    async fn metadata(&mut self, _: &TorrentReference) -> Result<TorrentMetadata, TorrentFailure> {
        self.metadata_calls += 1;
        if self.mode == "stall" {
            std::future::pending::<()>().await;
        }
        if self.mode == "malformed" {
            return Err(TorrentFailure::new(
                "torrent_metadata_invalid",
                "Bad metadata",
            ));
        }
        let mut f = files();
        if self.mode == "changed" {
            f[0].size_bytes += 1;
        }
        Ok(TorrentMetadata {
            info_hash_v1: if self.mode == "wrong_hash" {
                "2".repeat(40)
            } else {
                "1".repeat(40)
            },
            name: Some("Season".into()),
            files: f,
        })
    }
    async fn open_file(
        &mut self,
        _: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure> {
        self.opened.store(file.id, Ordering::SeqCst);
        Ok(MediaInput::LocalFile {
            path: self.path.clone(),
        })
    }
    async fn shutdown(&mut self) -> Result<(), TorrentFailure> {
        self.stopped = true;
        self.shutdown_flag.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn lists_then_inspects_explicit_file_and_keeps_identity_and_streams() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("episode.wav");
    std::fs::write(&path, wav()).unwrap();
    let mut service = Service::new(path.clone(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Files {
            reference: reference(),
            resolve_http: true,
        },
        vec![],
        TorrentServiceLimits::default(),
        &mut service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none());
    let report = report.unwrap();
    assert!(report.service.stopped);
    assert_eq!(
        service.opened.load(Ordering::SeqCst),
        usize::MAX,
        "Listing must not start a file download"
    );
    let torrent = report.torrent.unwrap();
    assert_eq!(torrent.files.len(), 3);
    let mut service = Service::new(path, "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Inspect {
            torrent,
            file_id: 0,
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
        },
        vec![],
        TorrentServiceLimits::default(),
        &mut service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none(), "{:?}", error);
    let report = report.unwrap();
    assert!(report.service.stopped);
    assert_eq!(
        service.opened.load(Ordering::SeqCst),
        0,
        "Must not choose the larger episode automatically"
    );
    assert_eq!(report.file.unwrap().path, "Season/E01.wav");
    assert_eq!(report.torrent.unwrap().info_hash_v1, "1".repeat(40));
    let media = report.media.unwrap();
    assert_eq!(media.streams[0].codec.as_deref(), Some("pcm_s16le"));
    assert_eq!(media.streams[0].channels, Some(2));
}

#[tokio::test]
async fn rejects_replaced_metadata_and_always_stops_before_returning() {
    for mode in ["wrong_hash", "changed", "malformed"] {
        let mut service = Service::new("missing".into(), mode);
        let (report, error) = run_with_service(
            TorrentOperation::Inspect {
                torrent: catalog(),
                file_id: 0,
                inspection_limits: MediaInspectionLimits::default(),
                selection: StreamSelection::default(),
            },
            vec![],
            TorrentServiceLimits::default(),
            &mut service,
            &probe(),
            CancellationToken::new(),
        )
        .await;
        assert!(error.is_some());
        assert!(report.unwrap().service.stopped);
        assert!(service.shutdown_flag.load(Ordering::SeqCst));
        assert_eq!(service.opened.load(Ordering::SeqCst), usize::MAX);
    }
}

#[tokio::test]
async fn timeout_cancellation_and_probe_failure_keep_cleanup_and_binding() {
    for cancel in [false, true] {
        let mut service = Service::new("missing".into(), "stall");
        let token = CancellationToken::new();
        if cancel {
            token.cancel();
        }
        let (report, error) = run_with_service(
            TorrentOperation::Files {
                reference: reference(),
                resolve_http: true,
            },
            vec![],
            TorrentServiceLimits { timeout_seconds: 1 },
            &mut service,
            &probe(),
            token,
        )
        .await;
        assert_eq!(
            error.unwrap().code,
            if cancel {
                "cancelled"
            } else {
                "torrent_service_timeout"
            }
        );
        assert!(report.unwrap().service.stopped);
    }
    let mut service = Service::new("missing".into(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Inspect {
            torrent: catalog(),
            file_id: 0,
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
        },
        vec![],
        TorrentServiceLimits::default(),
        &mut service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_some());
    let report = report.unwrap();
    assert!(report.service.stopped);
    assert!(report.torrent.is_some());
    assert_eq!(report.file.unwrap().id, 0);
    assert!(report.media.is_none());
}

#[test]
fn validates_identity_paths_and_selection_before_starting_service() {
    for path in [
        "../movie.mkv",
        "/movie.mkv",
        "C:/movie.mkv",
        "Season/../E01.wav",
        "Season/CON.wav",
        "Season/a\\b.wav",
        "Season/file. ",
    ] {
        let mut c = catalog();
        c.files[0].path = path.into();
        assert!(c.validate().is_err(), "{path}");
    }
    let c = catalog();
    assert!(c.selected_file(2).is_err());
    assert!(c.selected_file(99).is_err());
    let mut c = c;
    c.files[1].path = c.files[0].path.to_ascii_uppercase();
    assert!(c.validate().is_err());
    let bad = TorrentReference::Http {
        uri: reference().uri().into(),
        source_id: "fixture".into(),
        post_form: None,
    };
    assert!(validate_reference(&bad).is_err());
    let mut c = catalog();
    c.info_hash_v1 = "2".repeat(40);
    assert!(c.validate().is_err());
}

struct Source(SourceDescriptor);
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
        unreachable!()
    }
    async fn resolve(
        &self,
        _: &str,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        Ok(reference())
    }
}
#[tokio::test]
async fn preserves_original_search_reference_when_resolving_details() {
    let source = Arc::new(Source(SourceDescriptor {
        id: "fixture".into(),
        name: "fixture".into(),
        site_url: "https://example.org".into(),
        categories: vec!["all".into()],
        resolves_http: true,
        scope: Default::default(),
        language: None,
        alternate_of: None,
    }));
    let original = TorrentReference::Http {
        uri: "https://example.org/details/1".into(),
        source_id: "fixture".into(),
        post_form: None,
    };
    let mut service = Service::new("missing".into(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Files {
            reference: original.clone(),
            resolve_http: true,
        },
        vec![source],
        TorrentServiceLimits::default(),
        &mut service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none());
    let c = report.unwrap().torrent.unwrap();
    assert_eq!(c.source_reference, original);
    assert_eq!(c.resolved_reference, reference());
}
