mod extraction;
mod matching;
mod quality;
mod title;

use crate::{SearchError, SearchReport, SearchResult, SourceReport};
pub use extraction::analyze_release;
pub use matching::analyze_report;
pub use quality::quality_score;
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
    /// Several seasons in one torrent, e.g. S01-S05 or "Season 1, 2, 3".
    MultiSeasonPack,
    CompleteClaim,
}

/// Canonical release source formats, best first. Theatre captures come last.
pub const SOURCE_FORMATS: &[&str] = &[
    "remux", "bluray", "webdl", "webrip", "hdtv", "hdrip", "dvdrip", "screener", "telecine",
    "telesync", "cam",
];

/// Theatre captures and screeners are rejected unless the request allows them.
pub const LOW_QUALITY_FORMATS: &[&str] = &["cam", "telesync", "telecine", "screener"];

/// Canonical audio language codes (ISO 639-1) recognised in release names.
pub const AUDIO_LANGUAGES: &[&str] = &[
    "zh", "en", "ar", "ja", "ko", "fr", "de", "es", "ru", "hi", "th", "it", "pt", "tr", "pl", "nl",
    "uk", "ta", "te", "fa", "he", "id", "vi",
];

fn default_reject_formats() -> Vec<String> {
    LOW_QUALITY_FORMATS
        .iter()
        .map(|&value| value.into())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Only these source formats are eligible, e.g. ["remux"]. A title that
    /// names no format is uncertain, not excluded.
    #[serde(default)]
    pub formats: Vec<String>,
    /// Source formats that exclude a release. Defaults to theatre captures and
    /// screeners; pass [] to allow every format.
    #[serde(default = "default_reject_formats")]
    pub reject_formats: Vec<String>,
}

impl Default for ReleaseConstraints {
    fn default() -> Self {
        Self {
            resolutions: vec![],
            codecs: vec![],
            audio_languages: vec![],
            coverage: None,
            min_size_bytes: None,
            max_size_bytes: None,
            min_seeders: None,
            formats: vec![],
            reject_formats: default_reject_formats(),
        }
    }
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
    /// Source formats in preference order, e.g. ["remux", "bluray", "webdl"].
    #[serde(default)]
    pub formats: Vec<String>,
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
                    !["h264", "hevc", "av1", "vp9", "mpeg2", "xvid"].contains(&codec.as_str())
                })
            {
                return Err(invalid(
                    "Use canonical codecs: h264, hevc, av1, vp9, mpeg2, xvid",
                ));
            }
        }
        for languages in [
            &self.constraints.audio_languages,
            &self.preferences.audio_languages,
        ] {
            if languages.len() > 16
                || languages
                    .iter()
                    .any(|language| !AUDIO_LANGUAGES.contains(&language.as_str()))
            {
                return Err(invalid(&format!(
                    "Unsupported audio language code; use one of: {}",
                    AUDIO_LANGUAGES.join(", ")
                )));
            }
        }
        for list in [
            &self.constraints.reject_formats,
            &self.constraints.formats,
            &self.preferences.formats,
        ] {
            if list.len() > SOURCE_FORMATS.len()
                || list
                    .iter()
                    .any(|format| !SOURCE_FORMATS.contains(&format.as_str()))
            {
                return Err(invalid(&format!(
                    "formats and reject_formats accept: {}",
                    SOURCE_FORMATS.join(", ")
                )));
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
    pub quality: QualitySignals,
    pub issues: Vec<String>,
}

/// Quality markers named in release titles, merged across all observed titles.
/// These are claims from names; inspect the file to measure the real streams.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct QualitySignals {
    /// dolby_vision, hdr10_plus, hdr10, hdr, hlg
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hdr: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u8>,
    /// truehd, dts_hd_ma, dts_x, dts, flac, eac3, ac3, opus, aac
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub audio_codecs: Vec<String>,
    /// Highest channel layout named, e.g. "7.1", "5.1", "2.0".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_channels: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub atmos: bool,
    /// MULTi, Dual Audio or several audio language tags.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub multi_audio: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repack: bool,
    /// UPSCALE or AI tags: the named resolution is not native.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub upscaled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_group: Option<String>,
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

/// Download health from the highest seeder count any source reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// At least HEALTHY_SEEDERS seeders.
    Healthy,
    /// 1 to HEALTHY_SEEDERS - 1 seeders; may be slow.
    Weak,
    /// No source reported a seeder count.
    Unknown,
    /// Every report says 0 seeders.
    Dead,
}

pub const HEALTHY_SEEDERS: u64 = 5;

impl Availability {
    pub fn from_seeders(seeders: Option<u64>) -> Self {
        match seeders {
            None => Self::Unknown,
            Some(0) => Self::Dead,
            Some(count) if count < HEALTHY_SEEDERS => Self::Weak,
            Some(_) => Self::Healthy,
        }
    }
    pub(crate) fn order(self) -> u8 {
        match self {
            Self::Healthy => 0,
            Self::Weak => 1,
            Self::Unknown => 2,
            Self::Dead => 3,
        }
    }
}

/// Results sort by eligibility, preference_score, availability, quality_score,
/// then seeders: the best named quality among releases that can be downloaded.
#[derive(Debug, Clone, Serialize)]
pub struct Ranking {
    pub preference_score: u32,
    pub factors: Vec<PreferenceFactor>,
    /// Estimate from title claims: source format, resolution, HDR, audio.
    pub quality_score: u32,
    pub availability: Availability,
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
