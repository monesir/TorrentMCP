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
        TorrentFile {
            id: 3,
            path: "Season/E03.wav".into(),
            size_bytes: 44 + 3 * 1024 * 1024,
            media_kind: "audio".into(),
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
    stopped: AtomicBool,
    opened: Arc<AtomicUsize>,
    open_now: AtomicUsize,
    peak_open: AtomicUsize,
    closed: AtomicUsize,
    metadata_calls: AtomicUsize,
    path: PathBuf,
    mode: &'static str,
    shutdown_flag: Arc<AtomicBool>,
}
impl Service {
    fn new(path: PathBuf, mode: &'static str) -> Self {
        Self {
            stopped: AtomicBool::new(false),
            opened: Arc::new(AtomicUsize::new(usize::MAX)),
            open_now: AtomicUsize::new(0),
            peak_open: AtomicUsize::new(0),
            closed: AtomicUsize::new(0),
            metadata_calls: AtomicUsize::new(0),
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
            stopped: self.stopped.load(Ordering::SeqCst),
            shutdown_error: None,
        }
    }
    async fn metadata(&self, _: &TorrentReference) -> Result<TorrentMetadata, TorrentFailure> {
        self.metadata_calls.fetch_add(1, Ordering::SeqCst);
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
        &self,
        _: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure> {
        self.opened.store(file.id, Ordering::SeqCst);
        let now = self.open_now.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_open.fetch_max(now, Ordering::SeqCst);
        // Hold the file long enough for concurrent opens to overlap.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(MediaInput::LocalFile {
            path: self.path.clone(),
        })
    }
    async fn close_file(&self, _: &TorrentCatalog, _: &TorrentFile) -> Result<(), TorrentFailure> {
        self.open_now.fetch_sub(1, Ordering::SeqCst);
        self.closed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), TorrentFailure> {
        self.stopped.store(true, Ordering::SeqCst);
        self.shutdown_flag.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn lists_then_inspects_explicit_file_and_keeps_identity_and_streams() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("episode.wav");
    std::fs::write(&path, wav()).unwrap();
    let service = Service::new(path.clone(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Files {
            reference: reference(),
            resolve_http: true,
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
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
    assert_eq!(torrent.files.len(), 4);
    let service = Service::new(path, "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Inspect {
            torrent,
            file_id: 0,
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
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
        let service = Service::new("missing".into(), mode);
        let (report, error) = run_with_service(
            TorrentOperation::Inspect {
                torrent: catalog(),
                file_id: 0,
                inspection_limits: MediaInspectionLimits::default(),
                selection: StreamSelection::default(),
            },
            vec![],
            TorrentServiceLimits::default(),
            &service,
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
        let service = Service::new("missing".into(), "stall");
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
            &service,
            &probe(),
            token,
        )
        .await;
        let error = error.unwrap();
        assert_eq!(
            error.code,
            if cancel {
                "cancelled"
            } else {
                "torrent_service_timeout"
            }
        );
        assert_eq!(!cancel, error.message.contains("no peer supplied it"));
        assert!(report.unwrap().service.stopped);
    }
    let service = Service::new("missing".into(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Inspect {
            torrent: catalog(),
            file_id: 0,
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
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
    let service = Service::new("missing".into(), "ok");
    let (report, error) = run_with_service(
        TorrentOperation::Files {
            reference: original.clone(),
            resolve_http: true,
        },
        vec![source],
        TorrentServiceLimits::default(),
        &service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none());
    let c = report.unwrap().torrent.unwrap();
    assert_eq!(c.source_reference, original);
    assert_eq!(c.resolved_reference, reference());
}

#[tokio::test]
async fn inspects_several_files_concurrently_in_order_and_releases_each() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("episode.wav");
    std::fs::write(&path, wav()).unwrap();
    let service = Service::new(path, "ok");
    let (report, error) = run_with_service(
        TorrentOperation::InspectFiles {
            source: TorrentSource::Catalog(catalog()),
            files: FileChoice::Ids(vec![1, 0, 3]),
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
            parallel: 4,
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none(), "{error:?}");
    let report = report.unwrap();
    assert!(report.service.stopped);
    let ids: Vec<_> = report.inspections.iter().map(|i| i.file_id).collect();
    assert_eq!(ids, [1, 0, 3], "results keep the requested order");
    assert!(report.inspections.iter().all(|i| i.media.is_some()));
    assert_eq!(
        service.peak_open.load(Ordering::SeqCst),
        2,
        "after a fast first file the rest open together"
    );
    assert_eq!(service.closed.load(Ordering::SeqCst), 3);
    assert_eq!(service.open_now.load(Ordering::SeqCst), 0);
    assert_eq!(service.metadata_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_shared_service_survives_operations_and_releases_files_after_failures() {
    let service = Service::new("missing".into(), "ok");
    for _ in 0..2 {
        let (report, error) = perform(
            TorrentOperation::InspectFiles {
                source: TorrentSource::Catalog(catalog()),
                files: FileChoice::Ids(vec![0, 1]),
                inspection_limits: MediaInspectionLimits::default(),
                selection: StreamSelection::default(),
                parallel: 1,
            },
            vec![],
            TorrentServiceLimits::default(),
            &service,
            &probe(),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(error.unwrap().code, "all_inspections_failed");
        assert!(report.inspections.iter().all(|i| i.failure.is_some()));
        assert!(
            !report.service.stopped,
            "perform leaves a shared service running"
        );
    }
    assert_eq!(
        service.peak_open.load(Ordering::SeqCst),
        1,
        "parallel 1 is sequential"
    );
    assert_eq!(service.closed.load(Ordering::SeqCst), 4);
    assert_eq!(service.open_now.load(Ordering::SeqCst), 0);
    let (_, error) = perform(
        TorrentOperation::InspectFiles {
            source: TorrentSource::Catalog(catalog()),
            files: FileChoice::Ids(vec![0, 2]),
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
            parallel: 2,
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(
        error.is_some(),
        "a non-media file is rejected before any read"
    );
    let slow = Service::new("missing".into(), "ok");
    let (report, _) = perform(
        TorrentOperation::InspectFiles {
            source: TorrentSource::Catalog(catalog()),
            files: FileChoice::Ids(vec![0, 1, 3]),
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
            parallel: 4,
        },
        vec![],
        TorrentServiceLimits::default(),
        &slow,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(report.inspections.len(), 3);
    let (_, error) = perform(
        TorrentOperation::InspectFiles {
            source: TorrentSource::Catalog(catalog()),
            files: FileChoice::Ids(vec![0, 0]),
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
            parallel: 2,
        },
        vec![],
        TorrentServiceLimits::default(),
        &slow,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(error.unwrap().code, "invalid_request", "a repeated file id");
    assert_eq!(
        slow.peak_open.load(Ordering::SeqCst),
        1,
        "a failed first file keeps the rest sequential"
    );
    assert_eq!(service.closed.load(Ordering::SeqCst), 4);
}

#[test]
fn sampling_picks_a_typical_video_or_spreads_over_episodes_skipping_extras() {
    let file = |id: usize, path: &str, size: u64| TorrentFile {
        id,
        path: path.into(),
        size_bytes: size,
        media_kind: media_kind(path).into(),
    };
    let mut c = catalog();
    c.files = vec![
        file(0, "Show/S01E03.mkv", 900),
        file(1, "Show/S01E01.mkv", 1000),
        file(2, "Show/Sample/sample.mkv", 50),
        file(3, "Show/S01E02.mkv", 950),
        file(4, "Show/S01E04.mkv", 980),
        file(5, "Show/notes.nfo", 10),
        file(6, "Show/Extras/featurette.mkv", 100),
        file(7, "Show/S01E05.mkv", 990),
    ];
    assert_eq!(sample_files(&c, 1), [4], "median-sized episode");
    let mut with_movies = c.clone();
    with_movies.files.push(file(8, "Show/Movie 1.mkv", 4000));
    with_movies.files.push(file(9, "Show/Movie 2.mkv", 4100));
    assert_eq!(
        sample_files(&with_movies, 1),
        [7],
        "an episode, not a bundled movie"
    );
    assert_eq!(
        sample_files(&c, 3),
        [1, 0, 7],
        "first, middle, last episode"
    );
    assert_eq!(
        sample_files(&c, 20).len(),
        5,
        "never more than the main videos"
    );
    c.files = vec![file(0, "readme.txt", 10)];
    assert!(sample_files(&c, 3).is_empty());
}

#[tokio::test]
async fn inspect_torrent_reads_a_reference_and_samples_files_in_one_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("episode.wav");
    std::fs::write(&path, wav()).unwrap();
    let service = Service::new(path, "ok");
    let (report, error) = run_with_service(
        TorrentOperation::InspectFiles {
            source: TorrentSource::Reference {
                reference: reference(),
                resolve_http: true,
            },
            files: FileChoice::Sample(1),
            inspection_limits: MediaInspectionLimits::default(),
            selection: StreamSelection::default(),
            parallel: 2,
        },
        vec![],
        TorrentServiceLimits::default(),
        &service,
        &probe(),
        CancellationToken::new(),
    )
    .await;
    assert!(error.is_none(), "{error:?}");
    let report = report.unwrap();
    assert_eq!(report.operation, "inspect_torrent");
    assert!(report.torrent.is_some());
    assert_eq!(service.metadata_calls.load(Ordering::SeqCst), 1);
    // An audio-only torrent samples its median-sized audio file.
    assert_eq!(report.inspections.len(), 1);
    assert_eq!(report.inspections[0].file_id, 3);
    assert!(report.inspections[0].media.is_some());
}

#[test]
fn a_pasted_magnet_needs_no_separate_info_hash() {
    let hash = "a".repeat(40);
    let mut pasted = TorrentReference::Magnet {
        uri: format!("magnet:?xt=urn:btih:{}&dn=Show", hash.to_uppercase()),
        info_hash_v1: None,
    };
    assert!(validate_reference(&pasted).is_ok());
    complete_reference(&mut pasted);
    assert!(
        matches!(&pasted, TorrentReference::Magnet { info_hash_v1: Some(h), .. } if *h == hash)
    );
    let wrong = TorrentReference::Magnet {
        uri: format!("magnet:?xt=urn:btih:{hash}"),
        info_hash_v1: Some("b".repeat(40)),
    };
    assert!(
        validate_reference(&wrong).is_err(),
        "a supplied hash must match"
    );
}
