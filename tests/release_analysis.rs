//! Parser mechanics use controlled names; the saved Nirvana report is a real search.
use torrent_search::{
    SearchReport, SearchResult, SourceObservation, TorrentReference,
    analysis::{
        AnalysisRequest, CoverageKind, Eligibility, EpisodeRange, FactState, SearchTarget,
        analyze_release, analyze_report,
    },
};

fn request() -> AnalysisRequest {
    AnalysisRequest {
        target: SearchTarget {
            title: "Nirvana in Fire".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn release(title: &str) -> SearchResult {
    SearchResult {
        title: title.into(),
        reference: TorrentReference::Http {
            post_form: None,
            uri: "https://example.org/unchanged-raw-reference".into(),
            source_id: "fixture".into(),
        },
        observations: vec![SourceObservation {
            source_id: "fixture".into(),
            title: title.into(),
            raw_title: Some(title.into()),
            site_url: "https://example.org".into(),
            description_url: None,
            size_bytes: None,
            seeders: Some(2),
            leechers: None,
            published_unix: None,
            observed_at_unix: 0,
        }],
    }
}

fn report(results: Vec<SearchResult>) -> SearchReport {
    SearchReport {
        schema_version: 1,
        query: "fixture".into(),
        category: "all".into(),
        partial: false,
        sources: vec![],
        results,
    }
}

#[test]
fn extracts_explicit_metadata_with_evidence_and_preserves_the_original() {
    let title =
        "[Group] Nirvana.in.Fire.S01E01-E03.2015.1080p.WEB-DL.x264.Chinese.Audio.English.Subs";
    let result = release(title);
    let parsed = analyze_release(&result, &request());
    assert_eq!(parsed.year.claims[0].value, 2015);
    assert_eq!(parsed.season.claims[0].value, 1);
    assert_eq!(
        parsed.episodes.claims[0].value,
        vec![EpisodeRange { start: 1, end: 3 }]
    );
    assert_eq!(parsed.resolution.claims[0].value, 1080);
    assert_eq!(parsed.video_codec.claims[0].value, "h264");
    assert_eq!(parsed.audio_language.claims[0].value, "zh");
    assert_eq!(parsed.subtitle_language.claims[0].value, "en");
    assert_eq!(parsed.source_format.claims[0].value, "webdl");
    assert_eq!(parsed.titles[0].raw, title);
    assert_eq!(parsed.year.claims[0].evidence[0].source_id, "fixture");
    assert_eq!(parsed.year.claims[0].evidence[0].token, "2015");
    assert_eq!(
        result.reference.uri(),
        "https://example.org/unchanged-raw-reference"
    );
}

#[test]
fn disjoint_episode_labels_do_not_invent_the_missing_episode() {
    let mut req = request();
    req.target.episodes = Some(EpisodeRange { start: 1, end: 3 });
    let analyzed = analyze_report(report(vec![release("Nirvana in Fire S01E01E03")]), req).unwrap();
    assert_eq!(analyzed.results[0].eligibility, Eligibility::Excluded);
    assert_eq!(
        analyzed.results[0].analysis.episodes.claims[0].value,
        vec![
            EpisodeRange { start: 1, end: 1 },
            EpisodeRange { start: 3, end: 3 }
        ]
    );
}

#[test]
fn equivalent_episode_ranges_from_multiple_sources_are_one_claim() {
    let mut result = release("Nirvana in Fire S01E01-E03");
    let mut second = result.observations[0].clone();
    second.source_id = "second".into();
    second.raw_title = Some("Nirvana in Fire EP01-03".into());
    result.observations.push(second);
    let parsed = analyze_release(&result, &request());
    assert_eq!(parsed.episodes.state, FactState::Claimed);
    assert_eq!(parsed.episodes.claims.len(), 1);
    assert_eq!(parsed.episodes.claims[0].evidence.len(), 3);
}

#[test]
fn bare_titles_complete_labels_and_subtitles_do_not_prove_missing_facts() {
    let mut req = request();
    req.target.episodes = Some(EpisodeRange { start: 1, end: 54 });
    req.constraints.audio_languages = vec!["en".into()];
    let analyzed = analyze_report(
        report(vec![
            release("Nirvana in Fire Complete English Subs 1080p"),
            release("Nirvana in Fire 1080p"),
        ]),
        req,
    )
    .unwrap();
    assert_eq!(analyzed.summary.uncertain, 2);
    for item in analyzed.results {
        assert_eq!(item.analysis.audio_language.state, FactState::Unknown);
        assert_eq!(item.analysis.episodes.state, FactState::Unknown);
    }
    let parsed = analyze_release(&release("Nirvana in Fire Complete"), &request());
    assert_eq!(parsed.coverage.claims[0].value, CoverageKind::CompleteClaim);
}

#[test]
fn numeric_title_aliases_and_sequel_suffix_are_not_confused() {
    let mut req = request();
    req.target.title = "1923".into();
    req.target.aliases = vec!["琅琊榜".into()];
    let parsed = analyze_release(&release("1923 S01E02 1080p"), &req);
    assert_eq!(parsed.year.state, FactState::Unknown);
    assert_eq!(parsed.episodes.claims[0].value[0].start, 2);
    let analyzed = analyze_report(report(vec![release("琅琊榜 2015 1080p")]), req).unwrap();
    assert_eq!(analyzed.results[0].eligibility, Eligibility::Eligible);
    let parsed = analyze_release(
        &release("Nirvana in Fire â…¡ EP27-28 2017 4k&1080p"),
        &request(),
    );
    assert_eq!(parsed.part.state, FactState::Inferred);
    assert_eq!(parsed.part.claims[0].value, 2);
    assert_eq!(parsed.resolution.state, FactState::Ambiguous);
    assert_eq!(parsed.season.state, FactState::Unknown);
    assert!(parsed.titles[0].normalized.contains("Fire II"));
    assert!(!parsed.titles[0].corrections.is_empty());
}

#[test]
fn conflicting_claims_are_uncertain_but_explicit_mismatch_is_excluded() {
    let mut req = request();
    req.target.year = Some(2015);
    req.constraints.resolutions = vec![1080];
    let analyzed = analyze_report(
        report(vec![
            release("Nirvana in Fire 2015 2017 1080p"),
            release("Nirvana in Fire 2017 1080p"),
            release("Nirvana in Fire 2015 4k&1080p"),
            release("Nirvana in Fire 2015 1080p"),
            release("Unrelated 2015 1080p"),
        ]),
        req,
    )
    .unwrap();
    assert_eq!(analyzed.summary.eligible, 1);
    assert_eq!(analyzed.summary.uncertain, 2);
    assert_eq!(analyzed.summary.excluded, 2);
}

#[test]
fn preferences_rank_without_excluding_and_unknown_size_is_uncertain() {
    let mut req = request();
    req.preferences.resolutions = vec![1080, 720];
    req.preferences.audio_languages = vec!["zh".into()];
    let analyzed = analyze_report(
        report(vec![
            release("Nirvana in Fire 720p"),
            release("Nirvana in Fire 1080p Chinese Audio"),
        ]),
        req.clone(),
    )
    .unwrap();
    assert_eq!(analyzed.summary.eligible, 2);
    assert_eq!(analyzed.results[0].ranking.preference_score, 140);
    assert_eq!(analyzed.results[1].ranking.preference_score, 50);
    req.constraints.max_size_bytes = Some(1000);
    let analyzed = analyze_report(report(vec![release("Nirvana in Fire 1080p")]), req).unwrap();
    assert_eq!(analyzed.results[0].eligibility, Eligibility::Uncertain);
}

#[test]
fn real_nirvana_report_separates_original_and_sequel_and_keeps_all_references() {
    let report: SearchReport = serde_json::from_str(
        include_str!("../verification/nirvana-in-fire-search.json").trim_start_matches('\u{feff}'),
    )
    .unwrap();
    assert_eq!(report.results.len(), 31);
    let originals: Vec<_> = report
        .results
        .iter()
        .map(|item| (item.title.clone(), item.reference.uri().to_owned()))
        .collect();
    let mut req = request();
    req.target.year = Some(2015);
    req.constraints.resolutions = vec![1080];
    req.preferences.audio_languages = vec!["zh".into()];
    let analyzed = analyze_report(report, req).unwrap();
    assert_eq!(analyzed.summary.total, 31);
    let top = &analyzed.results[0];
    assert!(top.result.title.contains("Chinese Audio"));
    assert_eq!(top.eligibility, Eligibility::Eligible);
    for item in &analyzed.results {
        assert!(originals.contains(&(
            item.result.title.clone(),
            item.result.reference.uri().to_owned()
        )));
        if item.result.title.contains("2017") {
            assert_eq!(
                item.eligibility,
                Eligibility::Excluded,
                "{}",
                item.result.title
            );
        }
        if item.result.title.contains("â…¡") {
            assert_eq!(item.analysis.part.claims[0].value, 2);
        }
    }
    assert!(analyzed.summary.excluded > 0);
    assert!(analyzed.summary.eligible > 0);
}
