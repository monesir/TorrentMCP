use super::*;
use crate::media::MediaStream;
use std::cmp::Ordering;

fn language_preference(
    report: Option<&TorrentOperationReport>,
    kind: &str,
    preferred: &[String],
    complete: bool,
) -> (Option<u64>, Option<String>) {
    let streams: Vec<_> = report
        .and_then(|r| r.media.as_ref())
        .map(|m| m.streams.iter().filter(|s| s.kind == kind).collect())
        .unwrap_or_default();
    for (i, expected) in preferred.iter().enumerate() {
        if streams.iter().any(|s| {
            s.language.as_deref().and_then(canonical_language) == canonical_language(expected)
        }) {
            return (Some(i as u64), Some(expected.clone()));
        }
    }
    if complete
        && streams
            .iter()
            .all(|s: &&MediaStream| s.language.as_deref().and_then(canonical_language).is_some())
    {
        (Some(preferred.len() as u64), None)
    } else {
        (None, None)
    }
}

pub(crate) fn factors(
    report: Option<&TorrentOperationReport>,
    video: Option<&MediaStream>,
    complete: bool,
    preferences: &MediaPreferences,
) -> Vec<MediaPreferenceFactor> {
    preferences
        .effective_order()
        .into_iter()
        .map(|criterion| {
            let (direction, value, matched_preference) = match criterion {
                MediaRankCriterion::VideoCodecs => {
                    let codec = video.and_then(codec);
                    let index =
                        codec.and_then(|v| preferences.video_codecs.iter().position(|p| p == v));
                    (
                        "preference_order",
                        codec.map(|_| index.unwrap_or(preferences.video_codecs.len()) as u64),
                        index.map(|i| preferences.video_codecs[i].clone()),
                    )
                }
                MediaRankCriterion::AudioLanguages | MediaRankCriterion::SubtitleLanguages => {
                    let (kind, languages) = if criterion == MediaRankCriterion::AudioLanguages {
                        ("audio", &preferences.audio_languages)
                    } else {
                        ("subtitle", &preferences.subtitle_languages)
                    };
                    let (value, matched) = language_preference(report, kind, languages, complete);
                    ("preference_order", value, matched)
                }
                MediaRankCriterion::SmallerFile | MediaRankCriterion::LargerFile => (
                    if criterion == MediaRankCriterion::SmallerFile {
                        "ascending"
                    } else {
                        "descending"
                    },
                    report.and_then(|r| r.file.as_ref()).map(|f| f.size_bytes),
                    None,
                ),
                MediaRankCriterion::HigherVideoBitrate | MediaRankCriterion::LowerVideoBitrate => (
                    if criterion == MediaRankCriterion::LowerVideoBitrate {
                        "ascending"
                    } else {
                        "descending"
                    },
                    video.and_then(bitrate),
                    None,
                ),
                MediaRankCriterion::HigherVideoPixels => (
                    "descending",
                    video.and_then(|s| {
                        s.width
                            .filter(|v| *v > 0)
                            .zip(s.height.filter(|v| *v > 0))
                            .and_then(|(w, h)| w.checked_mul(h))
                    }),
                    None,
                ),
            };
            let reason_code = if value.is_none() {
                "preference_value_unavailable"
            } else if direction == "preference_order" && matched_preference.is_none() {
                "no_listed_preference_present"
            } else {
                "explicit_preference_applied"
            };
            MediaPreferenceFactor {
                criterion,
                direction: direction.into(),
                value,
                matched_preference,
                reason_code: reason_code.into(),
            }
        })
        .collect()
}

pub(crate) fn compare(a: &ComparedMediaCandidate, b: &ComparedMediaCandidate) -> Ordering {
    let class = |eligibility| match eligibility {
        Eligibility::Eligible => 0,
        Eligibility::Uncertain => 1,
        Eligibility::Excluded => 2,
    };
    let first = class(a.assessment.eligibility).cmp(&class(b.assessment.eligibility));
    if first != Ordering::Equal {
        return first;
    }
    for (a, b) in a
        .assessment
        .preference_factors
        .iter()
        .zip(&b.assessment.preference_factors)
    {
        // Unknown values always follow known values, irrespective of ascending/descending order.
        let order = match (a.value, b.value) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        let order = if a.direction == "descending" {
            order.reverse()
        } else {
            order
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.candidate.id.cmp(&b.candidate.id)
}
