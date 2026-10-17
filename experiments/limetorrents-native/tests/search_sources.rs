//! Controlled fixtures exercise process failures, cancellation and contracts.
//! Live source quality is verified separately; fixtures are not real torrents.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchEngine, SearchLimits, SearchRequest, SourceStatus, sources::qbit::load_sources,
};

struct Fixture {
    directory: TempDir,
    manifest: Value,
}

impl Fixture {
    fn new(plugins: &[(&str, &str)]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("qbit/engines")).unwrap();
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        fs::copy(
            resources.join("qbit_worker.py"),
            root.join("qbit_worker.py"),
        )
        .unwrap();
        let mut common = Vec::new();
        for name in ["helpers.py", "novaprinter.py", "socks.py"] {
            let relative = format!("qbit/{name}");
            fs::copy(resources.join(&relative), root.join(&relative)).unwrap();
            common.push(json!({"path": relative, "sha256": digest(&fs::read(root.join(relative)).unwrap())}));
        }
        let sources: Vec<_> = plugins.iter().map(|(name, body)| {
            let script = format!("from novaprinter import prettyPrinter\nimport time\nclass {name}:\n    supported_categories = {{'all': ''}}\n    def search(self, query, category):\n        assert query == 'controlled%20fixture'\n{body}\n");
            fs::write(root.join(format!("qbit/engines/{name}.py")), &script).unwrap();
            json!({"id": name, "name": name, "site_url": "https://example.org", "categories": ["all"], "resolves_http": false, "sha256": digest(script.as_bytes())})
        }).collect();
        let manifest = json!({"schema_version": 1, "common_files": common, "sources": sources});
        fs::write(
            root.join("sources.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Self {
            directory,
            manifest,
        }
    }

    fn engine(&self, limits: SearchLimits) -> SearchEngine {
        let python = std::env::var_os("TORRENT_SEARCH_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| "python".into());
        SearchEngine::new(
            load_sources(self.directory.path(), &python).unwrap(),
            limits,
        )
        .unwrap()
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn request() -> SearchRequest {
    SearchRequest {
        query: "controlled fixture".into(),
        category: "all".into(),
        sources: vec![],
    }
}
fn result_statement(seed: u64) -> String {
    format!(
        "        prettyPrinter({{'name': 'اختبار | UTF-8', 'link': 'magnet:?xt=urn:btih:0000000000000000000000000000000000000000', 'size': '-1', 'seeds': {seed}, 'leech': -1, 'engine_url': 'https://example.org'}})"
    )
}

#[tokio::test]
async fn merges_known_identities_preserves_observations_and_keeps_partial_results() {
    let good = format!(
        "        print('diagnostic noise')\n{}",
        result_statement(10)
    );
    let bad = format!(
        "{}\n        raise RuntimeError('controlled source failure')",
        result_statement(20)
    );
    let fixture = Fixture::new(&[("healthy", &good), ("failing", &bad)]);
    let report = fixture
        .engine(SearchLimits::default())
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.partial);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].title, "اختبار | UTF-8");
    assert_eq!(report.results[0].observations.len(), 2);
    assert!(
        report.results[0]
            .observations
            .iter()
            .all(|origin| origin.size_bytes.is_none() && origin.leechers.is_none())
    );
    assert_eq!(
        report
            .sources
            .iter()
            .find(|source| source.source_id == "healthy")
            .unwrap()
            .status,
        SourceStatus::Completed
    );
    let failed = report
        .sources
        .iter()
        .find(|source| source.source_id == "failing")
        .unwrap();
    assert_eq!(failed.status, SourceStatus::Failed);
    assert_eq!(failed.result_count, 1);
    assert!(
        failed
            .error
            .as_ref()
            .unwrap()
            .contains("controlled source failure")
    );
}

#[tokio::test]
async fn timeout_preserves_results_and_cleans_up_the_worker() {
    let body = format!("{}\n        time.sleep(30)", result_statement(1));
    let fixture = Fixture::new(&[("hanging", &body)]);
    let limits = SearchLimits {
        source_timeout: Duration::from_secs(2),
        ..SearchLimits::default()
    };
    let report = fixture
        .engine(limits)
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.sources[0].status, SourceStatus::TimedOut);
    assert_eq!(report.results.len(), 1);
    assert!(report.sources[0].elapsed_ms < 5000);
}

#[tokio::test]
async fn cancellation_includes_queued_sources() {
    let body = "        time.sleep(30)";
    let fixture = Fixture::new(&[("first", body), ("queued", body)]);
    let token = CancellationToken::new();
    let signal = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        signal.cancel();
    });
    let limits = SearchLimits {
        parallel_sources: 1,
        ..SearchLimits::default()
    };
    let report = fixture
        .engine(limits)
        .search(request(), token)
        .await
        .unwrap();
    assert_eq!(report.sources.len(), 2);
    assert!(
        report
            .sources
            .iter()
            .all(|source| source.status == SourceStatus::Cancelled)
    );
}

#[tokio::test]
async fn result_caps_and_empty_results_are_successful_and_unknown_sources_are_rejected() {
    let body = format!("        for _ in range(100):\n    {}", result_statement(1));
    let fixture = Fixture::new(&[("many", &body), ("empty", "        pass")]);
    let limits = SearchLimits {
        results_per_source: 3,
        ..SearchLimits::default()
    };
    let engine = fixture.engine(limits);
    let report = engine
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert!(!report.partial);
    let many = report
        .sources
        .iter()
        .find(|source| source.source_id == "many")
        .unwrap();
    assert_eq!(many.result_count, 3);
    assert!(many.limited);
    let mut invalid = request();
    invalid.sources.push("absent".into());
    assert!(
        engine
            .search(invalid, CancellationToken::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn import_failures_and_unsupported_categories_have_explicit_states() {
    let fixture = Fixture::new(&[("broken", "        pass"), ("healthy", "        pass")]);
    // Repin an intentionally broken local fixture; this tests import failure,
    // not integrity rejection.
    let path = fixture.directory.path().join("qbit/engines/broken.py");
    fs::write(&path, "raise ImportError('controlled import failure')").unwrap();
    let mut manifest = fixture.manifest.clone();
    manifest["sources"][0]["sha256"] = json!(digest(&fs::read(path).unwrap()));
    fs::write(
        fixture.directory.path().join("sources.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let engine = fixture.engine(SearchLimits::default());
    let report = engine
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.sources[0].status, SourceStatus::Failed);
    assert_eq!(report.sources[1].status, SourceStatus::Completed);
    let mut tv = request();
    tv.category = "tv".into();
    let report = engine.search(tv, CancellationToken::new()).await.unwrap();
    assert!(
        report
            .sources
            .iter()
            .all(|source| source.status == SourceStatus::Unsupported)
    );
}

#[tokio::test]
async fn changed_plugin_is_rejected_without_breaking_other_sources() {
    let fixture = Fixture::new(&[("changed", "        pass"), ("healthy", "        pass")]);
    fs::write(
        fixture.directory.path().join("qbit/engines/changed.py"),
        "raise Exception('must not run')",
    )
    .unwrap();
    let report = fixture
        .engine(SearchLimits::default())
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.sources[0].status, SourceStatus::Failed);
    assert!(
        report.sources[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Integrity check failed")
    );
    assert_eq!(report.sources[1].status, SourceStatus::Completed);
}

#[tokio::test]
async fn malformed_output_is_isolated_and_large_diagnostics_do_not_block_search() {
    let healthy = format!(
        "        print('diagnostic' * 100000)\n{}",
        result_statement(3)
    );
    let malformed = "        import os\n        os.write(1, b'not-json\\n')";
    let fixture = Fixture::new(&[("healthy", &healthy), ("malformed", malformed)]);
    let report = fixture
        .engine(SearchLimits::default())
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert!(report.partial);
    assert_eq!(report.sources[0].status, SourceStatus::Completed);
    assert_eq!(report.sources[1].status, SourceStatus::Failed);
    assert!(
        report.sources[1]
            .error
            .as_ref()
            .unwrap()
            .contains("Invalid worker protocol")
    );
    assert_eq!(report.results.len(), 1);
}

#[tokio::test]
async fn same_title_is_not_enough_to_merge_different_torrents_and_output_is_bounded() {
    let first = result_statement(1);
    let second = first.replace(
        "0000000000000000000000000000000000000000",
        "1111111111111111111111111111111111111111",
    );
    let fixture = Fixture::new(&[("first", &first), ("second", &second)]);
    let report = fixture
        .engine(SearchLimits::default())
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.results.len(), 2);
    let many = format!("        for _ in range(100):\n    {}", result_statement(1));
    let fixture = Fixture::new(&[("many", &many)]);
    let limits = SearchLimits {
        output_bytes_per_source: 1024,
        ..SearchLimits::default()
    };
    let report = fixture
        .engine(limits)
        .search(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.sources[0].status, SourceStatus::Failed);
    assert!(
        report.sources[0]
            .error
            .as_ref()
            .unwrap()
            .contains("output limit exceeded")
    );
    assert!(!report.results.is_empty());
}
