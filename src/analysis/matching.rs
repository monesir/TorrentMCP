use super::{title::title_prefix_end, *};
use serde::Serialize;
use serde_json::{Value, json};

fn state_from_matches(matches: &[bool]) -> CriterionState {
    if matches.is_empty() {
        CriterionState::Unknown
    } else if matches.iter().all(|matched| *matched) {
        CriterionState::Match
    } else if matches.iter().all(|matched| !matched) {
        CriterionState::Mismatch
    } else {
        CriterionState::Unknown
    }
}

fn evaluation(
    field: &str,
    state: CriterionState,
    expected: Value,
    observed: Value,
) -> CriterionEvaluation {
    CriterionEvaluation {
        field: field.into(),
        state,
        reason_code: format!(
            "{field}_{}",
            match state {
                CriterionState::Match => "matches_claims",
                CriterionState::Mismatch => "contradicts_claims",
                CriterionState::Unknown => "missing_or_ambiguous",
            }
        ),
        expected,
        observed,
    }
}

fn require_fact<T: Serialize + PartialEq>(
    field: &str,
    fact: &Fact<T>,
    allowed: &[T],
) -> CriterionEvaluation {
    let matches: Vec<_> = fact
        .claims
        .iter()
        .map(|claim| allowed.contains(&claim.value))
        .collect();
    evaluation(
        field,
        state_from_matches(&matches),
        json!(allowed),
        json!(fact),
    )
}

fn covers(ranges: &[EpisodeRange], expected: EpisodeRange) -> bool {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|range| range.start);
    let mut cursor = u32::from(expected.start);
    for range in sorted {
        if u32::from(range.end) < cursor {
            continue;
        }
        if u32::from(range.start) > cursor {
            return false;
        }
        cursor = u32::from(range.end) + 1;
        if cursor > u32::from(expected.end) {
            return true;
        }
    }
    false
}

fn evaluate(
    result: &SearchResult,
    analysis: &ReleaseAnalysis,
    request: &AnalysisRequest,
) -> Vec<CriterionEvaluation> {
    let title_matches: Vec<_> = analysis
        .titles
        .iter()
        .map(|title| title_prefix_end(&title.normalized, &request.target).is_some())
        .collect();
    let mut title_state = state_from_matches(&title_matches);
    if title_state == CriterionState::Mismatch
        && analysis
            .titles
            .iter()
            .any(|title| title.normalized.is_empty() || !title.issues.is_empty())
    {
        title_state = CriterionState::Unknown;
    }
    let mut criteria = vec![evaluation(
        "title",
        title_state,
        json!({"title": request.target.title, "aliases": request.target.aliases}),
        json!(
            analysis
                .titles
                .iter()
                .map(|title| &title.normalized)
                .collect::<Vec<_>>()
        ),
    )];
    // Most release names omit the year; only a named, different year excludes.
    if let Some(year) = request.target.year
        && !analysis.year.claims.is_empty()
    {
        criteria.push(require_fact("year", &analysis.year, &[year]));
    }
    let mut season_matched = false;
    if let Some(season) = request.target.season {
        // Several season claims describe a multi-season pack, not a conflict.
        let state = if analysis.season.claims.is_empty() {
            CriterionState::Unknown
        } else if analysis
            .season
            .claims
            .iter()
            .any(|claim| claim.value == season)
        {
            CriterionState::Match
        } else {
            CriterionState::Mismatch
        };
        season_matched = state == CriterionState::Match;
        criteria.push(evaluation(
            "season",
            state,
            json!([season]),
            json!(analysis.season),
        ));
    }
    if let Some(part) = request.target.part {
        criteria.push(require_fact("part", &analysis.part, &[part]));
    }
    if let Some(episodes) = request.target.episodes {
        let matches: Vec<_> = analysis
            .episodes
            .claims
            .iter()
            .map(|claim| covers(&claim.value, episodes))
            .collect();
        let pack = analysis.coverage.claims.iter().any(|claim| {
            matches!(
                claim.value,
                CoverageKind::SeasonPack
                    | CoverageKind::MultiSeasonPack
                    | CoverageKind::CompleteClaim
            )
        });
        if matches.is_empty() && pack && season_matched {
            // A pack of the requested season contains its episodes.
            criteria.push(CriterionEvaluation {
                field: "episodes".into(),
                state: CriterionState::Match,
                reason_code: "episodes_in_season_pack".into(),
                expected: json!(episodes),
                observed: json!(analysis.coverage),
            });
        } else {
            criteria.push(evaluation(
                "episodes",
                state_from_matches(&matches),
                json!(episodes),
                json!(analysis.episodes),
            ));
        }
    }
    if !request.constraints.resolutions.is_empty() {
        criteria.push(require_fact(
            "resolution",
            &analysis.resolution,
            &request.constraints.resolutions,
        ));
    }
    if !request.constraints.codecs.is_empty() {
        criteria.push(require_fact(
            "video_codec",
            &analysis.video_codec,
            &request.constraints.codecs,
        ));
    }
    if !request.constraints.audio_languages.is_empty() {
        // A release may carry several audio tracks: any requested language
        // matches. Only explicit labels without it ("Chinese Audio") exclude.
        let allowed = &request.constraints.audio_languages;
        let claims = &analysis.audio_language.claims;
        let state = if claims.iter().any(|claim| allowed.contains(&claim.value)) {
            CriterionState::Match
        } else if !claims.is_empty() && claims.iter().all(|claim| !claim.inferred) {
            CriterionState::Mismatch
        } else {
            CriterionState::Unknown
        };
        criteria.push(evaluation(
            "audio_language",
            state,
            json!(allowed),
            json!(analysis.audio_language),
        ));
    }
    if !request.constraints.reject_formats.is_empty() {
        let allowed: Vec<String> = super::SOURCE_FORMATS
            .iter()
            .filter(|format| {
                !request
                    .constraints
                    .reject_formats
                    .iter()
                    .any(|r| r == *format)
            })
            .map(|format| (*format).into())
            .collect();
        let matches: Vec<_> = analysis
            .source_format
            .claims
            .iter()
            .map(|claim| allowed.contains(&claim.value))
            .collect();
        // An unnamed format is not evidence of a capture: only a named
        // rejected format excludes, and a mixed claim is uncertain. Allowed
        // formats add no criterion, keeping the response short.
        let state = state_from_matches(&matches);
        if !matches.is_empty() && state != CriterionState::Match {
            criteria.push(CriterionEvaluation {
                field: "source_format".into(),
                state,
                reason_code: if state == CriterionState::Mismatch {
                    "source_format_rejected"
                } else {
                    "source_format_mixed"
                }
                .into(),
                expected: json!({"reject": request.constraints.reject_formats}),
                observed: json!(analysis.source_format),
            });
        }
    }
    if !request.constraints.formats.is_empty() {
        criteria.push(require_fact(
            "source_format",
            &analysis.source_format,
            &request.constraints.formats,
        ));
    }
    if let Some(coverage) = request.constraints.coverage {
        criteria.push(require_fact("coverage", &analysis.coverage, &[coverage]));
    }
    let sizes: Vec<_> = result
        .observations
        .iter()
        .filter_map(|origin| origin.size_bytes)
        .collect();
    for (field, bound, is_minimum) in [
        ("min_size_bytes", request.constraints.min_size_bytes, true),
        ("max_size_bytes", request.constraints.max_size_bytes, false),
    ] {
        if let Some(bound) = bound {
            let matches: Vec<_> = sizes
                .iter()
                .map(|size| {
                    if is_minimum {
                        *size >= bound
                    } else {
                        *size <= bound
                    }
                })
                .collect();
            criteria.push(evaluation(
                field,
                state_from_matches(&matches),
                json!(bound),
                json!(sizes),
            ));
        }
    }
    if let Some(minimum) = request.constraints.min_seeders {
        let observed = result
            .observations
            .iter()
            .filter_map(|origin| origin.seeders)
            .max();
        criteria.push(evaluation(
            "highest_reported_seeders",
            observed.map_or(CriterionState::Unknown, |count| {
                if count >= minimum {
                    CriterionState::Match
                } else {
                    CriterionState::Mismatch
                }
            }),
            json!(minimum),
            json!(observed),
        ));
    }
    criteria
}

fn add_preference<T: PartialEq>(
    field: &str,
    fact: &Fact<T>,
    preferred: &[T],
    weight: u32,
    ranking: &mut Ranking,
) {
    if preferred.is_empty() || fact.claims.len() != 1 {
        return;
    }
    if let Some(index) = preferred
        .iter()
        .position(|value| value == &fact.claims[0].value)
    {
        let points = weight * (preferred.len() - index) as u32 / preferred.len() as u32;
        ranking.preference_score += points;
        ranking.factors.push(PreferenceFactor {
            field: field.into(),
            points,
            matched_preference_index: index,
        });
    }
}

/// For facts that hold several values at once (audio tracks): score the most
/// preferred value present.
fn add_set_preference(
    field: &str,
    fact: &Fact<String>,
    preferred: &[String],
    weight: u32,
    ranking: &mut Ranking,
) {
    if let Some(index) = preferred
        .iter()
        .position(|value| fact.claims.iter().any(|claim| &claim.value == value))
    {
        let points = weight * (preferred.len() - index) as u32 / preferred.len() as u32;
        ranking.preference_score += points;
        ranking.factors.push(PreferenceFactor {
            field: field.into(),
            points,
            matched_preference_index: index,
        });
    }
}

pub fn analyze_report(
    report: SearchReport,
    request: AnalysisRequest,
) -> Result<AnalyzedReport, SearchError> {
    request.validate()?;
    validate_report(&report)?;
    let mut results = Vec::new();
    let mut summary = AnalysisSummary::default();
    for result in report.results {
        let analysis = analyze_release(&result, &request);
        let evaluations = evaluate(&result, &analysis, &request);
        let eligibility = if evaluations
            .iter()
            .any(|criterion| criterion.state == CriterionState::Mismatch)
        {
            Eligibility::Excluded
        } else if evaluations
            .iter()
            .any(|criterion| criterion.state == CriterionState::Unknown)
        {
            Eligibility::Uncertain
        } else {
            Eligibility::Eligible
        };
        match eligibility {
            Eligibility::Eligible => summary.eligible += 1,
            Eligibility::Uncertain => summary.uncertain += 1,
            Eligibility::Excluded => summary.excluded += 1,
        }
        let seeders = result
            .observations
            .iter()
            .filter_map(|origin| origin.seeders)
            .max();
        let mut ranking = Ranking {
            preference_score: 0,
            factors: vec![],
            quality_score: super::quality_score(&analysis),
            availability: Availability::from_seeders(seeders),
            highest_reported_seeders: seeders,
        };
        add_preference(
            "resolution",
            &analysis.resolution,
            &request.preferences.resolutions,
            100,
            &mut ranking,
        );
        add_preference(
            "source_format",
            &analysis.source_format,
            &request.preferences.formats,
            80,
            &mut ranking,
        );
        add_preference(
            "video_codec",
            &analysis.video_codec,
            &request.preferences.codecs,
            60,
            &mut ranking,
        );
        add_set_preference(
            "audio_language",
            &analysis.audio_language,
            &request.preferences.audio_languages,
            40,
            &mut ranking,
        );
        add_preference(
            "coverage",
            &analysis.coverage,
            &request.preferences.coverage,
            40,
            &mut ranking,
        );
        results.push(AnalyzedResult {
            result,
            analysis,
            eligibility,
            evaluations,
            ranking,
        });
    }
    let eligibility_order = |value| match value {
        Eligibility::Eligible => 0,
        Eligibility::Uncertain => 1,
        Eligibility::Excluded => 2,
    };
    results.sort_by(|a, b| {
        eligibility_order(a.eligibility)
            .cmp(&eligibility_order(b.eligibility))
            .then_with(|| b.ranking.preference_score.cmp(&a.ranking.preference_score))
            .then_with(|| {
                a.ranking
                    .availability
                    .order()
                    .cmp(&b.ranking.availability.order())
            })
            .then_with(|| b.ranking.quality_score.cmp(&a.ranking.quality_score))
            .then_with(|| {
                b.ranking
                    .highest_reported_seeders
                    .cmp(&a.ranking.highest_reported_seeders)
            })
            .then_with(|| a.result.title.cmp(&b.result.title))
            .then_with(|| a.result.reference.uri().cmp(b.result.reference.uri()))
    });
    summary.total = results.len();
    Ok(AnalyzedReport {
        schema_version: 1,
        policy_version: "release-analysis-v2".into(),
        query: report.query,
        category: report.category,
        partial: report.partial,
        request,
        sources: report.sources,
        summary,
        results,
    })
}
