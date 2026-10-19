use crate::{
    SearchEngine, SearchError, SearchLimits, SearchReport, SearchRequest, SearchSource,
    TorrentReference,
    analysis::{AnalysisRequest, AnalyzedReport, Eligibility, analyze_report, validate_report},
    comparison::{
        InspectedMediaCandidate, MediaAnalysisRequest, MediaComparisonReport,
        SavedTorrentInspection,
    },
    media::{MediaInspectionReport, MediaInspectionRequest, inspect_media},
    torrent::{
        FileChoice, TorrentCatalog, TorrentFailure, TorrentOperation, TorrentOperationReport,
        TorrentPool, TorrentRuntime, TorrentServiceLimits, TorrentSource,
    },
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub const MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRequest {
    Search {
        schema_version: u32,
        search: SearchRequest,
        analysis: AnalysisRequest,
        #[serde(default)]
        limits: AgentLimits,
    },
    Analyze {
        schema_version: u32,
        report: SearchReport,
        analysis: AnalysisRequest,
    },
    InspectMedia {
        schema_version: u32,
        media: MediaInspectionRequest,
    },
    TorrentFiles {
        schema_version: u32,
        reference: TorrentReference,
        #[serde(default = "default_resolve_http")]
        resolve_http: bool,
        #[serde(default)]
        limits: TorrentServiceLimits,
    },
    InspectTorrentFile {
        schema_version: u32,
        torrent: TorrentCatalog,
        file_id: usize,
        #[serde(default)]
        limits: TorrentServiceLimits,
        #[serde(default)]
        inspection_limits: crate::media::MediaInspectionLimits,
        #[serde(default)]
        selection: crate::media::StreamSelection,
        #[serde(default)]
        analysis: Option<MediaAnalysisRequest>,
    },
    CompareMedia {
        schema_version: u32,
        analysis: MediaAnalysisRequest,
        candidates: Vec<InspectedMediaCandidate>,
    },
    /// Read a torrent and inspect several of its files in one call and one session.
    InspectTorrent {
        schema_version: u32,
        /// A search result reference, read now. Exclusive with `torrent`.
        #[serde(default)]
        reference: Option<TorrentReference>,
        /// A catalog from an earlier torrent_files call. Exclusive with `reference`.
        #[serde(default)]
        torrent: Option<TorrentCatalog>,
        #[serde(default = "default_resolve_http")]
        resolve_http: bool,
        /// Files to inspect; when empty, `sample_files` videos are chosen.
        #[serde(default)]
        file_ids: Vec<usize>,
        /// 1 picks a typical (median-sized) video; more spread over the episodes (default 1).
        #[serde(default)]
        sample_files: Option<usize>,
        #[serde(default = "default_parallel")]
        parallel: usize,
        #[serde(default)]
        limits: TorrentServiceLimits,
        #[serde(default)]
        inspection_limits: crate::media::MediaInspectionLimits,
        #[serde(default)]
        selection: crate::media::StreamSelection,
        #[serde(default)]
        analysis: Option<MediaAnalysisRequest>,
    },
    /// Search, inspect one representative file of each leading candidate, and
    /// compare their actual streams. "Best" is whatever the agent's own
    /// constraints and preferences say; nothing is assumed beyond them.
    Find {
        schema_version: u32,
        search: SearchRequest,
        /// Criteria read from release names: target, constraints, preferences.
        analysis: AnalysisRequest,
        /// Criteria checked on the inspected streams.
        #[serde(default)]
        media: MediaAnalysisRequest,
        #[serde(default)]
        inspect: FindInspection,
        #[serde(default)]
        limits: AgentLimits,
        #[serde(default)]
        torrent_limits: TorrentServiceLimits,
        #[serde(default)]
        inspection_limits: crate::media::MediaInspectionLimits,
        #[serde(default)]
        selection: crate::media::StreamSelection,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FindInspection {
    /// Leading search results to inspect, 0..8; 0 only searches.
    pub candidates: usize,
    /// Also inspect results whose names leave a constraint undecided.
    pub include_uncertain: bool,
    /// Candidates inspected at once, 1..4.
    pub parallel: usize,
}

impl Default for FindInspection {
    fn default() -> Self {
        Self {
            candidates: 3,
            include_uncertain: false,
            parallel: 2,
        }
    }
}

pub const MAX_FIND_CANDIDATES: usize = 8;

fn default_parallel() -> usize {
    3
}

fn default_resolve_http() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentLimits {
    pub timeout_seconds: u64,
    pub parallel_sources: usize,
    pub results_per_source: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: 20,
            parallel_sources: 4,
            results_per_source: 100,
        }
    }
}

impl AgentLimits {
    fn search_limits(&self) -> SearchLimits {
        SearchLimits {
            source_timeout: Duration::from_secs(self.timeout_seconds),
            parallel_sources: self.parallel_sources,
            results_per_source: self.results_per_source,
            ..SearchLimits::default()
        }
    }
}

impl AgentRequest {
    pub fn requires_sources(&self) -> bool {
        matches!(
            self,
            Self::Search { .. }
                | Self::Find { .. }
                | Self::TorrentFiles {
                    resolve_http: true,
                    reference: TorrentReference::Http {
                        post_form: None,
                        ..
                    },
                    ..
                }
                | Self::InspectTorrent {
                    resolve_http: true,
                    reference: Some(TorrentReference::Http {
                        post_form: None,
                        ..
                    }),
                    ..
                }
        )
    }

    /// Fill what the request implies but omits, before it runs.
    pub fn complete(&mut self) {
        match self {
            Self::TorrentFiles { reference, .. }
            | Self::InspectTorrent {
                reference: Some(reference),
                ..
            } => crate::torrent::complete_reference(reference),
            _ => {}
        }
    }

    pub fn validate(&self) -> Result<(), SearchError> {
        let version = match self {
            Self::Search { schema_version, .. }
            | Self::Analyze { schema_version, .. }
            | Self::InspectMedia { schema_version, .. }
            | Self::TorrentFiles { schema_version, .. }
            | Self::InspectTorrentFile { schema_version, .. }
            | Self::InspectTorrent { schema_version, .. }
            | Self::Find { schema_version, .. }
            | Self::CompareMedia { schema_version, .. } => *schema_version,
        };
        if version != 1 {
            return Err(SearchError::InvalidRequest(format!(
                "Unsupported schema_version: {version}; expected 1"
            )));
        }
        match self {
            Self::CompareMedia {
                analysis,
                candidates,
                ..
            } => {
                analysis.validate()?;
                crate::comparison::validate_candidates(candidates)
            }
            Self::InspectMedia { media, .. } => media.validate(),
            Self::TorrentFiles {
                reference, limits, ..
            } => {
                limits.validate()?;
                crate::torrent::validate_reference(reference)
            }
            Self::InspectTorrentFile {
                torrent,
                file_id,
                limits,
                inspection_limits,
                selection,
                analysis,
                ..
            } => {
                if let Some(analysis) = analysis {
                    analysis.validate()?;
                }
                limits.validate()?;
                torrent.validate()?;
                torrent.selected_file(*file_id)?;
                MediaInspectionRequest {
                    input: crate::media::MediaInput::LocalFile {
                        path: "validation".into(),
                    },
                    limits: inspection_limits.clone(),
                    selection: selection.clone(),
                }
                .validate()
            }
            Self::InspectTorrent {
                reference,
                torrent,
                file_ids,
                sample_files,
                parallel,
                limits,
                inspection_limits,
                selection,
                analysis,
                ..
            } => {
                let invalid = |text: &str| Err(SearchError::InvalidRequest(text.into()));
                match (reference, torrent) {
                    (Some(reference), None) => crate::torrent::validate_reference(reference)?,
                    (None, Some(torrent)) => {
                        torrent.validate()?;
                        for id in file_ids {
                            torrent.selected_file(*id)?;
                        }
                    }
                    _ => return invalid("Give exactly one of reference or torrent"),
                }
                if !file_ids.is_empty() && sample_files.is_some() {
                    return invalid("Give file_ids or sample_files, not both");
                }
                if crate::torrent::has_duplicates(file_ids) {
                    return invalid("file_ids must not repeat a file");
                }
                let max = crate::torrent::MAX_FILES_PER_REQUEST;
                if file_ids.len() > max || sample_files.is_some_and(|n| !(1..=max).contains(&n)) {
                    return invalid("Inspect 1..32 files per request");
                }
                if !(1..=crate::torrent::MAX_PARALLEL_INSPECTIONS).contains(parallel) {
                    return invalid("parallel must be within 1..8");
                }
                if let Some(analysis) = analysis {
                    analysis.validate()?;
                }
                limits.validate()?;
                MediaInspectionRequest {
                    input: crate::media::MediaInput::LocalFile {
                        path: "validation".into(),
                    },
                    limits: inspection_limits.clone(),
                    selection: selection.clone(),
                }
                .validate()
            }
            Self::Search {
                search,
                analysis,
                limits,
                ..
            } => {
                search.validate()?;
                analysis.validate()?;
                if !(1..=120).contains(&limits.timeout_seconds) {
                    return Err(SearchError::InvalidRequest(
                        "timeout_seconds must be within 1..120".into(),
                    ));
                }
                limits.search_limits().validate()
            }
            Self::Analyze {
                report, analysis, ..
            } => {
                validate_report(report)?;
                analysis.validate()
            }
            Self::Find {
                search,
                analysis,
                media,
                inspect,
                limits,
                torrent_limits,
                inspection_limits,
                selection,
                ..
            } => {
                search.validate()?;
                analysis.validate()?;
                media.validate()?;
                if !(1..=120).contains(&limits.timeout_seconds) {
                    return Err(SearchError::InvalidRequest(
                        "timeout_seconds must be within 1..120".into(),
                    ));
                }
                limits.search_limits().validate()?;
                if inspect.candidates > MAX_FIND_CANDIDATES || !(1..=4).contains(&inspect.parallel)
                {
                    return Err(SearchError::InvalidRequest(
                        "inspect.candidates must be within 0..8 and inspect.parallel within 1..4"
                            .into(),
                    ));
                }
                torrent_limits.validate()?;
                MediaInspectionRequest {
                    input: crate::media::MediaInput::LocalFile {
                        path: "validation".into(),
                    },
                    limits: inspection_limits.clone(),
                    selection: selection.clone(),
                }
                .validate()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Completed,
    Partial,
    Failed,
    Cancelled,
    Error,
}

impl AgentStatus {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Completed => 0,
            Self::Partial => 3,
            Self::Cancelled => 130,
            Self::Failed | Self::Error => 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFailure {
    pub code: String,
    pub message: String,
}

/// Built once per request and serialized; boxing the larger reports buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AgentResult {
    Search(AnalyzedReport),
    Media(MediaInspectionReport),
    Torrent(TorrentOperationReport),
    Comparison(MediaComparisonReport),
    Find(FindReport),
}

#[derive(Debug, Serialize)]
pub struct FindReport {
    pub schema_version: u32,
    /// The full analyzed search, for alternatives beyond the inspected few.
    pub search: AnalyzedReport,
    /// Inspected candidates, best first by the media comparison; candidates
    /// the comparison could not place follow in search order.
    pub candidates: Vec<FindCandidate>,
    /// Evidence for each placement: streams, criteria and preference factors.
    pub comparison: Option<MediaComparisonReport>,
}

#[derive(Debug, Serialize)]
pub struct FindCandidate {
    /// The candidate's id in `comparison.results`.
    pub id: String,
    /// Index in `search.results`.
    pub search_index: usize,
    pub title: String,
    pub reference: TorrentReference,
    /// 1 is best by the media preferences; None when not compared or when
    /// the inspection did not read the file's streams.
    pub position: Option<usize>,
    pub eligibility: Option<Eligibility>,
    pub inspection_status: AgentStatus,
    pub inspection_error: Option<AgentFailure>,
    pub file: Option<String>,
    pub elapsed_ms: u64,
    /// 2 when the first inspection timed out and was resumed once.
    pub attempts: u32,
}

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub schema_version: u32,
    /// Wall time of this request; source and media reports contain phase timings.
    pub elapsed_ms: u64,
    pub status: AgentStatus,
    pub result: Option<AgentResult>,
    pub error: Option<AgentFailure>,
}

impl AgentResponse {
    pub fn failure(error: SearchError) -> Self {
        let code = match error {
            SearchError::InvalidRequest(_) | SearchError::Json(_) => "invalid_request",
            SearchError::Runtime(_) => "runtime_error",
            SearchError::Io(_) => "io_error",
        };
        Self {
            schema_version: 1,
            elapsed_ms: 0,
            status: AgentStatus::Error,
            result: None,
            error: Some(AgentFailure {
                code: code.into(),
                message: error.to_string(),
            }),
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.status.exit_code()
    }
}

pub async fn execute(
    request: AgentRequest,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
) -> AgentResponse {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => return AgentResponse::failure(error.into()),
    };
    let probe = executable
        .parent()
        .unwrap_or(Path::new("."))
        .join(if cfg!(windows) {
            "runtime/media/ffprobe.exe"
        } else {
            "runtime/media/ffprobe"
        });
    execute_with_ffprobe(request, sources, cancellation, &probe).await
}

pub async fn execute_with_ffprobe(
    request: AgentRequest,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
    ffprobe: &Path,
) -> AgentResponse {
    execute_with_runtime(
        request,
        sources,
        cancellation,
        ffprobe,
        &TorrentRuntime::bundled(),
    )
    .await
}

pub async fn execute_with_runtime(
    request: AgentRequest,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
    ffprobe: &Path,
    runtime: &TorrentRuntime,
) -> AgentResponse {
    execute_with_pool(request, sources, cancellation, ffprobe, runtime, None).await
}

/// With a pool, torrent operations share its long-lived rqbit session instead
/// of starting and stopping one per request.
pub async fn execute_with_pool(
    request: AgentRequest,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
    ffprobe: &Path,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
) -> AgentResponse {
    let started = std::time::Instant::now();
    let mut response =
        execute_request(request, sources, cancellation, ffprobe, runtime, pool).await;
    response.elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    response
}

async fn execute_request(
    request: AgentRequest,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
    ffprobe: &Path,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
) -> AgentResponse {
    let mut request = request;
    request.complete();
    if let Err(error) = request.validate() {
        return AgentResponse::failure(error);
    }
    let request = match request {
        AgentRequest::CompareMedia {
            analysis,
            candidates,
            ..
        } => {
            if cancellation.is_cancelled() {
                return AgentResponse {
                    schema_version: 1,
                    elapsed_ms: 0,
                    status: AgentStatus::Cancelled,
                    result: None,
                    error: Some(AgentFailure {
                        code: "cancelled".into(),
                        message: "Comparison cancelled".into(),
                    }),
                };
            }
            return match crate::comparison::compare_media(candidates, analysis) {
                Err(error) => AgentResponse::failure(error),
                Ok(report) => AgentResponse {
                    schema_version: 1,
                    elapsed_ms: 0,
                    status: if report.partial {
                        AgentStatus::Partial
                    } else {
                        AgentStatus::Completed
                    },
                    result: Some(AgentResult::Comparison(report)),
                    error: None,
                },
            };
        }
        AgentRequest::Find {
            search,
            analysis,
            media,
            inspect,
            limits,
            torrent_limits,
            inspection_limits,
            selection,
            ..
        } => {
            return execute_find(
                FindPlan {
                    search,
                    analysis,
                    media,
                    inspect,
                    limits,
                    torrent_limits,
                    inspection_limits,
                    selection,
                },
                sources,
                cancellation,
                ffprobe,
                runtime,
                pool,
            )
            .await;
        }
        AgentRequest::TorrentFiles {
            reference,
            resolve_http,
            limits,
            ..
        } => {
            return execute_torrent(
                TorrentOperation::Files {
                    reference,
                    resolve_http,
                },
                sources,
                limits,
                runtime,
                pool,
                ffprobe,
                cancellation,
                None,
            )
            .await;
        }
        AgentRequest::InspectTorrentFile {
            torrent,
            file_id,
            limits,
            inspection_limits,
            selection,
            analysis,
            ..
        } => {
            return execute_torrent(
                TorrentOperation::Inspect {
                    torrent,
                    file_id,
                    inspection_limits,
                    selection,
                },
                sources,
                limits,
                runtime,
                pool,
                ffprobe,
                cancellation,
                analysis,
            )
            .await;
        }
        AgentRequest::InspectTorrent {
            reference,
            torrent,
            resolve_http,
            file_ids,
            sample_files,
            parallel,
            limits,
            inspection_limits,
            selection,
            analysis,
            ..
        } => {
            let source = match (reference, torrent) {
                (Some(reference), _) => TorrentSource::Reference {
                    reference,
                    resolve_http,
                },
                (None, Some(torrent)) => TorrentSource::Catalog(torrent),
                (None, None) => unreachable!("validated"),
            };
            let files = if file_ids.is_empty() {
                FileChoice::Sample(sample_files.unwrap_or(1))
            } else {
                FileChoice::Ids(file_ids)
            };
            return execute_inspect_torrent(
                TorrentOperation::InspectFiles {
                    source,
                    files,
                    inspection_limits,
                    selection,
                    parallel,
                },
                sources,
                limits,
                runtime,
                pool,
                ffprobe,
                cancellation,
                analysis,
            )
            .await;
        }
        other => other,
    };
    if let AgentRequest::InspectMedia { media, .. } = request {
        return match inspect_media(media, ffprobe, cancellation).await {
            Err(error) => AgentResponse::failure(error),
            Ok(outcome) => {
                let status = if outcome.failure_code.as_deref() == Some("cancelled") {
                    AgentStatus::Cancelled
                } else if outcome.report.is_none() {
                    AgentStatus::Failed
                } else if outcome.failure_code.is_some() {
                    AgentStatus::Partial
                } else {
                    AgentStatus::Completed
                };
                AgentResponse {
                    schema_version: 1,
                    elapsed_ms: 0,
                    status,
                    result: outcome.report.map(AgentResult::Media),
                    error: outcome.failure_code.map(|code| AgentFailure {
                        message: outcome.diagnostic.unwrap_or_else(|| code.clone()),
                        code,
                    }),
                }
            }
        };
    }
    let operation = async {
        match request {
            AgentRequest::InspectMedia { .. }
            | AgentRequest::Find { .. }
            | AgentRequest::InspectTorrent { .. }
            | AgentRequest::TorrentFiles { .. }
            | AgentRequest::InspectTorrentFile { .. }
            | AgentRequest::CompareMedia { .. } => {
                unreachable!("Media inspection is handled separately")
            }
            AgentRequest::Analyze {
                report, analysis, ..
            } => analyze_report(report, analysis),
            AgentRequest::Search {
                search,
                analysis,
                limits,
                ..
            } => {
                let engine = SearchEngine::new(sources, limits.search_limits())?;
                let report = engine.search(search, cancellation.clone()).await?;
                analyze_report(report, analysis)
            }
        }
    }
    .await;
    match operation {
        Err(error) => AgentResponse::failure(error),
        Ok(result) => {
            let no_sources_completed = !result.sources.is_empty()
                && !result.sources.iter().any(|source| source.status.answered());
            let status = if cancellation.is_cancelled() {
                AgentStatus::Cancelled
            } else if no_sources_completed {
                AgentStatus::Failed
            } else if result.partial {
                AgentStatus::Partial
            } else {
                AgentStatus::Completed
            };
            let error = if no_sources_completed && status != AgentStatus::Cancelled {
                Some(AgentFailure {
                    code: "no_sources_completed".into(),
                    message: "Inspect per-source reports before retrying".into(),
                })
            } else {
                None
            };
            AgentResponse {
                schema_version: 1,
                elapsed_ms: 0,
                status,
                result: Some(AgentResult::Search(result)),
                error,
            }
        }
    }
}

struct FindPlan {
    search: SearchRequest,
    analysis: AnalysisRequest,
    media: MediaAnalysisRequest,
    inspect: FindInspection,
    limits: AgentLimits,
    torrent_limits: TorrentServiceLimits,
    inspection_limits: crate::media::MediaInspectionLimits,
    selection: crate::media::StreamSelection,
}

async fn execute_find(
    plan: FindPlan,
    sources: Vec<Arc<dyn SearchSource>>,
    cancellation: CancellationToken,
    ffprobe: &Path,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
) -> AgentResponse {
    use futures_util::StreamExt;
    let engine = match SearchEngine::new(sources.clone(), plan.limits.search_limits()) {
        Ok(engine) => engine,
        Err(error) => return AgentResponse::failure(error),
    };
    let analyzed = match engine
        .search(plan.search, cancellation.clone())
        .await
        .and_then(|report| analyze_report(report, plan.analysis))
    {
        Ok(analyzed) => analyzed,
        Err(error) => return AgentResponse::failure(error),
    };
    let no_sources_completed = !analyzed.sources.is_empty()
        && !analyzed
            .sources
            .iter()
            .any(|source| source.status.answered());
    let picks: Vec<usize> = if cancellation.is_cancelled() {
        vec![]
    } else {
        analyzed
            .results
            .iter()
            .enumerate()
            .filter(|(_, result)| {
                result.eligibility == Eligibility::Eligible
                    || (plan.inspect.include_uncertain
                        && result.eligibility == Eligibility::Uncertain)
            })
            .map(|(index, _)| index)
            .take(plan.inspect.candidates)
            .collect()
    };

    // Without a daemon, one session serves every candidate of this request.
    let local = pool
        .is_none()
        .then(|| TorrentPool::new(runtime.clone(), Duration::from_secs(600)));
    let session = pool.or(local.as_ref()).expect("a pool");
    let attempts: Vec<(usize, SavedTorrentInspection, u32)> =
        futures_util::stream::iter(picks.iter().copied())
            .map(|index| {
                let reference = analyzed.results[index].result.reference.clone();
                let inspection_limits = plan.inspection_limits.clone();
                let selection = plan.selection.clone();
                let operation = move || TorrentOperation::InspectFiles {
                    source: TorrentSource::Reference {
                        reference: reference.clone(),
                        resolve_http: true,
                    },
                    files: FileChoice::Sample(1),
                    inspection_limits: inspection_limits.clone(),
                    selection: selection.clone(),
                    parallel: 1,
                };
                let sources = sources.clone();
                let limits = plan.torrent_limits.clone();
                let cancellation = cancellation.clone();
                async move {
                    let started = std::time::Instant::now();
                    let mut attempt = 0;
                    loop {
                        attempt += 1;
                        let (report, error) = run_torrent(
                            operation(),
                            sources.clone(),
                            limits.clone(),
                            runtime,
                            Some(session),
                            ffprobe,
                            cancellation.clone(),
                        )
                        .await;
                        let inspection = single_inspection(report, error, started.elapsed());
                        // A header that arrived too slowly is resumed, not
                        // restarted: the session kept what was downloaded, so
                        // one more try usually finishes it.
                        let slow = inspection
                            .error
                            .as_ref()
                            .is_some_and(|e| e.code == "probe_timeout");
                        if attempt == 1 && slow && !cancellation.is_cancelled() {
                            continue;
                        }
                        return (index, inspection, attempt);
                    }
                }
            })
            .buffered(plan.inspect.parallel)
            .collect()
            .await;
    let tries: std::collections::HashMap<usize, u32> =
        attempts.iter().map(|(i, _, n)| (*i, *n)).collect();
    let inspections: Vec<(usize, SavedTorrentInspection)> = attempts
        .into_iter()
        .map(|(index, inspection, _)| (index, inspection))
        .collect();
    if let Some(local) = &local {
        let _ = local.shutdown().await;
    }

    let id = |index: usize| format!("result-{}", index + 1);
    let comparison = if inspections.is_empty() {
        None
    } else {
        crate::comparison::compare_media(
            inspections
                .iter()
                .map(|(index, inspection)| InspectedMediaCandidate {
                    id: id(*index),
                    inspection: inspection.clone(),
                    video_stream_index: None,
                })
                .collect(),
            plan.media,
        )
        .ok()
    };
    let mut candidates: Vec<FindCandidate> = inspections
        .into_iter()
        .map(|(index, inspection)| {
            let placed = comparison
                .as_ref()
                .and_then(|report| report.results.iter().find(|r| r.candidate.id == id(index)));
            let report = inspection.result.as_ref();
            // Only verified streams earn a place; an uninspected candidate
            // is not ranked, whatever its name claims.
            let verified = report.is_some_and(|r| r.media.is_some());
            FindCandidate {
                id: id(index),
                search_index: index,
                title: analyzed.results[index].result.title.clone(),
                reference: analyzed.results[index].result.reference.clone(),
                position: placed.filter(|_| verified).map(|r| r.position),
                eligibility: placed.map(|r| r.assessment.eligibility),
                inspection_status: inspection.status,
                inspection_error: inspection.error.clone(),
                file: report.and_then(|r| r.file.as_ref()).map(|f| f.path.clone()),
                elapsed_ms: inspection.elapsed_ms,
                attempts: tries.get(&index).copied().unwrap_or(1),
            }
        })
        .collect();
    candidates.sort_by_key(|c| (c.position.unwrap_or(usize::MAX), c.search_index));

    let inspections_short = candidates
        .iter()
        .any(|c| c.inspection_status != AgentStatus::Completed);
    let status = if cancellation.is_cancelled() {
        AgentStatus::Cancelled
    } else if no_sources_completed {
        AgentStatus::Failed
    } else if analyzed.partial
        || inspections_short
        || comparison.as_ref().is_some_and(|c| c.partial)
    {
        AgentStatus::Partial
    } else {
        AgentStatus::Completed
    };
    let error = (no_sources_completed && status != AgentStatus::Cancelled).then(|| AgentFailure {
        code: "no_sources_completed".into(),
        message: "Inspect per-source reports before retrying".into(),
    });
    AgentResponse {
        schema_version: 1,
        elapsed_ms: 0,
        status,
        result: Some(AgentResult::Find(FindReport {
            schema_version: 1,
            search: analyzed,
            candidates,
            comparison,
        })),
        error,
    }
}

/// A one-file inspect_torrent result in the saved single-file shape that
/// compare_media accepts.
fn single_inspection(
    report: Option<TorrentOperationReport>,
    error: Option<TorrentFailure>,
    elapsed: Duration,
) -> SavedTorrentInspection {
    let mut error = error.map(|e| AgentFailure {
        code: e.code.into(),
        message: e.message,
    });
    let result = report.map(|report| {
        let first = report.inspections.into_iter().next();
        // The file's own failure (probe_timeout and why) says more than the
        // request-level "no file could be inspected".
        if let Some(failure) = first.as_ref().and_then(|i| i.failure.as_ref()) {
            error = Some(AgentFailure {
                code: failure.code.clone(),
                message: failure.message.clone(),
            });
        }
        TorrentOperationReport {
            schema_version: 1,
            operation: "inspect_torrent_file".into(),
            torrent: report.torrent,
            file: first.as_ref().and_then(|i| i.file.clone()),
            media: first.and_then(|i| i.media),
            inspections: vec![],
            service: report.service,
            assessment: None,
        }
    });
    let has_media = result.as_ref().is_some_and(|r| r.media.is_some());
    let status = match &error {
        Some(e) if e.code == "cancelled" => AgentStatus::Cancelled,
        None if has_media => AgentStatus::Completed,
        Some(_) if has_media => AgentStatus::Partial,
        _ => AgentStatus::Failed,
    };
    SavedTorrentInspection {
        schema_version: 1,
        elapsed_ms: elapsed.as_millis().try_into().unwrap_or(u64::MAX),
        status,
        result,
        error,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_torrent(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (Option<TorrentOperationReport>, Option<TorrentFailure>) {
    let Some(pool) = pool else {
        return crate::torrent::run(operation, sources, limits, runtime, ffprobe, cancellation)
            .await;
    };
    let service = match pool.acquire(&cancellation).await {
        Ok(service) => service,
        Err(error) => return (None, Some(error)),
    };
    let (report, failure) = crate::torrent::perform(
        operation,
        sources,
        limits,
        service.as_ref(),
        ffprobe,
        cancellation,
    )
    .await;
    drop(service);
    pool.touch().await;
    (Some(report), failure)
}

#[allow(clippy::too_many_arguments)]
async fn execute_torrent(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
    ffprobe: &Path,
    cancellation: CancellationToken,
    analysis: Option<MediaAnalysisRequest>,
) -> AgentResponse {
    let (mut report, error) = run_torrent(
        operation,
        sources,
        limits,
        runtime,
        pool,
        ffprobe,
        cancellation,
    )
    .await;
    let status = match &error {
        None => AgentStatus::Completed,
        Some(e) if e.code == "cancelled" => AgentStatus::Cancelled,
        Some(_) if report.as_ref().is_some_and(|r| r.media.is_some()) => AgentStatus::Partial,
        Some(_) => AgentStatus::Failed,
    };
    let error = error.map(|e| AgentFailure {
        code: e.code.into(),
        message: e.message,
    });
    if let (Some(report), Some(analysis)) = (report.as_mut(), analysis.as_ref()) {
        report.assessment = Some(crate::comparison::assess_inspection(
            Some(report),
            status,
            error.as_ref(),
            analysis,
            None,
        ));
    }
    AgentResponse {
        schema_version: 1,
        elapsed_ms: 0,
        status,
        result: report.map(AgentResult::Torrent),
        error,
    }
}

/// Each inspected file is assessed as if it were its own single-file inspection,
/// so compare_media and assess_inspection keep one meaning of "complete".
#[allow(clippy::too_many_arguments)]
async fn execute_inspect_torrent(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    runtime: &TorrentRuntime,
    pool: Option<&TorrentPool>,
    ffprobe: &Path,
    cancellation: CancellationToken,
    analysis: Option<MediaAnalysisRequest>,
) -> AgentResponse {
    let (mut report, error) = run_torrent(
        operation,
        sources,
        limits,
        runtime,
        pool,
        ffprobe,
        cancellation,
    )
    .await;
    let inspections = report.as_ref().map_or(&[][..], |r| &r.inspections[..]);
    let inspected = inspections.iter().filter(|i| i.media.is_some()).count();
    let status = match &error {
        Some(e) if e.code == "cancelled" => AgentStatus::Cancelled,
        Some(_) if inspected > 0 => AgentStatus::Partial,
        Some(_) => AgentStatus::Failed,
        None if inspections.iter().any(|i| i.failure.is_some()) => AgentStatus::Partial,
        None => AgentStatus::Completed,
    };
    if let (Some(report), Some(analysis)) = (report.as_mut(), analysis.as_ref()) {
        let service = report.service.clone();
        let torrent = report.torrent.clone();
        for inspection in &mut report.inspections {
            let single = TorrentOperationReport {
                schema_version: 1,
                operation: "inspect_torrent_file".into(),
                torrent: torrent.clone(),
                file: inspection.file.clone(),
                media: inspection.media.clone(),
                inspections: vec![],
                service: service.clone(),
                assessment: None,
            };
            let failure = inspection.failure.as_ref().map(|f| AgentFailure {
                code: f.code.clone(),
                message: f.message.clone(),
            });
            let file_status = if failure.is_none() && error.is_none() {
                AgentStatus::Completed
            } else if inspection.media.is_some() {
                AgentStatus::Partial
            } else {
                AgentStatus::Failed
            };
            inspection.assessment = Some(crate::comparison::assess_inspection(
                Some(&single),
                file_status,
                failure.as_ref(),
                analysis,
                None,
            ));
        }
    }
    AgentResponse {
        schema_version: 1,
        elapsed_ms: 0,
        status,
        result: report.map(AgentResult::Torrent),
        error: error.map(|e| AgentFailure {
            code: e.code.into(),
            message: e.message,
        }),
    }
}
