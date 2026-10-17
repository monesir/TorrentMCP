use super::*;
use crate::{
    analysis::CriterionState,
    media::{MediaInspectionReport, MediaStream},
};
use serde_json::{Value, json};

pub(crate) fn selected_video(
    media: &MediaInspectionReport,
    explicit: Option<u64>,
) -> Option<&MediaStream> {
    let videos: Vec<_> = media.streams.iter().filter(|s| video_stream(s)).collect();
    if let Some(index) = explicit {
        return videos.into_iter().find(|s| s.index == index);
    }
    if videos.len() == 1 {
        return videos.first().copied();
    }
    if media.selection.video_selection_unresolved {
        return None;
    }
    let selected: Vec<_> = videos
        .into_iter()
        .filter(|s| media.selection.selected_stream_indices.contains(&s.index))
        .collect();
    if selected.len() == 1 {
        selected.first().copied()
    } else {
        None
    }
}

fn criterion(
    field: &str,
    state: CriterionState,
    reason_code: &str,
    reason: &str,
    expected: Value,
    observed: Value,
    evidence_source: &str,
    stream_indices: Vec<u64>,
) -> MediaCriterionEvaluation {
    MediaCriterionEvaluation {
        criterion: CriterionEvaluation {
            field: field.into(),
            state,
            reason_code: reason_code.into(),
            expected,
            observed,
        },
        reason: reason.into(),
        evidence_source: evidence_source.into(),
        stream_indices,
    }
}

fn bound(
    field: &str,
    value: Option<u64>,
    expected: u64,
    minimum: bool,
    source: &str,
    indices: Vec<u64>,
) -> MediaCriterionEvaluation {
    let state = match value {
        None => CriterionState::Unknown,
        Some(v)
            if if minimum {
                v >= expected
            } else {
                v <= expected
            } =>
        {
            CriterionState::Match
        }
        Some(_) => CriterionState::Mismatch,
    };
    let (code, reason) = match state {
        CriterionState::Match => (
            "measured_bound_matches",
            "القيمة المقروءة تحقق الحد المطلوب",
        ),
        CriterionState::Mismatch => (
            "measured_bound_violated",
            "القيمة المقروءة تخالف الحد المطلوب",
        ),
        CriterionState::Unknown => (
            "measured_value_unavailable",
            "لا تتوفر قيمة مقروءة يمكن الحكم بها على هذا الشرط",
        ),
    };
    criterion(
        field,
        state,
        code,
        reason,
        json!(expected),
        json!(value),
        source,
        indices,
    )
}

fn language_criteria(
    kind: &str,
    required: &[String],
    media: Option<&MediaInspectionReport>,
    complete: bool,
) -> Vec<MediaCriterionEvaluation> {
    let streams: Vec<_> = media
        .map(|m| m.streams.iter().filter(|s| s.kind == kind).collect())
        .unwrap_or_default();
    let unknown = streams
        .iter()
        .any(|s: &&MediaStream| s.language.as_deref().and_then(canonical_language).is_none());
    let observed = json!(
        streams
            .iter()
            .map(|s| json!({"index":s.index,"language":s.language,"codec":s.codec,"title":s.title}))
            .collect::<Vec<_>>()
    );
    required
        .iter()
        .map(|requested| {
            let language = canonical_language(requested).unwrap_or_else(|| requested.clone());
            let matching: Vec<_> = streams
                .iter()
                .filter(|s| {
                    s.language.as_deref().and_then(canonical_language).as_ref() == Some(&language)
                })
                .map(|s| s.index)
                .collect();
            let (state, code, reason) = if !matching.is_empty() {
                (
                    CriterionState::Match,
                    "required_language_present",
                    "يوجد مسار باللغة المطلوبة بحسب وسم الحاوية",
                )
            } else if !complete || unknown {
                (
                    CriterionState::Unknown,
                    "required_language_unresolved",
                    "اللغة المطلوبة غير مثبتة؛ توجد لغة مجهولة أو أن الفحص غير مكتمل",
                )
            } else {
                (
                    CriterionState::Mismatch,
                    "required_language_absent",
                    "لا يوجد مسار موسوم باللغة المطلوبة في الملف المفحوص",
                )
            };
            criterion(
                if kind == "audio" {
                    "audio_languages"
                } else {
                    "subtitle_languages"
                },
                state,
                code,
                reason,
                json!(language),
                observed.clone(),
                "container_language_tags",
                matching,
            )
        })
        .collect()
}

pub fn assess_inspection(
    report: Option<&TorrentOperationReport>,
    status: AgentStatus,
    error: Option<&AgentFailure>,
    analysis: &MediaAnalysisRequest,
    explicit_video: Option<u64>,
) -> MediaAssessment {
    let media = report.and_then(|r| r.media.as_ref());
    let complete = status == AgentStatus::Completed
        && error.is_none()
        && report.is_some_and(|r| {
            r.operation == "inspect_torrent_file"
                && r.service.stopped
                && r.service.shutdown_error.is_none()
        })
        && media.is_some_and(|m| {
            !m.streams.is_empty()
                && !m.limitations.iter().any(|l| {
                    matches!(
                        l.as_str(),
                        "probe_timeout"
                            | "probe_failed"
                            | "probe_diagnostics"
                            | "read_budget_exceeded"
                            | "source_read_failed"
                            | "cancelled"
                    )
                })
        });
    let video = media.and_then(|m| selected_video(m, explicit_video));
    let mut evaluations = vec![criterion(
        "inspection_complete",
        if complete {
            CriterionState::Match
        } else {
            CriterionState::Unknown
        },
        if complete {
            "inspection_completed"
        } else {
            "inspection_incomplete"
        },
        if complete {
            "اكتمل فحص رؤوس الملف وأكد إغلاق الخدمة"
        } else {
            "الفحص مفقود أو جزئي أو متعثر؛ لا يثبت غياب الخاصية"
        },
        json!(true),
        json!({"status":status,"has_media":media.is_some(),"error":error.map(|e| &e.code)}),
        "inspection_status",
        vec![],
    )];
    let c = &analysis.constraints;
    let indices = video.map(|s| vec![s.index]).unwrap_or_default();
    for (field, expected, value) in [
        (
            "min_video_width",
            c.min_video_width,
            video.and_then(|s| s.width).filter(|v| *v > 0),
        ),
        (
            "min_video_height",
            c.min_video_height,
            video.and_then(|s| s.height).filter(|v| *v > 0),
        ),
    ] {
        if let Some(expected) = expected {
            evaluations.push(bound(
                field,
                value,
                expected,
                true,
                "ffprobe_media_headers",
                indices.clone(),
            ));
        }
    }
    if !c.video_codecs.is_empty() {
        let value = video.and_then(codec);
        let state = value.map_or(CriterionState::Unknown, |v| {
            if c.video_codecs.iter().any(|a| a == v) {
                CriterionState::Match
            } else {
                CriterionState::Mismatch
            }
        });
        evaluations.push(criterion(
            "video_codecs",
            state,
            match state {
                CriterionState::Match => "video_codec_allowed",
                CriterionState::Mismatch => "video_codec_not_allowed",
                CriterionState::Unknown => "video_codec_unavailable",
            },
            match state {
                CriterionState::Match => "ترميز مسار الفيديو ضمن الترميزات المقبولة",
                CriterionState::Mismatch => "ترميز مسار الفيديو خارج الترميزات المقبولة",
                CriterionState::Unknown => "تعذر تحديد ترميز مسار فيديو واحد",
            },
            json!(c.video_codecs),
            json!(value),
            "ffprobe_media_headers",
            indices.clone(),
        ));
    }
    let file_size = report.and_then(|r| r.file.as_ref()).map(|f| f.size_bytes);
    for (field, expected, minimum) in [
        ("min_file_size_bytes", c.min_file_size_bytes, true),
        ("max_file_size_bytes", c.max_file_size_bytes, false),
    ] {
        if let Some(expected) = expected {
            evaluations.push(bound(
                field,
                file_size,
                expected,
                minimum,
                "torrent_file_metadata",
                vec![],
            ));
        }
    }
    for (field, expected, minimum) in [
        ("min_video_bitrate_bps", c.min_video_bitrate_bps, true),
        ("max_video_bitrate_bps", c.max_video_bitrate_bps, false),
    ] {
        if let Some(expected) = expected {
            let source = video
                .map(|s| s.bitrate_bps.basis.as_str())
                .unwrap_or("unknown");
            evaluations.push(bound(
                field,
                video.and_then(bitrate),
                expected,
                minimum,
                source,
                indices.clone(),
            ));
        }
    }
    evaluations.extend(language_criteria(
        "audio",
        &c.audio_languages,
        media,
        complete,
    ));
    evaluations.extend(language_criteria(
        "subtitle",
        &c.subtitle_languages,
        media,
        complete,
    ));
    let eligibility = if evaluations
        .iter()
        .any(|e| e.criterion.state == CriterionState::Mismatch)
    {
        Eligibility::Excluded
    } else if evaluations
        .iter()
        .any(|e| e.criterion.state == CriterionState::Unknown)
    {
        Eligibility::Uncertain
    } else {
        Eligibility::Eligible
    };
    let preference_factors =
        super::ranking::factors(report, video, complete, &analysis.preferences);
    let mut limitations = vec![
        "bounded_headers_and_container_tags_not_full_file_validation".into(),
        "higher_bitrate_or_more_pixels_do_not_prove_better_visual_quality".into(),
        "file_size_is_for_the_selected_file_not_the_whole_torrent_or_a_remux".into(),
    ];
    if media.is_some_and(|m| m.streams.iter().filter(|s| video_stream(s)).count() > 1)
        && video.is_none()
    {
        limitations.push("multiple_video_streams_require_explicit_selection".into());
    }
    MediaAssessment {
        schema_version: 1,
        policy_version: "inspected-media-v1".into(),
        analysis: analysis.clone(),
        inspection_complete: complete,
        video_stream_index: video.map(|s| s.index),
        eligibility,
        evaluations,
        preference_factors,
        limitations,
    }
}
