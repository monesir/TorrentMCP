mod extraction;
mod matching;
mod title;

use crate::{SearchError, SearchReport, SearchResult, SourceReport};
pub use extraction::analyze_release;
pub use matching::analyze_report;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRequest {
    pub target: SearchTarget,
    #[serde(default)]
    pub constraints: ReleaseConstraints,
    #[serde(default)]
    pub preferences: ReleasePreferences,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchTarget {
    pub title: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub season: Option<u16>,
    #[serde(default)]
    pub part: Option<u16>,
    #[serde(default)]
    pub episodes: Option<EpisodeRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeRange {
    pub start: u16,
    pub end: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageKind {
    SingleEpisode,
    EpisodeBatch,
    SeasonPack,
    CompleteClaim,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseConstraints {
    #[serde(default)]
    pub resolutions: Vec<u16>,
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default)]
    pub audio_languages: Vec<String>,
    #[serde(default)]
    pub coverage: Option<CoverageKind>,
    #[serde(default)]
    pub min_size_bytes: Option<u64>,
    #[serde(default)]
    pub max_size_bytes: Option<u64>,
    #[serde(default)]
    pub min_seeders: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePreferences {
    #[serde(default)]
    pub resolutions: Vec<u16>,
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default)]
    pub audio_languages: Vec<String>,
    #[serde(default)]
    pub coverage: Vec<CoverageKind>,
}

impl AnalysisRequest {
    pub fn validate(&self) -> Result<(), SearchError> {
        let invalid = |message: &str| SearchError::InvalidRequest(message.into());
        if self.target.title.trim().is_empty()
            || self.target.title.chars().count() > 512
            || self.target.aliases.len() > 32
            || self
                .target
                .aliases
                .iter()
                .any(|alias| alias.trim().is_empty() || alias.chars().count() > 512)
        {
            return Err(invalid(
                "Target title and aliases must contain 1..512 characters; at most 32 aliases",
            ));
        }
        if self
            .target
            .year
            .is_some_and(|year| !(1880..=2100).contains(&year))
            || self.target.season.is_some_and(|number| number > 999)
            || self
                .target
                .part
                .is_some_and(|number| !(1..=99).contains(&number))
            || self.target.episodes.is_some_and(|range| {
                range.start == 0 || range.start > range.end || range.end > 9999
            })
        {
            return Err(invalid(
                "Invalid target year, season, part or episode range",
            ));
        }
        if self
            .constraints
            .min_size_bytes
            .zip(self.constraints.max_size_bytes)
            .is_some_and(|(min, max)| min > max)
        {
            return Err(invalid("min_size_bytes must not exceed max_size_bytes"));
        }
        if self.preferences.coverage.len() > 4 {
            return Err(invalid("At most four coverage preferences are supported"));
        }
        for resolutions in [&self.constraints.resolutions, &self.preferences.resolutions] {
            if resolutions.len() > 8
                || resolutions
                    .iter()
                    .any(|height| ![480, 576, 720, 1080, 1440, 2160, 4320].contains(height))
            {
                return Err(invalid("Unsupported resolution height"));
            }
        }
        for codecs in [&self.constraints.codecs, &self.preferences.codecs] {
            if codecs.len() > 8
                || codecs.iter().any(|codec| {
                    !["h264", "hevc", "av1", "mpeg2", "xvid"].contains(&codec.as_str())
                })
            {
                return Err(invalid(
                    "Use canonical codecs: h264, hevc, av1, mpeg2, xvid",
                ));
            }
        }
        for languages in [
            &self.constraints.audio_languages,
            &self.preferences.audio_languages,
        ] {
            if languages.len() > 16
                || languages.iter().any(|language| {
                    ![
                        "zh", "en", "ar", "ja", "ko", "fr", "de", "es", "ru", "hi", "th",
                    ]
                    .contains(&language.as_str())
                })
            {
                return Err(invalid("Unsupported canonical audio language code"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactState {
    Unknown,
    Claimed,
    Inferred,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractionEvidence {
    pub source_id: String,
    pub title_index: usize,
    pub token: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Claim<T> {
    pub value: T,
    pub inferred: bool,
    pub evidence: Vec<ExtractionEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fact<T> {
    pub state: FactState,
    pub claims: Vec<Claim<T>>,
}

impl<T> Default for Fact<T> {
    fn default() -> Self {
        Self {
            state: FactState::Unknown,
            claims: vec![],
        }
    }
}

impl<T: PartialEq> Fact<T> {
    fn push(&mut self, value: T, evidence: ExtractionEvidence, inferred: bool) {
        if let Some(existing) = self.claims.iter_mut().find(|claim| claim.value == value) {
            existing.evidence.push(evidence);
            existing.inferred &= inferred;
        } else {
            self.claims.push(Claim {
                value,
                inferred,
                evidence: vec![evidence],
            });
        }
        self.state = if self.claims.len() > 1 {
            FactState::Ambiguous
        } else if self.claims[0].inferred {
            FactState::Inferred
        } else {
            FactState::Claimed
        };
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TitleCorrection {
    pub rule: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzedTitle {
    pub source_id: String,
    pub raw: String,
    pub normalized: String,
    pub corrections: Vec<TitleCorrection>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReleaseAnalysis {
    pub titles: Vec<AnalyzedTitle>,
    pub year: Fact<u16>,
    pub season: Fact<u16>,
    pub part: Fact<u16>,
    pub episodes: Fact<Vec<EpisodeRange>>,
    pub resolution: Fact<u16>,
    pub video_codec: Fact<String>,
    pub audio_language: Fact<String>,
    pub subtitle_language: Fact<String>,
    pub source_format: Fact<String>,
    pub coverage: Fact<CoverageKind>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eligibility {
    Eligible,
    Uncertain,
    Excluded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionState {
    Match,
    Mismatch,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionEvaluation {
    pub field: String,
    pub state: CriterionState,
    pub reason_code: String,
    pub expected: serde_json::Value,
    pub observed: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreferenceFactor {
    pub field: String,
    pub points: u32,
    pub matched_preference_index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ranking {
    pub preference_score: u32,
    pub factors: Vec<PreferenceFactor>,
    pub highest_reported_seeders: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzedResult {
    #[serde(flatten)]
    pub result: SearchResult,
    pub analysis: ReleaseAnalysis,
    pub eligibility: Eligibility,
    pub evaluations: Vec<CriterionEvaluation>,
    pub ranking: Ranking,
}

#[derive(Debug, Default, Serialize)]
pub struct AnalysisSummary {
    pub total: usize,
    pub eligible: usize,
    pub uncertain: usize,
    pub excluded: usize,
}

#[derive(Debug, Serialize)]
pub struct AnalyzedReport {
    pub schema_version: u32,
    pub policy_version: String,
    pub query: String,
    pub category: String,
    pub partial: bool,
    pub request: AnalysisRequest,
    pub sources: Vec<SourceReport>,
    pub summary: AnalysisSummary,
    pub results: Vec<AnalyzedResult>,
}

pub fn validate_report(report: &SearchReport) -> Result<(), SearchError> {
    if report.schema_version != 1
        || report.results.len() > 20_000
        || report.results.iter().any(|result| {
            result.title.len() > 16 * 1024
                || result.observations.len() > 1000
                || result.observations.iter().any(|origin| {
                    origin.title.len() > 16 * 1024
                        || origin
                            .raw_title
                            .as_ref()
                            .is_some_and(|title| title.len() > 16 * 1024)
                })
        })
    {
        return Err(SearchError::InvalidRequest(
            "Unsupported or oversized source report".into(),
        ));
    }
    Ok(())
}
