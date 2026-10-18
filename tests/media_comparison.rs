use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use torrent_search::{agent::AgentStatus, comparison::*};

fn candidate(id: &str) -> InspectedMediaCandidate {
    let mut inspection: SavedTorrentInspection =
        serde_json::from_str(include_str!("fixtures/inspected-torrent.json")).unwrap();
    let hash = format!("{:x}", Sha256::digest(id.as_bytes()))[..40].to_owned();
    let report = inspection.result.as_mut().unwrap();
    let torrent = report.torrent.as_mut().unwrap();
    torrent.info_hash_v1 = hash.clone();
    torrent.source_reference = torrent_search::TorrentReference::Magnet {
        uri: format!("magnet:?xt=urn:btih:{hash}"),
        info_hash_v1: Some(hash),
    };
    torrent.resolved_reference = torrent.source_reference.clone();
    InspectedMediaCandidate {
        id: id.into(),
        inspection,
        video_stream_index: None,
    }
}
fn size(candidate: &mut InspectedMediaCandidate, bytes: u64) {
    let r = candidate.inspection.result.as_mut().unwrap();
    let id = r.file.as_ref().unwrap().id;
    r.torrent.as_mut().unwrap().files[id].size_bytes = bytes;
    r.file.as_mut().unwrap().size_bytes = bytes;
    r.media.as_mut().unwrap().format.file_size_bytes = bytes;
}
fn analysis() -> MediaAnalysisRequest {
    MediaAnalysisRequest {
        constraints: MediaConstraints {
            min_video_width: Some(160),
            min_video_height: Some(90),
            audio_languages: vec!["en".into()],
            subtitle_languages: vec!["ar".into()],
            max_file_size_bytes: Some(1024 * 1024),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn remove_arabic(candidate: &mut InspectedMediaCandidate, unknown: bool) {
    for stream in &mut candidate
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams
    {
        if stream.kind == "subtitle" && stream.language.as_deref() == Some("ar") {
            stream.language = if unknown { None } else { Some("fr".into()) };
        }
    }
}

#[test]
fn measured_properties_produce_all_three_decisions_with_evidence() {
    let good = candidate("good");
    let mut missing = candidate("missing");
    remove_arabic(&mut missing, false);
    let mut unknown = candidate("unknown");
    remove_arabic(&mut unknown, true);
    let mut large = candidate("large");
    size(&mut large, 2 * 1024 * 1024);
    let report = compare_media(vec![large, unknown, missing, good], analysis()).unwrap();
    assert_eq!(report.summary.eligible, 1);
    assert_eq!(report.summary.uncertain, 1);
    assert_eq!(report.summary.excluded, 2);
    assert!(
        !report.partial,
        "Unknown language does not mean the inspection request failed"
    );
    assert_eq!(report.results[0].candidate.id, "good");
    assert_eq!(report.results[1].candidate.id, "unknown");
    let failure = report
        .results
        .iter()
        .find(|r| r.candidate.id == "missing")
        .unwrap();
    let language = failure
        .assessment
        .evaluations
        .iter()
        .find(|e| e.criterion.field == "subtitle_languages")
        .unwrap();
    assert_eq!(language.criterion.reason_code, "required_language_absent");
    assert_eq!(language.evidence_source, "container_language_tags");
    assert!(
        report.results[0]
            .assessment
            .evaluations
            .iter()
            .any(|e| e.stream_indices == vec![0])
    );
}

#[test]
fn all_requested_languages_are_required_and_empty_subtitles_are_conclusive_only_when_complete() {
    let mut request = analysis();
    request.constraints.subtitle_languages = vec!["en".into(), "ar".into()];
    let mut missing = candidate("missing");
    remove_arabic(&mut missing, false);
    assert_eq!(
        compare_media(vec![missing], request.clone())
            .unwrap()
            .summary
            .excluded,
        1
    );
    let mut empty = candidate("empty");
    let m = empty
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap();
    m.streams.retain(|s| s.kind != "subtitle");
    m.selection.selected_stream_indices.retain(|i| *i < 3);
    assert_eq!(
        compare_media(vec![empty.clone()], request.clone())
            .unwrap()
            .summary
            .excluded,
        1
    );
    empty.inspection.status = AgentStatus::Partial;
    let report = compare_media(vec![empty], request).unwrap();
    assert_eq!(report.summary.uncertain, 1);
    assert!(report.partial);
}

#[test]
fn uses_video_bitrate_only_and_never_total_file_bitrate() {
    let mut c = candidate("missing-bitrate");
    let media = c
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap();
    media.format.overall_bitrate_bps.value = Some(100_000_000);
    media.streams[0].bitrate_bps.value = None;
    let mut request = analysis();
    request.constraints.min_video_bitrate_bps = Some(1_000_000);
    let report = compare_media(vec![c.clone()], request.clone()).unwrap();
    assert_eq!(report.summary.uncertain, 1);
    let media = c
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap();
    media.streams[0].bitrate_bps.value = Some(2_000_000);
    media.streams[0].bitrate_bps.basis = "container_statistics_tag".into();
    assert_eq!(
        compare_media(vec![c.clone()], request.clone())
            .unwrap()
            .summary
            .eligible,
        1
    );
    c.inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams[0]
        .bitrate_bps
        .basis = "ffprobe_format".into();
    assert_eq!(
        compare_media(vec![c], request).unwrap().summary.uncertain,
        1
    );
}

#[test]
fn explicit_ranking_priorities_are_lexicographic_and_unknown_values_follow_known() {
    let mut small = candidate("small-h264");
    size(&mut small, 200_000);
    let mut hevc = candidate("large-hevc");
    size(&mut hevc, 500_000);
    hevc.inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams[0]
        .codec = Some("hevc".into());
    let mut request = analysis();
    request.preferences.video_codecs = vec!["hevc".into(), "h264".into()];
    request.preferences.rank_by = vec![
        MediaRankCriterion::VideoCodecs,
        MediaRankCriterion::SmallerFile,
    ];
    let ranked = compare_media(vec![small.clone(), hevc.clone()], request.clone()).unwrap();
    assert_eq!(ranked.results[0].candidate.id, "large-hevc");
    assert_eq!(
        ranked.results[0].assessment.preference_factors[0]
            .matched_preference
            .as_deref(),
        Some("hevc")
    );
    request.preferences.rank_by.reverse();
    assert_eq!(
        compare_media(vec![small.clone(), hevc.clone()], request)
            .unwrap()
            .results[0]
            .candidate
            .id,
        "small-h264"
    );
    let mut request = analysis();
    request.preferences.rank_by = vec![MediaRankCriterion::HigherVideoBitrate];
    let stream = &mut hevc
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams[0];
    stream.bitrate_bps.value = Some(1_000_000);
    stream.bitrate_bps.basis = "ffprobe_stream".into();
    assert_eq!(
        compare_media(vec![small, hevc], request).unwrap().results[0]
            .candidate
            .id,
        "large-hevc"
    );
}

#[test]
fn multiple_video_streams_require_selection_and_attached_pictures_do_not_count() {
    let mut c = candidate("multi");
    let media = c
        .inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap();
    let mut other = media.streams[0].clone();
    other.index = 10;
    other.width = Some(80);
    other.height = Some(45);
    media.streams.push(other);
    media.selection.video_selection_unresolved = true;
    assert_eq!(
        compare_media(vec![c.clone()], analysis())
            .unwrap()
            .summary
            .uncertain,
        1
    );
    c.video_stream_index = Some(0);
    assert_eq!(
        compare_media(vec![c.clone()], analysis())
            .unwrap()
            .summary
            .eligible,
        1
    );
    c.video_stream_index = Some(10);
    assert_eq!(
        compare_media(vec![c.clone()], analysis())
            .unwrap()
            .summary
            .excluded,
        1
    );
    c.video_stream_index = Some(99);
    assert!(compare_media(vec![c.clone()], analysis()).is_err());
    c.video_stream_index = None;
    c.inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams
        .last_mut()
        .unwrap()
        .dispositions
        .insert("attached_pic".into(), true);
    assert_eq!(
        compare_media(vec![c], analysis()).unwrap().summary.eligible,
        1
    );
}

#[test]
fn rejects_mismatched_identity_corrupt_stream_indices_and_duplicate_candidates() {
    let mut c = candidate("a");
    c.inspection
        .result
        .as_mut()
        .unwrap()
        .file
        .as_mut()
        .unwrap()
        .size_bytes += 1;
    assert!(compare_media(vec![c], analysis()).is_err());
    let mut c = candidate("a");
    c.inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams[1]
        .index = 0;
    assert!(compare_media(vec![c], analysis()).is_err());
    let a = candidate("a");
    let mut duplicate = a.clone();
    duplicate.id = "b".into();
    assert!(compare_media(vec![a.clone(), duplicate], analysis()).is_err());
    assert!(compare_media(vec![a.clone(), a], analysis()).is_err());
}

#[test]
fn failed_and_partial_inspections_cannot_become_eligible_and_definite_violations_still_exclude() {
    let mut failed = candidate("failed");
    failed.inspection.result = None;
    failed.inspection.status = AgentStatus::Failed;
    let report = compare_media(vec![failed], MediaAnalysisRequest::default()).unwrap();
    assert!(report.partial);
    assert_eq!(report.summary.uncertain, 1);
    let mut partial = candidate("partial");
    partial.inspection.status = AgentStatus::Partial;
    size(&mut partial, 2 * 1024 * 1024);
    assert_eq!(
        compare_media(vec![partial], analysis())
            .unwrap()
            .summary
            .excluded,
        1
    );
    let mut c = candidate("shutdown");
    c.inspection.result.as_mut().unwrap().service.shutdown_error = Some("locked".into());
    assert_eq!(
        compare_media(vec![c], analysis())
            .unwrap()
            .summary
            .uncertain,
        1
    );
    // A daemon's shared session keeps running after the request; that alone
    // does not make the inspection incomplete.
    let mut c = candidate("shared");
    c.inspection.result.as_mut().unwrap().service.stopped = false;
    assert_eq!(
        compare_media(vec![c], analysis())
            .unwrap()
            .summary
            .uncertain,
        0
    );
}

#[test]
fn validates_bounds_languages_and_explicit_preference_configuration() {
    for value in [
        json!({"constraints":{"min_file_size_bytes":2,"max_file_size_bytes":1}}),
        json!({"constraints":{"min_video_width":0}}),
        json!({"constraints":{"audio_languages":["und"]}}),
        json!({"constraints":{"audio_languages":["eng","en"]}}),
        json!({"preferences":{"rank_by":["smaller_file","larger_file"]}}),
        json!({"preferences":{"rank_by":["video_codecs"]}}),
    ] {
        let request: MediaAnalysisRequest = serde_json::from_value(value).unwrap();
        assert!(request.validate().is_err());
    }
    assert!(
        serde_json::from_value::<MediaAnalysisRequest>(json!({"constraints":{"max_sze":42}}))
            .is_err()
    );
}

fn cli(input: &Value) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_torrent-search"))
        .args([
            "--assets",
            "missing",
            "--python",
            "missing",
            "--rqbit",
            "missing",
            "--ffprobe",
            "missing",
            "agent",
        ])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn comparison_cli_runs_without_any_runtime_and_preserves_input_evidence() {
    let c = candidate("a");
    let request = json!({"schema_version":1,"operation":"compare_media","analysis":analysis(),"candidates":[c]});
    let (exit, report) = cli(&request);
    assert_eq!(exit, 0);
    assert_eq!(report["result"]["summary"]["eligible"], 1);
    assert_eq!(
        report["result"]["results"][0]["inspection"],
        request["candidates"][0]["inspection"]
    );
    let mut partial = request;
    partial["candidates"][0]["inspection"]["status"] = json!("partial");
    let (exit, report) = cli(&partial);
    assert_eq!(exit, 3);
    assert_eq!(report["result"]["summary"]["uncertain"], 1);
}

#[test]
fn aliases_empty_codec_and_stale_assessments_do_not_invent_matches() {
    let mut c = candidate("a");
    let mut request = analysis();
    request.constraints.audio_languages = vec!["eng".into()];
    request.constraints.subtitle_languages = vec!["ara".into()];
    let first = compare_media(vec![c.clone()], request.clone()).unwrap();
    assert_eq!(first.summary.eligible, 1);
    c.inspection.result.as_mut().unwrap().assessment = Some(first.results[0].assessment.clone());
    // A previously eligible decision is not reused for changed constraints.
    request.constraints.min_video_width = Some(1920);
    let reference = &mut c
        .inspection
        .result
        .as_mut()
        .unwrap()
        .torrent
        .as_mut()
        .unwrap()
        .source_reference;
    if let torrent_search::TorrentReference::Magnet { uri, .. } = reference {
        uri.push_str("&dn=2160p.AV1.Arabic");
    }
    assert_eq!(
        compare_media(vec![c.clone()], request.clone())
            .unwrap()
            .summary
            .excluded,
        1
    );
    request.constraints.min_video_width = None;
    request.constraints.video_codecs = vec!["hevc".into()];
    c.inspection
        .result
        .as_mut()
        .unwrap()
        .media
        .as_mut()
        .unwrap()
        .streams[0]
        .codec = Some(String::new());
    assert_eq!(
        compare_media(vec![c], request).unwrap().summary.uncertain,
        1
    );
}
