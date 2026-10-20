use super::*;
use crate::{
    SearchEngine, SearchLimits, SearchSource,
    media::{MediaInspectionLimits, MediaInspectionRequest, StreamSelection, inspect_media},
};
use futures_util::{StreamExt, stream};
use std::{path::Path, sync::Arc, time::Instant};

pub enum TorrentOperation {
    Files {
        reference: TorrentReference,
        resolve_http: bool,
    },
    Inspect {
        torrent: TorrentCatalog,
        file_id: usize,
        inspection_limits: MediaInspectionLimits,
        selection: StreamSelection,
    },
    /// Several files of one torrent, read concurrently through one service.
    InspectFiles {
        source: TorrentSource,
        files: FileChoice,
        inspection_limits: MediaInspectionLimits,
        selection: StreamSelection,
        parallel: usize,
    },
}

/// Where the torrent comes from: a search reference read now, or a catalog read earlier.
pub enum TorrentSource {
    Reference {
        reference: TorrentReference,
        resolve_http: bool,
    },
    Catalog(TorrentCatalog),
}

pub enum FileChoice {
    Ids(Vec<usize>),
    /// Up to this many video files chosen by `sample_files`.
    Sample(usize),
}

impl TorrentOperation {
    fn name(&self) -> &'static str {
        match self {
            Self::Files { .. } => "torrent_files",
            Self::Inspect { .. } => "inspect_torrent_file",
            Self::InspectFiles { .. } => "inspect_torrent",
        }
    }
}

/// The episodes or the film: video files at least a fifth of the largest
/// one's size, which skips samples and extras. Audio-only torrents (music,
/// audiobooks) yield their audio files.
pub(crate) fn main_videos(torrent: &TorrentCatalog) -> Vec<&TorrentFile> {
    let of_kind = |kind: &str| -> Vec<&TorrentFile> {
        torrent
            .files
            .iter()
            .filter(|f| f.media_kind == kind && f.size_bytes > 0)
            .collect()
    };
    let mut videos = of_kind("video");
    if videos.is_empty() {
        videos = of_kind("audio");
    }
    let largest = videos.iter().map(|f| f.size_bytes).max().unwrap_or(0);
    videos.retain(|f| f.size_bytes >= largest / 5);
    videos
}

/// Choose video files to inspect without the caller reading the catalog.
///
/// Main videos are those at least a fifth of the largest video's size, which
/// skips samples and extras (audio files when there is no video). One file:
/// the median-sized main video. Several: spread evenly over the main videos
/// in path order (first, middle, last episode).
pub fn sample_files(torrent: &TorrentCatalog, count: usize) -> Vec<usize> {
    let mut videos = main_videos(torrent);
    if videos.is_empty() {
        return vec![];
    }
    if count <= 1 {
        // The median size is a typical episode, not a movie bundled with the
        // series; with a lone main video it is that video.
        videos.sort_by_key(|f| (f.size_bytes, f.id));
        return vec![videos[(videos.len() - 1) / 2].id];
    }
    videos.sort_by(|a, b| a.path.cmp(&b.path));
    let count = count.min(videos.len());
    let mut ids: Vec<_> = (0..count)
        .map(|i| videos[i * (videos.len() - 1) / (count - 1).max(1)].id)
        .collect();
    ids.dedup();
    ids
}

/// The most files read at once; each holds an ffprobe process and peer bandwidth.
pub const MAX_PARALLEL_INSPECTIONS: usize = 8;
pub const MAX_FILES_PER_REQUEST: usize = 32;

/// One file to inspect: a catalog the caller obtained earlier and a file in it.
#[derive(Debug, Clone)]
pub struct InspectionTarget {
    pub torrent: Arc<TorrentCatalog>,
    pub file_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionFailure {
    pub code: String,
    pub message: String,
}

impl From<TorrentFailure> for InspectionFailure {
    fn from(failure: TorrentFailure) -> Self {
        Self {
            code: failure.code.into(),
            message: failure.message,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInspection {
    pub info_hash_v1: String,
    pub file_id: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<TorrentFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<MediaInspectionReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<InspectionFailure>,
    pub elapsed_ms: u64,
    /// Set by the agent when the request carries a media analysis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<crate::comparison::MediaAssessment>,
}

pub async fn run(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    runtime: &TorrentRuntime,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (Option<TorrentOperationReport>, Option<TorrentFailure>) {
    let service = match RqbitService::start(runtime, &limits, &cancellation).await {
        Ok(service) => service,
        Err(error) => return (None, Some(error)),
    };
    match super::torbox_first(runtime, &service) {
        Some(torbox) => {
            run_with_service(operation, sources, limits, &torbox, ffprobe, cancellation).await
        }
        None => run_with_service(operation, sources, limits, &service, ffprobe, cancellation).await,
    }
}

/// Run one operation, then shut the service down. Also used by other
/// download-service implementations and protocol integration tests.
pub async fn run_with_service(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    service: &dyn TorrentService,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (Option<TorrentOperationReport>, Option<TorrentFailure>) {
    let (mut report, mut failure) =
        perform(operation, sources, limits, service, ffprobe, cancellation).await;
    // Always wait for owned service shutdown, including cancellation, timeout and malformed metadata.
    let shutdown =
        tokio::time::timeout(std::time::Duration::from_secs(5), service.shutdown()).await;
    report.service = service.description();
    let shutdown_error = match shutdown {
        Ok(Ok(())) if report.service.stopped => None,
        Ok(Err(e)) => Some(e.message),
        _ => Some("Torrent service shutdown was not confirmed".into()),
    };
    if let Some(message) = shutdown_error {
        report.service.shutdown_error = Some(message.clone());
        if failure.is_none() {
            failure = Some(TorrentFailure::new(
                "torrent_service_shutdown_failed",
                message,
            ));
        }
    }
    (Some(report), failure)
}

/// Run one operation on a shared service, leaving it running for the next.
pub async fn perform(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    service: &dyn TorrentService,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (TorrentOperationReport, Option<TorrentFailure>) {
    let mut report = TorrentOperationReport {
        schema_version: 1,
        operation: operation.name().into(),
        torrent: None,
        file: None,
        media: None,
        inspections: vec![],
        service: service.description(),
        assessment: None,
    };
    let failure = match operation {
        TorrentOperation::Files {
            reference,
            resolve_http,
        } => bounded(limits.timeout_seconds, &cancellation, async {
            let resolved = if resolve_http {
                resolve_reference(&reference, sources, &limits, cancellation.clone()).await?
            } else {
                reference.clone()
            };
            let metadata = service.metadata(&resolved).await?;
            report.torrent = Some(catalog(reference, resolved, metadata)?);
            Ok(())
        })
        .await
        .map_err(metadata_timeout(limits.timeout_seconds))
        .err(),
        TorrentOperation::Inspect {
            torrent,
            file_id,
            inspection_limits,
            selection,
        } => {
            let torrent = Arc::new(torrent);
            match bounded(
                limits.timeout_seconds,
                &cancellation,
                verify_catalog(service, &torrent, &[file_id]),
            )
            .await
            .map_err(metadata_timeout(limits.timeout_seconds))
            {
                Err(error) => Some(error),
                Ok(()) => {
                    report.torrent = Some((*torrent).clone());
                    report.file = torrent.files.get(file_id).cloned();
                    let inspection = inspect_target(
                        service,
                        InspectionTarget { torrent, file_id },
                        &limits,
                        &inspection_limits,
                        &selection,
                        ffprobe,
                        &cancellation,
                    )
                    .await;
                    report.media = inspection.media;
                    inspection.failure.map(|failure| TorrentFailure {
                        code: static_code(&failure.code),
                        message: failure.message,
                    })
                }
            }
        }
        TorrentOperation::InspectFiles {
            source,
            files,
            inspection_limits,
            selection,
            parallel,
        } => {
            let ready = bounded(limits.timeout_seconds, &cancellation, async {
                let torrent = match source {
                    TorrentSource::Reference {
                        reference,
                        resolve_http,
                    } => {
                        let resolved = if resolve_http {
                            resolve_reference(&reference, sources, &limits, cancellation.clone())
                                .await?
                        } else {
                            reference.clone()
                        };
                        let metadata = service.metadata(&resolved).await?;
                        catalog(reference, resolved, metadata)?
                    }
                    TorrentSource::Catalog(torrent) => {
                        verify_catalog(service, &torrent, &[]).await?;
                        torrent
                    }
                };
                let file_ids = match files {
                    FileChoice::Ids(ids) => ids,
                    FileChoice::Sample(count) => sample_files(&torrent, count),
                };
                if file_ids.is_empty() || file_ids.len() > MAX_FILES_PER_REQUEST {
                    return Err(TorrentFailure::new(
                        "no_media_files",
                        "Select 1..32 nonempty video or audio files to inspect",
                    ));
                }
                if has_duplicates(&file_ids) {
                    return Err(TorrentFailure::new(
                        "invalid_request",
                        "file_ids must not repeat a file",
                    ));
                }
                for &id in &file_ids {
                    torrent.selected_file(id)?;
                }
                Ok((Arc::new(torrent), file_ids))
            })
            .await
            .map_err(metadata_timeout(limits.timeout_seconds));
            match ready {
                Err(error) => Some(error),
                Ok((torrent, file_ids)) => {
                    report.torrent = Some((*torrent).clone());
                    let targets = file_ids
                        .iter()
                        .map(|&file_id| InspectionTarget {
                            torrent: torrent.clone(),
                            file_id,
                        })
                        .collect();
                    report.inspections = inspect_files(
                        service,
                        targets,
                        &limits,
                        &inspection_limits,
                        &selection,
                        ffprobe,
                        parallel,
                        &cancellation,
                    )
                    .await;
                    if cancellation.is_cancelled() {
                        Some(cancelled())
                    } else if report.inspections.iter().all(|i| i.failure.is_some()) {
                        Some(TorrentFailure::new(
                            "all_inspections_failed",
                            "No selected file could be inspected",
                        ))
                    } else {
                        None
                    }
                }
            }
        }
    };
    report.service = service.description();
    (report, failure)
}

/// Inspect files concurrently through one shared service, keeping the input order.
/// Catalogs must come from this service's metadata (see `verify_catalog`).
#[allow(clippy::too_many_arguments)]
/// A first file inspected within this time shows a swarm fast enough to feed
/// several readers at once.
pub const FAST_FIRST_INSPECTION_MS: u64 = 15_000;

/// Inspect `targets`, keeping their order. The first file runs alone: if it
/// succeeds within `FAST_FIRST_INSPECTION_MS` the rest run `parallel` at a
/// time; otherwise one at a time, because readers splitting a slow swarm's
/// bandwidth all reach their timeout instead of some finishing.
#[allow(clippy::too_many_arguments)]
pub async fn inspect_files(
    service: &dyn TorrentService,
    targets: Vec<InspectionTarget>,
    limits: &TorrentServiceLimits,
    inspection_limits: &MediaInspectionLimits,
    selection: &StreamSelection,
    ffprobe: &Path,
    parallel: usize,
    cancellation: &CancellationToken,
) -> Vec<FileInspection> {
    let inspect = |target| {
        inspect_target(
            service,
            target,
            limits,
            inspection_limits,
            selection,
            ffprobe,
            cancellation,
        )
    };
    let mut targets = targets.into_iter();
    let Some(first) = targets.next() else {
        return Vec::new();
    };
    let first = inspect(first).await;
    let fast = first.failure.is_none() && first.elapsed_ms <= FAST_FIRST_INSPECTION_MS;
    let parallel = if fast {
        parallel.clamp(1, MAX_PARALLEL_INSPECTIONS)
    } else {
        1
    };
    let mut inspections = vec![first];
    inspections.extend(
        stream::iter(targets)
            .map(inspect)
            .buffered(parallel)
            .collect::<Vec<_>>()
            .await,
    );
    inspections
}

pub fn has_duplicates(ids: &[usize]) -> bool {
    let mut seen = std::collections::HashSet::new();
    !ids.iter().all(|id| seen.insert(id))
}

/// Check a caller's catalog against the service's metadata for that infohash.
pub async fn verify_catalog(
    service: &dyn TorrentService,
    torrent: &TorrentCatalog,
    file_ids: &[usize],
) -> Result<(), TorrentFailure> {
    torrent.validate()?;
    if file_ids.len() > MAX_FILES_PER_REQUEST {
        return Err(TorrentFailure::new(
            "invalid_request",
            "Select at most 32 files to inspect",
        ));
    }
    if has_duplicates(file_ids) {
        return Err(TorrentFailure::new(
            "invalid_request",
            "file_ids must not repeat a file",
        ));
    }
    for &id in file_ids {
        torrent.selected_file(id)?;
    }
    let metadata = service.metadata(&torrent.resolved_reference).await?;
    let actual = catalog(
        torrent.source_reference.clone(),
        torrent.resolved_reference.clone(),
        metadata,
    )?;
    if actual.info_hash_v1 != torrent.info_hash_v1 || actual.files != torrent.files {
        return Err(TorrentFailure::new(
            "torrent_catalog_changed",
            "Torrent metadata differs from the catalog selected by the agent",
        ));
    }
    Ok(())
}

async fn inspect_target(
    service: &dyn TorrentService,
    target: InspectionTarget,
    limits: &TorrentServiceLimits,
    inspection_limits: &MediaInspectionLimits,
    selection: &StreamSelection,
    ffprobe: &Path,
    cancellation: &CancellationToken,
) -> FileInspection {
    let started = Instant::now();
    let torrent = &target.torrent;
    let mut inspection = FileInspection {
        info_hash_v1: torrent.info_hash_v1.clone(),
        file_id: target.file_id,
        file: None,
        media: None,
        failure: None,
        elapsed_ms: 0,
        assessment: None,
    };
    let file = match torrent.selected_file(target.file_id) {
        Ok(file) => file.clone(),
        Err(error) => {
            inspection.failure = Some(TorrentFailure::from(error).into());
            return inspection;
        }
    };
    inspection.file = Some(file.clone());
    let mut opened = false;
    // Opening the file and reading it each get their full time: a bound of
    // the torrent limit alone cut off probes allowed to run longer than it.
    let budget = limits.timeout_seconds + inspection_limits.timeout_seconds;
    let result = bounded(budget, cancellation, async {
        let input = service.open_file(torrent, &file).await?;
        opened = true;
        let outcome = inspect_media(
            MediaInspectionRequest {
                input,
                limits: inspection_limits.clone(),
                selection: selection.clone(),
            },
            ffprobe,
            cancellation.clone(),
        )
        .await?;
        Ok(outcome)
    })
    .await;
    // Release even after a timeout or cancellation dropped the read midway.
    // An open that did not complete holds nothing (services keep it atomic).
    if opened {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            service.close_file(torrent, &file),
        )
        .await;
    }
    match result {
        Err(error) => inspection.failure = Some(error.into()),
        Ok(outcome) => {
            inspection.media = outcome.report;
            if let Some(code) = outcome.failure_code {
                let explained = (code == "probe_timeout").then(|| {
                    format!(
                        "ffprobe did not finish within inspection_limits.timeout_seconds ({} s): \
                         peers delivered the file's header too slowly. The file was not \
                         inspected, so nothing about its streams is known. Retry with a larger \
                         timeout (at most 120 s); a long-running `torrentx serve` or `torrentx \
                         mcp` keeps the partial download, so a retry resumes it",
                        inspection_limits.timeout_seconds
                    )
                });
                inspection.failure = Some(InspectionFailure {
                    message: outcome
                        .diagnostic
                        .or(explained)
                        .unwrap_or_else(|| code.clone()),
                    code: static_code(&code).into(),
                });
            }
        }
    }
    inspection.elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    inspection
}

/// A deadline hit before the file list arrived means no peer answered, which
/// a blocked network causes as often as a dead torrent; say so instead of
/// leaving the agent to guess.
fn metadata_timeout(seconds: u64) -> impl FnOnce(TorrentFailure) -> TorrentFailure {
    move |failure| {
        if failure.code != "torrent_service_timeout" {
            return failure;
        }
        TorrentFailure::new(
            "torrent_service_timeout",
            format!(
                "The torrent's metadata (its file list) did not arrive within \
                 limits.timeout_seconds ({seconds} s): no peer supplied it. Either the swarm \
                 has no reachable peers or this network blocks BitTorrent (trackers and DHT \
                 use UDP, which some campus, office and national networks block). Nothing \
                 was inspected, so nothing about the files is known; it does not show the \
                 torrent is dead"
            ),
        )
    }
}

fn static_code(code: &str) -> &'static str {
    match code {
        "cancelled" => "cancelled",
        "probe_timeout" => "probe_timeout",
        "read_budget_exceeded" => "read_budget_exceeded",
        "source_read_failed" => "source_read_failed",
        "probe_diagnostics" => "probe_diagnostics",
        "torrent_service_timeout" => "torrent_service_timeout",
        "torrent_service_failed" => "torrent_service_failed",
        "torrent_service_unavailable" => "torrent_service_unavailable",
        "torrent_catalog_changed" => "torrent_catalog_changed",
        "torrent_identity_mismatch" => "torrent_identity_mismatch",
        "torrent_metadata_invalid" => "torrent_metadata_invalid",
        _ => "probe_failed",
    }
}

fn catalog(
    source_reference: TorrentReference,
    resolved_reference: TorrentReference,
    metadata: TorrentMetadata,
) -> Result<TorrentCatalog, TorrentFailure> {
    if let TorrentReference::Magnet {
        info_hash_v1: Some(expected),
        ..
    } = &resolved_reference
        && expected != &metadata.info_hash_v1
    {
        return Err(TorrentFailure::new(
            "torrent_identity_mismatch",
            "Torrent service returned metadata for another infohash",
        ));
    }
    let catalog = TorrentCatalog {
        schema_version: 1,
        source_reference,
        resolved_reference,
        info_hash_v1: metadata.info_hash_v1,
        name: metadata.name,
        files: metadata.files,
    };
    catalog
        .validate()
        .map_err(|e| TorrentFailure::new("torrent_metadata_invalid", e.to_string()))?;
    Ok(catalog)
}

async fn resolve_reference(
    reference: &TorrentReference,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: &TorrentServiceLimits,
    cancellation: CancellationToken,
) -> Result<TorrentReference, TorrentFailure> {
    validate_reference(reference)?;
    if let TorrentReference::Http {
        uri,
        source_id,
        post_form: None,
    } = reference
    {
        let descriptor = sources
            .iter()
            .find(|s| s.descriptor().id == *source_id)
            .map(|s| s.descriptor());
        if descriptor.is_some_and(|d| d.resolves_http) {
            let engine = SearchEngine::new(
                sources,
                SearchLimits {
                    source_timeout: std::time::Duration::from_secs(limits.timeout_seconds),
                    ..Default::default()
                },
            )?;
            let resolved = engine.resolve(source_id, uri, cancellation).await?;
            validate_reference(&resolved)?;
            return Ok(resolved);
        }
    }
    Ok(reference.clone())
}
