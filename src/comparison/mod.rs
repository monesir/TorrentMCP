//! Evaluate measured file/stream properties, separately from release-name claims.
mod evaluate;
mod ranking;

use crate::{
    SearchError,
    agent::{AgentFailure, AgentStatus},
    analysis::{AnalysisSummary, CriterionEvaluation, Eligibility},
    torrent::TorrentOperationReport,
};
pub use evaluate::assess_inspection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConstraints {
    pub min_video_width: Option<u64>,
    pub min_video_height: Option<u64>,
    pub video_codecs: Vec<String>,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub min_file_size_bytes: Option<u64>,
    pub max_file_size_bytes: Option<u64>,
    pub min_video_bitrate_bps: Option<u64>,
    pub max_video_bitrate_bps: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRankCriterion {
    VideoCodecs,
    AudioLanguages,
    SubtitleLanguages,
    SmallerFile,
    LargerFile,
    HigherVideoBitrate,
    LowerVideoBitrate,
    HigherVideoPixels,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaPreferences {
    pub video_codecs: Vec<String>,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    /// In priority order. Empty uses only the nonempty codec/language lists, then id.
    pub rank_by: Vec<MediaRankCriterion>,
}
impl MediaPreferences {
    pub fn effective_order(&self) -> Vec<MediaRankCriterion> {
        if !self.rank_by.is_empty() {
            return self.rank_by.clone();
        }
        let mut order = Vec::new();
        if !self.video_codecs.is_empty() {
            order.push(MediaRankCriterion::VideoCodecs);
        }
        if !self.audio_languages.is_empty() {
            order.push(MediaRankCriterion::AudioLanguages);
        }
        if !self.subtitle_languages.is_empty() {
            order.push(MediaRankCriterion::SubtitleLanguages);
        }
        order
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaAnalysisRequest {
    pub constraints: MediaConstraints,
    pub preferences: MediaPreferences,
}
impl MediaAnalysisRequest {
    pub fn validate(&self) -> Result<(), SearchError> {
        let c = &self.constraints;
        if [c.min_video_width, c.min_video_height]
            .into_iter()
            .flatten()
            .any(|v| !(1..=65535).contains(&v))
            || [
                c.min_file_size_bytes,
                c.max_file_size_bytes,
                c.min_video_bitrate_bps,
                c.max_video_bitrate_bps,
            ]
            .into_iter()
            .flatten()
            .any(|v| v == 0)
            || c.min_file_size_bytes
                .zip(c.max_file_size_bytes)
                .is_some_and(|(a, b)| a > b)
            || c.min_video_bitrate_bps
                .zip(c.max_video_bitrate_bps)
                .is_some_and(|(a, b)| a > b)
        {
            return invalid("Invalid media dimensions or min/max bounds");
        }
        for codecs in [&c.video_codecs, &self.preferences.video_codecs] {
            if codecs.len() > 32
                || codecs.iter().any(|s| {
                    s.is_empty()
                        || s.len() > 32
                        || !s
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                })
                || codecs.iter().collect::<HashSet<_>>().len() != codecs.len()
            {
                return invalid("Use unique lowercase ffprobe video codec names");
            }
        }
        for languages in [
            &c.audio_languages,
            &c.subtitle_languages,
            &self.preferences.audio_languages,
            &self.preferences.subtitle_languages,
        ] {
            let mut seen = HashSet::new();
            if languages.len() > 32
                || languages.iter().any(|s| {
                    if !(2..=3).contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_lowercase()) {
                        return true;
                    }
                    match canonical_language(s) {
                        Some(code) => !seen.insert(code),
                        None => true,
                    }
                })
            {
                return invalid(
                    "Use unique known lowercase language codes; und/mul/zxx are not requested languages",
                );
            }
        }
        let p = &self.preferences;
        let ranks: HashSet<_> = p.rank_by.iter().copied().collect();
        if ranks.len() != p.rank_by.len()
            || ranks.len() > 8
            || (ranks.contains(&MediaRankCriterion::SmallerFile)
                && ranks.contains(&MediaRankCriterion::LargerFile))
            || (ranks.contains(&MediaRankCriterion::HigherVideoBitrate)
                && ranks.contains(&MediaRankCriterion::LowerVideoBitrate))
        {
            return invalid("Use distinct, nonconflicting ranking criteria");
        }
        for (field, list) in [
            (MediaRankCriterion::VideoCodecs, &p.video_codecs),
            (MediaRankCriterion::AudioLanguages, &p.audio_languages),
            (MediaRankCriterion::SubtitleLanguages, &p.subtitle_languages),
        ] {
            if (!p.rank_by.is_empty() && !list.is_empty() && !ranks.contains(&field))
                || (ranks.contains(&field) && list.is_empty())
            {
                return invalid("Preference lists and explicit rank_by must agree");
            }
        }
        Ok(())
    }
}

pub(crate) fn invalid<T>(message: &str) -> Result<T, SearchError> {
    Err(SearchError::InvalidRequest(message.into()))
}
pub(crate) fn canonical_language(code: &str) -> Option<String> {
    if matches!(code, "und" | "mul" | "zxx") {
        return None;
    }
    crate::media::normalize_language(code)
}

/// The saved agent envelope. Older records without elapsed_ms remain readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTorrentInspection {
    pub schema_version: u32,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub status: AgentStatus,
    pub result: Option<TorrentOperationReport>,
    pub error: Option<AgentFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectedMediaCandidate {
    pub id: String,
    pub inspection: SavedTorrentInspection,
    #[serde(default)]
    pub video_stream_index: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCriterionEvaluation {
    #[serde(flatten)]
    pub criterion: CriterionEvaluation,
    pub reason: String,
    pub evidence_source: String,
    pub stream_indices: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPreferenceFactor {
    pub criterion: MediaRankCriterion,
    pub direction: String,
    pub value: Option<u64>,
    pub matched_preference: Option<String>,
    pub reason_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAssessment {
    pub schema_version: u32,
    pub policy_version: String,
    pub analysis: MediaAnalysisRequest,
    pub inspection_complete: bool,
    pub video_stream_index: Option<u64>,
    pub eligibility: Eligibility,
    pub evaluations: Vec<MediaCriterionEvaluation>,
    pub preference_factors: Vec<MediaPreferenceFactor>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ComparedMediaCandidate {
    #[serde(flatten)]
    pub candidate: InspectedMediaCandidate,
    pub position: usize,
    pub assessment: MediaAssessment,
}

#[derive(Debug, Serialize)]
pub struct MediaComparisonReport {
    pub schema_version: u32,
    pub policy_version: String,
    pub analysis: MediaAnalysisRequest,
    pub ranking_criteria: Vec<MediaRankCriterion>,
    pub partial: bool,
    pub summary: AnalysisSummary,
    pub results: Vec<ComparedMediaCandidate>,
    pub limitations: Vec<String>,
}

pub fn validate_candidates(candidates: &[InspectedMediaCandidate]) -> Result<(), SearchError> {
    if candidates.is_empty() || candidates.len() > 100 {
        return invalid("Supply 1..100 inspected candidates");
    }
    let mut ids = HashSet::new();
    let mut files = HashSet::new();
    for candidate in candidates {
        if candidate.id.trim().is_empty()
            || candidate.id.len() > 256
            || candidate.id.chars().any(char::is_control)
            || !ids.insert(&candidate.id)
        {
            return invalid("Candidate ids must be unique, nonempty and at most 256 bytes");
        }
        validate_inspection(&candidate.inspection)?;
        if let Some(report) = &candidate.inspection.result
            && let (Some(torrent), Some(file)) = (&report.torrent, &report.file)
            && !files.insert((torrent.info_hash_v1.clone(), file.id))
        {
            return invalid("The same torrent file cannot appear twice in a comparison");
        }
        if let Some(index) = candidate.video_stream_index {
            let valid = candidate
                .inspection
                .result
                .as_ref()
                .and_then(|r| r.media.as_ref())
                .is_some_and(|m| {
                    m.streams
                        .iter()
                        .any(|s| s.index == index && video_stream(s))
                });
            if !valid {
                return invalid(
                    "video_stream_index must identify a real video stream in this inspection",
                );
            }
        }
    }
    Ok(())
}

pub fn validate_inspection(inspection: &SavedTorrentInspection) -> Result<(), SearchError> {
    if inspection.schema_version != 1 {
        return invalid("Unsupported saved inspection schema_version");
    }
    let Some(report) = &inspection.result else {
        if inspection.status == AgentStatus::Completed {
            return invalid("Completed inspection has no result");
        }
        return Ok(());
    };
    if report.schema_version != 1
        || !matches!(
            report.operation.as_str(),
            "inspect_torrent_file" | "torrent_files"
        )
    {
        return invalid("Expected a torrent file inspection or file catalog report");
    }
    if let Some(torrent) = &report.torrent {
        torrent.validate()?;
    }
    if let Some(file) = &report.file {
        let torrent = report.torrent.as_ref().ok_or_else(|| {
            SearchError::InvalidRequest("Inspected file has no torrent identity".into())
        })?;
        if torrent.selected_file(file.id)? != file {
            return invalid("Inspected file differs from its torrent catalog");
        }
    }
    if let Some(media) = &report.media {
        if report.operation != "inspect_torrent_file"
            || report.file.as_ref().map(|f| f.size_bytes) != Some(media.format.file_size_bytes)
        {
            return invalid("Media inspection size differs from selected torrent file");
        }
        let mut indices = HashSet::new();
        if media.schema_version != 1
            || media.evidence_source != "ffprobe_media_headers"
            || media.inspection_scope != "bounded_header_probe"
            || media.streams.len() > 10000
            || media.streams.iter().any(|s| !indices.insert(s.index))
        {
            return invalid("Invalid media inspection version, evidence source or stream indices");
        }
        // A selection cannot silently refer to streams absent from the saved header report.
        if media
            .selection
            .selected_stream_indices
            .iter()
            .any(|i| !indices.contains(i))
        {
            return invalid("Selected stream is absent from media report");
        }
    }
    Ok(())
}

pub(crate) fn video_stream(stream: &crate::media::MediaStream) -> bool {
    stream.kind == "video"
        && !stream
            .dispositions
            .get("attached_pic")
            .copied()
            .unwrap_or(false)
}

pub(crate) fn bitrate(stream: &crate::media::MediaStream) -> Option<u64> {
    if !matches!(
        stream.bitrate_bps.basis.as_str(),
        "ffprobe_stream" | "container_statistics_tag"
    ) {
        return None;
    }
    stream.bitrate_bps.value.filter(|v| *v > 0)
}

pub(crate) fn codec(stream: &crate::media::MediaStream) -> Option<&str> {
    stream
        .codec
        .as_deref()
        .filter(|value| !matches!(*value, "" | "unknown" | "N/A"))
}

pub fn compare_media(
    candidates: Vec<InspectedMediaCandidate>,
    analysis: MediaAnalysisRequest,
) -> Result<MediaComparisonReport, SearchError> {
    analysis.validate()?;
    validate_candidates(&candidates)?;
    let mut results = Vec::with_capacity(candidates.len());
    let mut summary = AnalysisSummary::default();
    let mut partial = false;
    for candidate in candidates {
        let assessment = assess_inspection(
            candidate.inspection.result.as_ref(),
            candidate.inspection.status,
            candidate.inspection.error.as_ref(),
            &analysis,
            candidate.video_stream_index,
        );
        partial |= !assessment.inspection_complete;
        match assessment.eligibility {
            Eligibility::Eligible => summary.eligible += 1,
            Eligibility::Uncertain => summary.uncertain += 1,
            Eligibility::Excluded => summary.excluded += 1,
        }
        results.push(ComparedMediaCandidate {
            candidate,
            position: 0,
            assessment,
        });
    }
    results.sort_by(ranking::compare);
    for (i, result) in results.iter_mut().enumerate() {
        result.position = i + 1;
    }
    summary.total = results.len();
    Ok(MediaComparisonReport {
        schema_version: 1,
        policy_version: "inspected-media-v1".into(),
        ranking_criteria: analysis.preferences.effective_order(),
        analysis,
        partial,
        summary,
        results,
        limitations: vec![
            "ranking_reflects_explicit_preferences_not_perceptual_quality".into(),
            "language_and_payload_size_tags_are_container_claims".into(),
            "no_file_download_or_network_access_in_compare_media".into(),
        ],
    })
}
