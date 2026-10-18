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
            let descriptor = source.descriptor();
            let id = descriptor.id.clone();
            let selected = if request.sources.is_empty() {
                descriptor.selected_by_default(&request.category)
            } else {
                request.sources.contains(&id)
            };
            if !selected {
                continue;
            }
            // A site without the requested category is searched with "all"
            // rather than skipped; release analysis filters the results.
            let query_category = descriptor.query_category(&request.category).to_owned();
            let fallback = (query_category != request.category).then(|| query_category.clone());
            let source = source.clone();
            let semaphore = semaphore.clone();
            let request = SearchRequest {
                category: query_category,
                ..request.clone()
            };
            let limits = self.limits.clone();
            let cancellation = cancellation.clone();
            let task_source_id = id.clone();
            let task = tasks.spawn(async move {
                let permit = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return empty_outcome(&id, SourceStatus::Cancelled, None),
                    permit = semaphore.acquire_owned() => permit.expect("Semaphore is owned by the search"),
                };
                let mut outcome = source.search(&request, &limits, cancellation).await;
                outcome.report.queried_category = fallback;
                // Results gathered before a failure are kept and reported as such.
                if matches!(outcome.report.status, SourceStatus::Failed | SourceStatus::TimedOut)
                    && !outcome.results.is_empty()
                {
                    outcome.report.status = SourceStatus::Partial;
                }
                drop(permit);
                outcome
            });
            task_sources.insert(task.id(), task_source_id);
        }
        if tasks.is_empty() {
            return Err(SearchError::InvalidRequest(format!(
                "No default source covers category '{}'; name sources explicitly",
                request.category
            )));
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
            queried_category: None,
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
            } => match crate::reference::info_hash_in_url(uri) {
                Some(hash) if post_form.is_none() => format!("v1:{hash}"),
                _ => format!(
                    "http:{source_id}:{uri}:{}",
                    serde_json::to_string(post_form).expect("String map serialization")
                ),
            },
        };
        if let Some(&index) = positions.get(&key) {
            let existing: &mut SearchResult = &mut merged[index];
            // Prefer a magnet: it needs no further fetch from the source site.
            if matches!(existing.reference, TorrentReference::Http { .. })
                && matches!(result.reference, TorrentReference::Magnet { .. })
            {
                existing.reference = result.reference;
            }
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

    #[test]
    fn hash_named_torrent_file_merges_with_its_magnet_and_keeps_the_magnet() {
        let hash = "0ed31ccd8167f56a39d6471eb0140439d1b32b5d";
        let result = |reference: TorrentReference| SearchResult {
            title: "T".into(),
            reference,
            observations: Vec::new(),
        };
        let file = result(TorrentReference::Http {
            uri: format!("https://itorrents.net/torrent/{hash}.torrent"),
            source_id: "a".into(),
            post_form: None,
        });
        let magnet = result(
            crate::reference::parse_reference(
                &crate::reference::magnet_from_hash(hash, "T"),
                "b",
            )
            .unwrap(),
        );
        let merged = merge_results(vec![file, magnet]);
        assert_eq!(merged.len(), 1);
        assert!(matches!(merged[0].reference, TorrentReference::Magnet { .. }));
    }
}
