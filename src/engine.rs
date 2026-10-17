use crate::{
    SearchError, SearchLimits, SearchReport, SearchRequest, SearchResult, SearchSource,
    SourceDescriptor, SourceOutcome, SourceReport, SourceStatus, TorrentReference,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::{sync::Semaphore, task::JoinSet};
use tokio_util::sync::CancellationToken;

pub struct SearchEngine {
    sources: Vec<Arc<dyn SearchSource>>,
    limits: SearchLimits,
}

impl SearchEngine {
    pub fn new(
        sources: Vec<Arc<dyn SearchSource>>,
        limits: SearchLimits,
    ) -> Result<Self, SearchError> {
        limits.validate()?;
        let mut identifiers = HashSet::new();
        if sources.is_empty()
            || sources.iter().any(|source| {
                source.descriptor().id.is_empty()
                    || !identifiers.insert(source.descriptor().id.clone())
            })
        {
            return Err(SearchError::InvalidRequest(
                "Sources must have distinct nonempty identifiers".into(),
            ));
        }
        Ok(Self { sources, limits })
    }

    pub fn sources(&self) -> Vec<&SourceDescriptor> {
        self.sources
            .iter()
            .map(|source| source.descriptor())
            .collect()
    }

    pub async fn search(
        &self,
        request: SearchRequest,
        cancellation: CancellationToken,
    ) -> Result<SearchReport, SearchError> {
        request.validate()?;
        for id in &request.sources {
            self.find_source(id)?;
        }
        let semaphore = Arc::new(Semaphore::new(self.limits.parallel_sources));
        let mut tasks = JoinSet::new();
        let mut task_sources = HashMap::new();
        for source in &self.sources {
            let id = source.descriptor().id.clone();
            if !request.sources.is_empty() && !request.sources.contains(&id) {
                continue;
            }
            let source = source.clone();
            let semaphore = semaphore.clone();
            let request = request.clone();
            let limits = self.limits.clone();
            let cancellation = cancellation.clone();
            let task_source_id = id.clone();
            let task = tasks.spawn(async move {
                let permit = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return empty_outcome(&id, SourceStatus::Cancelled, None),
                    permit = semaphore.acquire_owned() => permit.expect("Semaphore is owned by the search"),
                };
                let outcome = if !source.descriptor().categories.contains(&request.category) {
                    empty_outcome(&id, SourceStatus::Unsupported, Some(format!("Category '{}' is not supported", request.category)))
                } else {
                    source.search(&request, &limits, cancellation).await
                };
                drop(permit);
                outcome
            });
            task_sources.insert(task.id(), task_source_id);
        }
        let mut outcomes = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    let id = &task_sources[&error.id()];
                    outcomes.push(empty_outcome(
                        id,
                        SourceStatus::Failed,
                        Some(format!("Source task failed: {error}")),
                    ));
                }
            }
        }
        outcomes.sort_by(|a, b| a.report.source_id.cmp(&b.report.source_id));
        let mut reports = Vec::new();
        let mut collected = Vec::new();
        for outcome in outcomes {
            reports.push(outcome.report);
            collected.extend(outcome.results);
        }
        Ok(SearchReport {
            schema_version: 1,
            query: request.query,
            category: request.category,
            partial: reports
                .iter()
                .any(|report| report.status != SourceStatus::Completed),
            sources: reports,
            results: merge_results(collected),
        })
    }

    pub async fn resolve(
        &self,
        source_id: &str,
        uri: &str,
        cancellation: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        self.find_source(source_id)?
            .resolve(uri, &self.limits, cancellation)
            .await
    }

    fn find_source(&self, id: &str) -> Result<&Arc<dyn SearchSource>, SearchError> {
        self.sources
            .iter()
            .find(|source| source.descriptor().id == id)
            .ok_or_else(|| SearchError::InvalidRequest(format!("Unknown source: {id}")))
    }
}

pub fn empty_outcome(id: &str, status: SourceStatus, error: Option<String>) -> SourceOutcome {
    SourceOutcome {
        report: SourceReport {
            source_id: id.into(),
            status,
            elapsed_ms: 0,
            result_count: 0,
            limited: false,
            error,
        },
        results: Vec::new(),
    }
}

fn merge_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut merged: Vec<SearchResult> = Vec::new();
    let mut positions = HashMap::new();
    for mut result in results {
        let key = match &result.reference {
            TorrentReference::Magnet {
                info_hash_v1: Some(hash),
                ..
            } => format!("v1:{hash}"),
            TorrentReference::Magnet { uri, .. } => format!("magnet:{uri}"),
            TorrentReference::Http {
                uri,
                source_id,
                post_form,
            } => format!(
                "http:{source_id}:{uri}:{}",
                serde_json::to_string(post_form).expect("String map serialization")
            ),
        };
        if let Some(&index) = positions.get(&key) {
            let existing: &mut SearchResult = &mut merged[index];
            existing.observations.append(&mut result.observations);
        } else {
            positions.insert(key, merged.len());
            merged.push(result);
        }
    }
    // This is a factual availability sort, not an assessment of release quality.
    let seeders = |result: &SearchResult| {
        result
            .observations
            .iter()
            .filter_map(|origin| origin.seeders)
            .max()
    };
    merged.sort_by(|a, b| {
        seeders(b)
            .cmp(&seeders(a))
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.reference.uri().cmp(b.reference.uri()))
    });
    merged
}

#[cfg(test)]
mod reference_tests {
    use super::*;
    #[test]
    fn post_download_identities_do_not_merge_and_old_get_json_still_roundtrips() {
        let get =
            serde_json::json!({"kind":"http","uri":"https://example.org/file","source_id":"a"});
        let reference: TorrentReference = serde_json::from_value(get.clone()).unwrap();
        assert_eq!(serde_json::to_value(reference).unwrap(), get);
        let row = |hid: &str| SearchResult {
            title: hid.into(),
            reference: TorrentReference::Http {
                uri: "https://example.org/torfile/".into(),
                source_id: "a".into(),
                post_form: Some(std::collections::BTreeMap::from([(
                    "hid".into(),
                    hid.into(),
                )])),
            },
            observations: Vec::new(),
        };
        assert_eq!(
            merge_results(vec![row("one"), row("two"), row("one")]).len(),
            2
        );
    }
}
