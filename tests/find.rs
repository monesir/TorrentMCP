use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SearchSource, SourceDescriptor,
    SourceObservation, SourceOutcome, SourceReport, SourceStatus, TorrentReference,
    agent::{AgentRequest, execute_with_runtime},
    torrent::TorrentRuntime,
};

struct Source(SourceDescriptor);

fn result(title: &str, hash: char, seeders: u64) -> SearchResult {
    let hash = hash.to_string().repeat(40);
    SearchResult {
        title: title.into(),
        reference: TorrentReference::Magnet {
            uri: format!("magnet:?xt=urn:btih:{hash}"),
            info_hash_v1: Some(hash),
        },
        observations: vec![SourceObservation {
            source_id: "fixture".into(),
            title: title.into(),
            raw_title: None,
            site_url: "https://example.org".into(),
            description_url: None,
            size_bytes: Some(4 << 30),
            seeders: Some(seeders),
            leechers: None,
            published_unix: None,
            observed_at_unix: 1,
        }],
    }
}

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
        let results = vec![
            result("Shogun S01 1080p WEB-DL", 'a', 900),
            result("Shogun S01 1080p BluRay REMUX", 'b', 20),
            result("Shogun S01 720p HDTV", 'c', 500),
        ];
        SourceOutcome {
            report: SourceReport {
                source_id: "fixture".into(),
                status: SourceStatus::Completed,
                elapsed_ms: 1,
                result_count: results.len(),
                limited: false,
                error: None,
                queried_category: None,
                failure_kind: None,
            },
            results,
        }
    }
    async fn resolve(
        &self,
        _: &str,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        unreachable!()
    }
}

fn sources() -> Vec<Arc<dyn SearchSource>> {
    vec![Arc::new(Source(SourceDescriptor {
        id: "fixture".into(),
        name: "fixture".into(),
        site_url: "https://example.org".into(),
        categories: vec!["all".into()],
        resolves_http: false,
        scope: Default::default(),
        language: None,
        alternate_of: None,
        retired: None,
    }))]
}

fn request(inspect: Value) -> AgentRequest {
    serde_json::from_value(json!({
        "schema_version": 1,
        "operation": "find",
        "search": {"query": "shogun", "category": "all", "sources": []},
        "analysis": {
            "target": {"title": "Shogun", "season": 1},
            "constraints": {"resolutions": [1080]},
            "preferences": {"formats": ["remux", "webdl"]}
        },
        "media": {"preferences": {"video_codecs": ["hevc"]}},
        "inspect": inspect
    }))
    .unwrap()
}

async fn run(request: AgentRequest) -> Value {
    let cache = tempfile::tempdir().unwrap();
    let runtime = TorrentRuntime {
        rqbit: "missing-rqbit".into(),
        cache_directory: cache.path().into(),
        download_limit_bytes_per_second: None,
    };
    request.validate().unwrap();
    let response = execute_with_runtime(
        request,
        sources(),
        CancellationToken::new(),
        "missing-ffprobe".as_ref(),
        &runtime,
    )
    .await;
    serde_json::to_value(&response).unwrap()
}

#[tokio::test]
async fn find_inspects_the_leading_eligible_results_in_the_agents_order() {
    let response = run(request(json!({}))).await;
    let result = &response["result"];
    assert_eq!(result["search"]["summary"]["eligible"], 2);
    let candidates = result["candidates"].as_array().unwrap();
    let titles: Vec<_> = candidates
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Shogun S01 1080p BluRay REMUX", "Shogun S01 1080p WEB-DL"],
        "only eligible results, ranked by the agent's format preference"
    );
    // No torrent runtime here: each inspection fails and says why, and the
    // comparison keeps the candidates as undecided rather than ranking them.
    for candidate in candidates {
        assert_eq!(candidate["inspection_status"], "failed");
        assert_eq!(
            candidate["inspection_error"]["code"],
            "torrent_service_unavailable"
        );
        assert_eq!(candidate["eligibility"], "uncertain");
        assert!(candidate["position"].is_null(), "unverified, so unranked");
    }
    assert_eq!(result["comparison"]["summary"]["uncertain"], 2);
    assert_eq!(response["status"], "partial");
    let markdown = torrent_search::markdown::render(&response).unwrap();
    assert!(markdown.contains("Shogun S01 1080p BluRay REMUX"));
    assert!(markdown.contains("torrent&#95;service&#95;unavailable"));
}

#[tokio::test]
async fn find_with_no_candidates_only_searches() {
    let response = run(request(json!({"candidates": 0}))).await;
    assert_eq!(response["status"], "completed");
    assert_eq!(response["result"]["candidates"], json!([]));
    assert!(response["result"]["comparison"].is_null());
    assert_eq!(response["result"]["search"]["summary"]["total"], 3);
}

#[test]
fn find_bounds_its_inspection_plan() {
    for inspect in [
        json!({"candidates": 9}),
        json!({"parallel": 0}),
        json!({"parallel": 5}),
    ] {
        assert!(request(inspect).validate().is_err());
    }
}
