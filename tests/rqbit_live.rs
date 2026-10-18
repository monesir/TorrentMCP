//! Live check of the rqbit session against a real swarm. Run with:
//! TORRENT_SEARCH_LIVE_MAGNET=magnet:... TORRENT_SEARCH_LIVE_RUNTIME=dist/.../runtime \
//! cargo test --test rqbit_live -- --ignored --nocapture
use std::{path::PathBuf, sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    TorrentReference,
    media::{MediaInspectionLimits, StreamSelection},
    torrent::*,
};

#[tokio::test]
#[ignore = "needs network, rqbit and a healthy magnet"]
async fn one_session_lists_once_and_inspects_several_files_in_parallel() {
    let magnet = std::env::var("TORRENT_SEARCH_LIVE_MAGNET").expect("magnet");
    let runtime_dir = PathBuf::from(std::env::var("TORRENT_SEARCH_LIVE_RUNTIME").expect("runtime"));
    let cache = tempfile::tempdir().unwrap();
    let runtime = TorrentRuntime {
        rqbit: runtime_dir.join("torrent/rqbit.exe"),
        cache_directory: cache.path().into(),
        download_limit_bytes_per_second: None,
    };
    let ffprobe = runtime_dir.join("media/ffprobe.exe");
    let reference = torrent_search::reference::parse_reference(&magnet, "live").unwrap();
    let limits = TorrentServiceLimits {
        timeout_seconds: 120,
    };
    let cancel = CancellationToken::new();

    for round in ["cold", "warm"] {
        let started = Instant::now();
        let service = RqbitService::start(&runtime, &limits, &cancel).await.unwrap();
        let session = service.description().working_directory;
        let (report, error) = perform(
            TorrentOperation::Files {
                reference: reference.clone(),
                resolve_http: false,
            },
            vec![],
            limits.clone(),
            &service,
            &ffprobe,
            cancel.clone(),
        )
        .await;
        assert!(error.is_none(), "{error:?}");
        let torrent = report.torrent.unwrap();
        println!(
            "{round}: metadata for {} files in {} ms",
            torrent.files.len(),
            started.elapsed().as_millis()
        );
        let mut videos: Vec<_> = torrent
            .files
            .iter()
            .filter(|f| f.media_kind == "video")
            .map(|f| f.id)
            .collect();
        videos.truncate(3);
        let inspect_started = Instant::now();
        let targets = videos
            .iter()
            .map(|&file_id| InspectionTarget {
                torrent: Arc::new(torrent.clone()),
                file_id,
            })
            .collect();
        let inspections = inspect_files(
            &service,
            targets,
            &limits,
            &MediaInspectionLimits::default(),
            &StreamSelection::default(),
            &ffprobe,
            3,
            &cancel,
        )
        .await;
        for inspection in &inspections {
            println!(
                "  file {} in {} ms: {}",
                inspection.file_id,
                inspection.elapsed_ms,
                match (&inspection.media, &inspection.failure) {
                    (_, Some(f)) => format!("FAILED {} {}", f.code, f.message),
                    (Some(m), None) => format!("{} streams", m.streams.len()),
                    _ => "no report".into(),
                }
            );
        }
        println!(
            "{round}: {} files inspected in {} ms",
            inspections.len(),
            inspect_started.elapsed().as_millis()
        );
        assert!(inspections.iter().any(|i| i.media.is_some()));
        // Released files leave no partial data behind while the session runs.
        let leftover: u64 = walk(&session.join("torrents"));
        println!("{round}: bytes left in session after release: {leftover}");
        service.shutdown().await.unwrap();
        assert!(!session.exists(), "session directory removed on shutdown");
    }
    assert!(
        cache
            .path()
            .join("metadata")
            .join(format!(
                "{}.torrent",
                match reference {
                    TorrentReference::Magnet { info_hash_v1, .. } => info_hash_v1.unwrap(),
                    _ => unreachable!(),
                }
            ))
            .exists()
    );
}

fn walk(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p)
                    } else {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}
