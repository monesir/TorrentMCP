use super::*;
use crate::{
    SearchEngine, SearchLimits, SearchSource,
    media::{MediaInspectionLimits, MediaInspectionRequest, StreamSelection, inspect_media},
};
use std::{path::Path, sync::Arc};

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
}

impl TorrentOperation {
    fn name(&self) -> &'static str {
        match self {
            Self::Files { .. } => "torrent_files",
            Self::Inspect { .. } => "inspect_torrent_file",
        }
    }
}

pub async fn run(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    runtime: &TorrentRuntime,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (Option<TorrentOperationReport>, Option<TorrentFailure>) {
    let mut service = match RqbitService::start(runtime, &limits, &cancellation).await {
        Ok(service) => service,
        Err(error) => return (None, Some(error)),
    };
    run_with_service(
        operation,
        sources,
        limits,
        &mut service,
        ffprobe,
        cancellation,
    )
    .await
}

/// Also used by other download-service implementations and protocol integration tests.
pub async fn run_with_service(
    operation: TorrentOperation,
    sources: Vec<Arc<dyn SearchSource>>,
    limits: TorrentServiceLimits,
    service: &mut dyn TorrentService,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> (Option<TorrentOperationReport>, Option<TorrentFailure>) {
    let mut report = TorrentOperationReport {
        schema_version: 1,
        operation: operation.name().into(),
        torrent: None,
        file: None,
        media: None,
        service: service.description(),
        assessment: None,
    };
    let work = async {
        match operation {
            TorrentOperation::Files {
                reference,
                resolve_http,
            } => {
                let resolved = if resolve_http {
                    resolve_reference(&reference, sources, &limits, cancellation.clone()).await?
                } else {
                    reference.clone()
                };
                let metadata = service.metadata(&resolved).await?;
                report.torrent = Some(catalog(reference, resolved, metadata)?);
            }
            TorrentOperation::Inspect {
                torrent,
                file_id,
                inspection_limits,
                selection,
            } => {
                torrent.validate()?;
                torrent.selected_file(file_id)?;
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
                let file = actual.selected_file(file_id)?.clone();
                report.torrent = Some(actual);
                report.file = Some(file.clone());
                let input = service
                    .open_file(report.torrent.as_ref().unwrap(), &file)
                    .await?;
                let outcome = inspect_media(
                    MediaInspectionRequest {
                        input,
                        limits: inspection_limits,
                        selection,
                    },
                    ffprobe,
                    cancellation.clone(),
                )
                .await?;
                report.media = outcome.report;
                if let Some(code) = outcome.failure_code {
                    let code = match code.as_str() {
                        "cancelled" => "cancelled",
                        "probe_timeout" => "probe_timeout",
                        "read_budget_exceeded" => "read_budget_exceeded",
                        "source_read_failed" => "source_read_failed",
                        "probe_diagnostics" => "probe_diagnostics",
                        _ => "probe_failed",
                    };
                    return Err(TorrentFailure::new(
                        code,
                        outcome.diagnostic.unwrap_or_else(|| code.into()),
                    ));
                }
            }
        }
        Ok(())
    };
    let mut failure = bounded(limits.timeout_seconds, &cancellation, work)
        .await
        .err();
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

fn catalog(
    source_reference: TorrentReference,
    resolved_reference: TorrentReference,
    metadata: TorrentMetadata,
) -> Result<TorrentCatalog, TorrentFailure> {
    if let TorrentReference::Magnet {
        info_hash_v1: Some(expected),
        ..
    } = &resolved_reference
    {
        if expected != &metadata.info_hash_v1 {
            return Err(TorrentFailure::new(
                "torrent_identity_mismatch",
                "Torrent service returned metadata for another infohash",
            ));
        }
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
