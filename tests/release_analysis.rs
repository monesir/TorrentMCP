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

fn seeded(title: &str, seeders: u64) -> SearchResult {
    let mut result = release(title);
    result.observations[0].seeders = Some(seeders);
    result
}

fn target(title: &str) -> AnalysisRequest {
    AnalysisRequest {
        target: SearchTarget {
            title: title.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn theatre_captures_are_rejected_by_default_and_allowed_on_request() {
    let titles = [
        "The Batman (2022) 1080p HDTS x264",
        "The Batman 2022 HDCAM x264 AAC",
        "Oppenheimer (2023) NEW ENG 1080p HQ-CAM x264",
        "The Batman 2022 1080p WEBRip x264",
    ];
    let releases = || titles.iter().map(|title| release(title)).collect();
    let mut req = target("The Batman");
    req.target.aliases = vec!["Oppenheimer".into()];
    let analyzed = analyze_report(report(releases()), req.clone()).unwrap();
    assert_eq!(analyzed.summary.excluded, 3);
    assert_eq!(analyzed.results[0].result.title, titles[3]);
    req.constraints.reject_formats = vec![];
    let analyzed = analyze_report(report(releases()), req).unwrap();
    assert_eq!(analyzed.summary.excluded, 0);
}

#[test]
fn multi_season_packs_contain_the_requested_season_and_its_episodes() {
    let mut req = target("Breaking Bad");
    req.target.season = Some(3);
    req.target.episodes = Some(EpisodeRange { start: 4, end: 4 });
    let analyzed = analyze_report(
        report(vec![
            release("Breaking Bad S01-S05 Complete 1080p BluRay x265"),
            release("Breaking Bad - Complete Season 1,2,3,4,5 + Extras - BluRay 1080p"),
            release("Breaking Bad Seasons 1 to 5 1080p"),
            release("Breaking Bad S03 1080p BluRay"),
            release("Breaking Bad S01 S02 1080p"),
            release("Breaking Bad S03E04 1080p"),
        ]),
        req,
    )
    .unwrap();
    for item in &analyzed.results {
        let expected = if item.result.title.contains("S01 S02") {
            Eligibility::Excluded
        } else {
            Eligibility::Eligible
        };
        assert_eq!(item.eligibility, expected, "{}", item.result.title);
    }
    let pack = analyze_release(&release("Breaking Bad S01-S05 1080p"), &target("Breaking Bad"));
    assert_eq!(pack.coverage.claims[0].value, CoverageKind::MultiSeasonPack);
    assert_eq!(pack.season.claims.len(), 5);
}

#[test]
fn a_missing_year_is_not_uncertainty_but_a_different_year_excludes() {
    let mut req = target("Shogun");
    req.target.year = Some(2024);
    let analyzed = analyze_report(
        report(vec![
            release("Shogun S01 1080p WEB-DL"),
            release("Shogun 1980 S01 1080p BluRay"),
        ]),
        req,
    )
    .unwrap();
    assert_eq!(analyzed.results[0].eligibility, Eligibility::Eligible);
    assert_eq!(analyzed.results[1].eligibility, Eligibility::Excluded);
}

#[test]
fn titles_match_through_site_prefixes_and_diacritics() {
    let mut req = target("Shogun");
    req.target.season = Some(1);
    let analyzed = analyze_report(
        report(vec![
            release("Shōgun 2024 S01 1080p WEBRip"),
            release("www.UIndex.org    -    Shogun S01 1080p WEB-DL"),
            release("[ www.TorrentDay.com ] - Shogun S01 1080p"),
        ]),
        req,
    )
    .unwrap();
    assert_eq!(analyzed.summary.eligible, 3);
}

#[test]
fn audio_tags_list_several_tracks_and_any_requested_one_matches() {
    let mut req = request();
    req.constraints.audio_languages = vec!["en".into()];
    let analyzed = analyze_report(
        report(vec![
            release("Nirvana in Fire S01 ITA ENG 1080p"),
            release("Nirvana in Fire S01 Chinese Audio 1080p"),
            release("Nirvana in Fire S01 ITA 1080p"),
        ]),
        req,
    )
    .unwrap();
    let by_title = |needle: &str| {
        analyzed
            .results
            .iter()
            .find(|item| item.result.title.contains(needle))
            .unwrap()
            .eligibility
    };
    assert_eq!(by_title("ITA ENG"), Eligibility::Eligible);
    assert_eq!(by_title("Chinese Audio"), Eligibility::Excluded);
    // A bare tag does not prove the other tracks are absent.
    assert_eq!(by_title("S01 ITA 1080p"), Eligibility::Uncertain);
}

#[test]
fn best_named_quality_ranks_first_among_healthy_releases() {
    let analyzed = analyze_report(
        report(vec![
            seeded("Dune Part Two 2024 2160p WEB-DL DDP5 1 Atmos DV HDR H 265-FLUX", 2544),
            seeded(
                "Dune Part Two 2024 UHD BluRay 2160p TrueHD Atmos 7 1 DV HEVC REMUX-FraMeSToR",
                380,
            ),
            seeded("Dune Part Two 2024 REMUX 2160p TrueHD 7 1 DV", 2),
            seeded("Dune Part Two 2024 REPACK 2160p UPSCALE WEB HEVC 10Bit AAC 2 0", 900),
            seeded("Dune Part Two 2024 2160p BluRay REMUX", 0),
        ]),
        target("Dune Part Two"),
    )
    .unwrap();
    let order: Vec<_> = analyzed
        .results
        .iter()
        .map(|item| item.ranking.highest_reported_seeders.unwrap())
        .collect();
    // Healthy by quality (remux, WEB-DL, upscale), then weak, then dead.
    assert_eq!(order, [380, 2544, 900, 2, 0]);
    let top = &analyzed.results[0];
    assert_eq!(top.analysis.quality.audio_codecs, ["truehd"]);
    assert!(top.analysis.quality.atmos);
    assert_eq!(
        top.analysis.quality.release_group.as_deref(),
        Some("FraMeSToR")
    );
}
