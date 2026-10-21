use super::StreamSelection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaValue<T> {
    pub value: Option<T>,
    pub basis: String,
}

impl<T> MediaValue<T> {
    fn new(value: Option<T>, basis: &str) -> Self {
        Self {
            basis: if value.is_some() {
                basis.into()
            } else {
                "unknown".into()
            },
            value,
        }
    }
}

fn unknown_value() -> MediaValue<u64> {
    MediaValue::new(None, "unknown")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFormat {
    pub name: Option<String>,
    pub duration_seconds: Option<f64>,
    pub file_size_bytes: u64,
    pub overall_bitrate_bps: MediaValue<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaStream {
    pub index: u64,
    pub kind: String,
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub pixel_format: Option<String>,
    pub component_bit_depth: MediaValue<u64>,
    pub frame_rate: Option<String>,
    pub color_primaries: Option<String>,
    pub color_transfer: Option<String>,
    pub color_space: Option<String>,
    pub hdr_signals: Vec<String>,
    pub sample_rate_hz: Option<u64>,
    /// Bits per audio sample as the codec stores them; lossless codecs report
    /// it, lossy ones have none.
    #[serde(default = "unknown_value")]
    pub sample_bit_depth: MediaValue<u64>,
    pub channels: Option<u64>,
    pub channel_layout: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub bitrate_bps: MediaValue<u64>,
    /// The single video track's bitrate when only the file's total is known:
    /// the total minus every other track and the attachments. Approximate
    /// (container overhead counts as video); never used to filter or rank.
    #[serde(default = "unknown_value")]
    pub estimated_bitrate_bps: MediaValue<u64>,
    pub payload_size_bytes: MediaValue<u64>,
    pub dispositions: BTreeMap<String, bool>,
    pub side_data: Vec<Value>,
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionReport {
    pub video_selection_unresolved: bool,
    pub selected_stream_indices: Vec<u64>,
    pub unclassified_stream_indices: Vec<u64>,
    pub omitted_stream_indices: Vec<u64>,
    pub missing_audio_languages: Vec<String>,
    pub missing_subtitle_languages: Vec<String>,
    pub selected_payload_bytes: MediaValue<u64>,
    pub omitted_payload_bytes: MediaValue<u64>,
    pub changes_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInspectionReport {
    pub schema_version: u32,
    pub evidence_source: String,
    pub inspection_scope: String,
    pub input_kind: String,
    pub format: MediaFormat,
    pub streams: Vec<MediaStream>,
    pub selection: SelectionReport,
    /// Reserved range lengths, including repeated and failed requests. An upper bound on source payload read.
    pub read_bytes_charged: u64,
    pub read_requests: u64,
    pub read_budget_bytes: u64,
    pub elapsed_ms: u64,
    pub limitations: Vec<String>,
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && *value != "unknown" && *value != "N/A")
        .map(str::to_owned)
}
fn number(value: &Value, field: &str) -> Option<u64> {
    value
        .get(field)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}
fn positive(value: &Value, field: &str) -> Option<u64> {
    number(value, field).filter(|value| *value > 0)
}
fn tags(value: &Value) -> BTreeMap<String, String> {
    value
        .get("tags")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.into())))
        .collect()
}
fn tag<'a>(tags: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}
pub(crate) fn language(code: &str) -> Option<String> {
    let code = code.trim().to_ascii_lowercase();
    Some(
        match code.as_str() {
            "" | "und" | "unknown" => return None,
            "eng" => "en",
            "ara" => "ar",
            "zho" | "chi" => "zh",
            "jpn" => "ja",
            "kor" => "ko",
            "fra" | "fre" => "fr",
            "deu" | "ger" => "de",
            "spa" => "es",
            "rus" => "ru",
            "hin" => "hi",
            "tha" => "th",
            "ita" => "it",
            "por" => "pt",
            "nld" | "dut" => "nl",
            "tur" => "tr",
            other
                if (2..=3).contains(&other.len())
                    && other.bytes().all(|byte| byte.is_ascii_lowercase()) =>
            {
                other
            }
            _ => return None,
        }
        .into(),
    )
}

fn bit_depth(raw: &Value) -> MediaValue<u64> {
    // bits_per_sample describes audio samples; it must not become video component depth.
    if let Some(value) = positive(raw, "bits_per_raw_sample") {
        return MediaValue::new(Some(value), "ffprobe_stream");
    }
    let pixel = text(raw, "pix_fmt").unwrap_or_default();
    let depth = match pixel.as_str() {
        "yuv420p" | "yuv422p" | "yuv444p" | "yuvj420p" | "yuvj422p" | "yuvj444p" | "nv12"
        | "nv21" | "rgb24" | "bgr24" | "rgba" | "bgra" | "gray" => Some(8),
        "yuv420p10le" | "yuv420p10be" | "yuv422p10le" | "yuv422p10be" | "yuv444p10le"
        | "yuv444p10be" | "p010le" | "p010be" | "gbrp10le" | "gbrp10be" => Some(10),
        "yuv420p12le" | "yuv422p12le" | "yuv444p12le" | "gbrp12le" => Some(12),
        _ => None,
    };
    MediaValue::new(depth, "derived_from_pixel_format")
}

fn stream(raw: &Value) -> Option<MediaStream> {
    let index = number(raw, "index")?;
    let tags = tags(raw);
    let bitrate = positive(raw, "bit_rate");
    let tagged_bitrate = tag(&tags, "BPS")
        .and_then(|value| value.parse().ok())
        .filter(|value: &u64| *value > 0);
    let payload = tag(&tags, "NUMBER_OF_BYTES").and_then(|value| value.parse().ok());
    let dispositions = raw
        .get("disposition")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.clone(), value.as_u64().is_some_and(|flag| flag != 0)))
        .collect();
    let side_data = raw
        .get("side_data_list")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut hdr_signals = Vec::new();
    match text(raw, "color_transfer").as_deref() {
        Some("smpte2084") => hdr_signals.push("pq_transfer".into()),
        Some("arib-std-b67") => hdr_signals.push("hlg_transfer".into()),
        _ => (),
    }
    for data in &side_data {
        if let Some(kind) = text(data, "side_data_type") {
            if kind.contains("DOVI") {
                hdr_signals.push("dolby_vision_metadata".into());
            } else if kind.contains("Mastering display") {
                hdr_signals.push("mastering_display_metadata".into());
            } else if kind.contains("Content light") {
                hdr_signals.push("content_light_metadata".into());
            }
        }
    }
    let kind = text(raw, "codec_type").unwrap_or_else(|| "unknown".into());
    let sample_bit_depth = if kind == "audio" {
        MediaValue::new(positive(raw, "bits_per_raw_sample"), "ffprobe_stream")
    } else {
        unknown_value()
    };
    Some(MediaStream {
        index,
        component_bit_depth: if kind == "video" {
            bit_depth(raw)
        } else {
            MediaValue::new(None, "unknown")
        },
        kind,
        codec: text(raw, "codec_name"),
        profile: text(raw, "profile"),
        width: positive(raw, "width"),
        height: positive(raw, "height"),
        pixel_format: text(raw, "pix_fmt"),
        frame_rate: text(raw, "avg_frame_rate").filter(|value| value != "0/0"),
        color_primaries: text(raw, "color_primaries"),
        color_transfer: text(raw, "color_transfer"),
        color_space: text(raw, "color_space"),
        hdr_signals,
        sample_rate_hz: positive(raw, "sample_rate"),
        sample_bit_depth,
        channels: positive(raw, "channels"),
        channel_layout: text(raw, "channel_layout"),
        language: tag(&tags, "language").and_then(language),
        title: tag(&tags, "title").map(str::to_owned),
        bitrate_bps: if bitrate.is_some() {
            MediaValue::new(bitrate, "ffprobe_stream")
        } else {
            MediaValue::new(tagged_bitrate, "container_statistics_tag")
        },
        estimated_bitrate_bps: unknown_value(),
        payload_size_bytes: MediaValue::new(payload, "container_statistics_tag"),
        dispositions,
        side_data,
        tags,
    })
}

fn selection(streams: &[MediaStream], requested: &StreamSelection) -> SelectionReport {
    let videos: Vec<_> = streams
        .iter()
        .filter(|item| item.kind == "video" && item.dispositions.get("attached_pic") != Some(&true))
        .collect();
    let video_selection_unresolved = match requested.video_stream_index {
        Some(index) => !videos.iter().any(|item| item.index == index),
        None => videos.len() > 1,
    };
    let mut selected = Vec::new();
    let mut unclassified = Vec::new();
    let mut omitted = Vec::new();
    for item in streams {
        let wanted = match item.kind.as_str() {
            "audio" => Some(&requested.audio_languages),
            "subtitle" => Some(&requested.subtitle_languages),
            _ => None,
        };
        if item.kind == "video" && item.dispositions.get("attached_pic") != Some(&true) {
            if video_selection_unresolved {
                unclassified.push(item.index);
            } else if requested
                .video_stream_index
                .is_none_or(|index| index == item.index)
            {
                selected.push(item.index);
            } else {
                omitted.push(item.index);
            }
        } else if item.dispositions.get("attached_pic") == Some(&true)
            || !["video", "audio", "subtitle"].contains(&item.kind.as_str())
        {
            unclassified.push(item.index);
        } else if let Some(wanted) = wanted.filter(|wanted| !wanted.is_empty()) {
            match &item.language {
                Some(code)
                    if wanted
                        .iter()
                        .filter_map(|value| language(value))
                        .any(|value| &value == code) =>
                {
                    selected.push(item.index)
                }
                Some(_) => omitted.push(item.index),
                None => unclassified.push(item.index),
            }
        } else {
            selected.push(item.index);
        }
    }
    let missing = |kind: &str, wanted: &[String]| {
        wanted
            .iter()
            .filter(|code| {
                !streams
                    .iter()
                    .any(|item| item.kind == kind && item.language == language(code))
            })
            .cloned()
            .collect()
    };
    let total = |indices: &[u64]| -> Option<u64> {
        indices.iter().try_fold(0u64, |sum, index| {
            sum.checked_add(
                streams
                    .iter()
                    .find(|item| item.index == *index)?
                    .payload_size_bytes
                    .value?,
            )
        })
    };
    let selected_bytes = if unclassified.is_empty() && !video_selection_unresolved {
        total(&selected)
    } else {
        None
    };
    SelectionReport {
        video_selection_unresolved,
        selected_payload_bytes: MediaValue::new(selected_bytes, "sum_of_container_statistics_tags"),
        omitted_payload_bytes: MediaValue::new(total(&omitted), "sum_of_container_statistics_tags"),
        selected_stream_indices: selected,
        unclassified_stream_indices: unclassified,
        omitted_stream_indices: omitted,
        missing_audio_languages: missing("audio", &requested.audio_languages),
        missing_subtitle_languages: missing("subtitle", &requested.subtitle_languages),
        changes_applied: false,
    }
}

fn is_attached_picture(stream: &MediaStream) -> bool {
    stream.dispositions.get("attached_pic") == Some(&true)
}

/// Text subtitles weigh a few hundred bits per second (a cover picture is one
/// image); image subtitles (PGS, VobSub) can reach tens of kilobits and must
/// be known.
fn negligible_without_bitrate(stream: &MediaStream) -> bool {
    stream.kind == "subtitle"
        && matches!(
            stream.codec.as_deref(),
            Some("ass" | "ssa" | "subrip" | "srt" | "mov_text" | "webvtt" | "text")
        )
}

/// The video track's bitrate as the file's total minus everything else, when
/// there is one video track whose own bitrate is unknown and every other
/// track's bitrate (and every attachment's size) is known.
fn estimated_video_bitrate(
    raw: &[Value],
    streams: &[MediaStream],
    overall: Option<u64>,
    duration: Option<f64>,
) -> Option<(usize, u64)> {
    let overall = overall?;
    let duration = duration.filter(|seconds| *seconds > 0.0)?;
    let mut videos = streams
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == "video" && !is_attached_picture(s));
    let (position, video) = videos.next()?;
    if videos.next().is_some() || video.bitrate_bps.value.is_some() {
        return None;
    }
    let mut others = 0u64;
    for (index, stream) in streams.iter().enumerate() {
        if index == position {
            continue;
        }
        let bits = if stream.kind == "attachment" {
            // Fonts in anime releases: ffprobe stores the file as extradata.
            let bytes = raw
                .iter()
                .find(|item| number(item, "index") == Some(stream.index))
                .and_then(|item| positive(item, "extradata_size"))?;
            (bytes as f64 * 8.0 / duration) as u64
        } else if let Some(value) = stream.bitrate_bps.value {
            value
        } else if negligible_without_bitrate(stream) || is_attached_picture(stream) {
            0
        } else {
            return None;
        };
        others = others.checked_add(bits)?;
    }
    let estimate = overall.checked_sub(others).filter(|value| *value > 0)?;
    Some((position, estimate))
}

pub(super) fn build_report(
    raw: Value,
    requested: &StreamSelection,
    size: u64,
    kind: &str,
) -> Option<MediaInspectionReport> {
    let raw_streams = raw.get("streams")?.as_array()?;
    let mut streams: Vec<_> = raw_streams.iter().filter_map(stream).collect();
    if streams.is_empty() {
        return None;
    }
    let format = &raw["format"];
    let duration_seconds = text(format, "duration")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0);
    let overall_bitrate = positive(format, "bit_rate");
    if let Some((position, estimate)) =
        estimated_video_bitrate(raw_streams, &streams, overall_bitrate, duration_seconds)
    {
        streams[position].estimated_bitrate_bps =
            MediaValue::new(Some(estimate), "overall_minus_other_streams");
    }
    let selection = selection(&streams, requested);
    Some(MediaInspectionReport {
        schema_version: 1,
        evidence_source: "ffprobe_media_headers".into(),
        inspection_scope: "bounded_header_probe".into(),
        input_kind: kind.into(),
        format: MediaFormat {
            name: text(format, "format_name"),
            duration_seconds,
            file_size_bytes: size,
            overall_bitrate_bps: MediaValue::new(overall_bitrate, "ffprobe_format"),
        },
        streams,
        selection,
        read_bytes_charged: 0,
        read_requests: 0,
        read_budget_bytes: 0,
        elapsed_ms: 0,
        limitations: vec![
            "full_file_integrity_not_checked".into(),
            "visual_quality_not_measured".into(),
            "metadata_and_language_tags_may_be_missing_or_incorrect".into(),
            "track_sizes_and_bitrates_are_not_full_packet_scan_measurements".into(),
            "selection_does_not_reduce_torrent_download_bytes".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn never_substitutes_total_bitrate_for_unknown_video_bitrate() {
        let report = build_report(json!({"format":{"bit_rate":"50000000"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p10le","color_transfer":"smpte2084"},
            {"index":1,"codec_type":"audio","codec_name":"truehd","bit_rate":"4000000","channels":8,"tags":{"language":"eng"}}
        ]}), &StreamSelection::default(), 1000, "local_file").unwrap();
        assert_eq!(report.format.overall_bitrate_bps.value, Some(50000000));
        assert_eq!(report.streams[0].bitrate_bps.value, None);
        assert_eq!(report.streams[0].bitrate_bps.basis, "unknown");
        assert_eq!(report.streams[0].component_bit_depth.value, Some(10));
        assert_eq!(
            report.streams[0].component_bit_depth.basis,
            "derived_from_pixel_format"
        );
        assert_eq!(report.streams[0].hdr_signals, vec!["pq_transfer"]);
        assert_eq!(report.streams[1].sample_bit_depth.value, None);
    }

    #[test]
    fn estimates_video_bitrate_from_the_total_minus_other_tracks() {
        let report = build_report(json!({"format":{"bit_rate":"20000000","duration":"100"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"hevc"},
            {"index":1,"codec_type":"audio","codec_name":"eac3","bit_rate":"640000"},
            {"index":2,"codec_type":"subtitle","codec_name":"ass"},
            {"index":3,"codec_type":"attachment","codec_name":"ttf","extradata_size":125000},
            {"index":4,"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}}
        ]}), &StreamSelection::default(), 1000, "local_file").unwrap();
        // 20 Mb/s - 640 kb/s audio - 10 kb/s font; the subtitle and cover count as 0.
        assert_eq!(report.streams[0].bitrate_bps.value, None);
        assert_eq!(
            report.streams[0].estimated_bitrate_bps.value,
            Some(19350000)
        );
        assert_eq!(
            report.streams[0].estimated_bitrate_bps.basis,
            "overall_minus_other_streams"
        );
        assert_eq!(report.streams[1].estimated_bitrate_bps.value, None);
    }

    #[test]
    fn does_not_estimate_video_bitrate_when_another_track_is_unknown() {
        let unknown_audio = json!({"format":{"bit_rate":"20000000","duration":"100"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264"},
            {"index":1,"codec_type":"audio","codec_name":"truehd"}
        ]});
        let image_subtitle = json!({"format":{"bit_rate":"20000000","duration":"100"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264"},
            {"index":1,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"}
        ]});
        let measured = json!({"format":{"bit_rate":"20000000","duration":"100"},"streams":[
            {"index":0,"codec_type":"video","codec_name":"h264","bit_rate":"18000000"}
        ]});
        for raw in [unknown_audio, image_subtitle, measured] {
            let report =
                build_report(raw, &StreamSelection::default(), 1000, "local_file").unwrap();
            assert_eq!(report.streams[0].estimated_bitrate_bps.value, None);
        }
    }

    #[test]
    fn reports_lossless_audio_sample_depth() {
        let report = build_report(json!({"format":{},"streams":[
            {"index":0,"codec_type":"audio","codec_name":"flac","sample_rate":"96000","bits_per_raw_sample":"24","channels":2}
        ]}), &StreamSelection::default(), 1000, "local_file").unwrap();
        assert_eq!(report.streams[0].sample_bit_depth.value, Some(24));
        assert_eq!(report.streams[0].sample_rate_hz, Some(96000));
        assert_eq!(report.streams[0].component_bit_depth.value, None);
    }

    #[test]
    fn separates_requested_languages_and_reports_tag_based_payload_sizes() {
        let selection = StreamSelection {
            audio_languages: vec!["zh".into()],
            subtitle_languages: vec!["ara".into()],
            ..Default::default()
        };
        let report = build_report(json!({"streams":[
            {"index":0,"codec_type":"video","tags":{"NUMBER_OF_BYTES":"1000"}},
            {"index":1,"codec_type":"audio","tags":{"LANGUAGE":"chi","NUMBER_OF_BYTES":"200","BPS":"100"}},
            {"index":2,"codec_type":"audio","tags":{"language":"rus","NUMBER_OF_BYTES":"300"}},
            {"index":3,"codec_type":"subtitle","tags":{"language":"ara","NUMBER_OF_BYTES":"10"},"disposition":{"forced":1}}
        ]}), &selection, 2000, "local_file").unwrap();
        assert_eq!(report.selection.selected_stream_indices, vec![0, 1, 3]);
        assert_eq!(report.selection.omitted_stream_indices, vec![2]);
        assert_eq!(report.selection.selected_payload_bytes.value, Some(1210));
        assert_eq!(report.selection.omitted_payload_bytes.value, Some(300));
        assert_eq!(
            report.streams[1].bitrate_bps.basis,
            "container_statistics_tag"
        );
        assert_eq!(report.streams[3].dispositions.get("forced"), Some(&true));
        assert!(!report.selection.changes_applied);
    }

    #[test]
    fn multiple_video_tracks_require_selection_and_missing_tags_remain_unclassified() {
        let raw = json!({"streams":[{"index":0,"codec_type":"video"},{"index":1,"codec_type":"video"},
            {"index":2,"codec_type":"audio"},{"index":3,"codec_type":"subtitle","tags":{"language":"und"}}]});
        let mut selection = StreamSelection {
            audio_languages: vec!["en".into()],
            subtitle_languages: vec!["ar".into()],
            ..Default::default()
        };
        let report = build_report(raw.clone(), &selection, 1000, "download_service").unwrap();
        assert!(report.selection.video_selection_unresolved);
        assert_eq!(
            report.selection.unclassified_stream_indices,
            vec![0, 1, 2, 3]
        );
        assert!(report.selection.selected_payload_bytes.value.is_none());
        selection.video_stream_index = Some(1);
        let report = build_report(raw, &selection, 1000, "download_service").unwrap();
        assert!(!report.selection.video_selection_unresolved);
        assert_eq!(report.selection.selected_stream_indices, vec![1]);
        assert_eq!(report.selection.omitted_stream_indices, vec![0]);
        assert_eq!(report.selection.missing_subtitle_languages, vec!["ar"]);
    }
}
