use super::{
    title::{canonical_words, normalize_title, title_prefix_end},
    *,
};
use regex::Regex;
use std::sync::LazyLock;

static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:18[89]\d|19\d{2}|20\d{2}|2100)\b").unwrap());
static SEASON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:s|season[ ._-]*)(\d{1,3})(?:\b|e)").unwrap());
static SEASON_EPISODES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bs(\d{1,3})((?:e\d{1,4})+(?:-e?\d{1,4})?)\b").unwrap());
static EPISODE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)e(\d{1,4})").unwrap());
static EPISODES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:episodes?|ep|e)[ ._-]*(\d{1,4})(?:\s*[-–]\s*(?:ep|e)?(\d{1,4}))?\b")
        .unwrap()
});
static X_EPISODES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(\d{1,2})x(\d{2})(?:-(\d{2}))?\b").unwrap());
static PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:part|pt)[ ._-]*(\d{1,2}|iii|ii|iv|vi|vii|viii|ix|x|v)\b").unwrap()
});
static RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(480|576|720|1080|1440|2160|4320)[pi]|(4k|8k))\b").unwrap()
});
static CODEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:x264|h[ ._-]?264|avc|x265|h[ ._-]?265|hevc|av1|mpeg[ ._-]?2|xvid)\b")
        .unwrap()
});
static AUDIO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(chinese|mandarin|english|arabic|japanese|korean|french|german|spanish|russian|hindi|thai)[ ._-]+(?:audio|dub|dubbed)\b").unwrap()
});
static SUBTITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(chinese|mandarin|english|eng|arabic|japanese|korean|french|german|spanish|russian|hindi|thai)[ ._-]*(?:sub|subs|subtitles)\b").unwrap()
});
static FORMAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:web[ ._-]?dl|web[ ._-]?rip|blu[ ._-]?ray|bdrip|hdtv|dvdrip|remux)\b")
        .unwrap()
});
static COMPLETE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:complete|full[ ._-]+season)\b").unwrap());

fn part_number(token: &str) -> Option<u16> {
    token
        .parse()
        .ok()
        .or_else(|| match token.to_ascii_lowercase().as_str() {
            "ii" => Some(2),
            "iii" => Some(3),
            "iv" => Some(4),
            "v" => Some(5),
            "vi" => Some(6),
            "vii" => Some(7),
            "viii" => Some(8),
            "ix" => Some(9),
            "x" => Some(10),
            _ => None,
        })
        .filter(|number| (1..=99).contains(number))
}

fn language(token: &str) -> String {
    match token.to_ascii_lowercase().as_str() {
        "chinese" | "mandarin" => "zh",
        "english" | "eng" => "en",
        "arabic" => "ar",
        "japanese" => "ja",
        "korean" => "ko",
        "french" => "fr",
        "german" => "de",
        "spanish" => "es",
        "russian" => "ru",
        "hindi" => "hi",
        "thai" => "th",
        _ => unreachable!(),
    }
    .into()
}

pub fn analyze_release(result: &SearchResult, request: &AnalysisRequest) -> ReleaseAnalysis {
    let mut analysis = ReleaseAnalysis::default();
    let titles: Vec<_> = if result.observations.is_empty() {
        vec![("unattributed", result.title.as_str())]
    } else {
        result
            .observations
            .iter()
            .map(|origin| {
                (
                    origin.source_id.as_str(),
                    origin.raw_title.as_deref().unwrap_or(&origin.title),
                )
            })
            .collect()
    };
    for (title_index, (source_id, raw)) in titles.into_iter().enumerate() {
        let title = normalize_title(raw, source_id);
        let end = title_prefix_end(&title.normalized, &request.target);
        // A numeric series title (e.g. 1923) must not become its release year.
        let metadata = &title.normalized[end.unwrap_or(0)..];
        let evidence = |token: &str, rule: &str| ExtractionEvidence {
            source_id: source_id.into(),
            title_index,
            token: token.into(),
            rule: rule.into(),
        };
        for found in YEAR.find_iter(metadata) {
            analysis.year.push(
                found.as_str().parse().unwrap(),
                evidence(found.as_str(), "year_label"),
                false,
            );
        }
        for found in SEASON.captures_iter(metadata) {
            if let Ok(value) = found[1].parse() {
                analysis
                    .season
                    .push(value, evidence(&found[0], "season_label"), false);
            }
        }
        let mut title_ranges = Vec::new();
        let mut range_evidence = Vec::new();
        let mut record_range = |start: &str, end: Option<&str>, token: &str, rule: &str| {
            let start: u16 = start.parse().unwrap_or(0);
            let end = end.and_then(|value| value.parse().ok()).unwrap_or(start);
            if start == 0 || start > end || end > 9999 {
                analysis.issues.push("invalid_episode_range".into());
                return;
            }
            let range = EpisodeRange { start, end };
            if !title_ranges.contains(&range) {
                title_ranges.push(range);
            }
            range_evidence.push(evidence(token, rule));
        };
        for found in SEASON_EPISODES.captures_iter(metadata) {
            let (listed, range_end) = found[2]
                .split_once('-')
                .map_or((found[2].as_ref(), None), |(listed, end)| {
                    (listed, Some(end.trim_start_matches(['e', 'E'])))
                });
            let numbers: Vec<_> = EPISODE_NUMBER.captures_iter(listed).collect();
            for (index, number) in numbers.iter().enumerate() {
                record_range(
                    &number[1],
                    if index + 1 == numbers.len() {
                        range_end
                    } else {
                        None
                    },
                    &found[0],
                    "season_episode_label",
                );
            }
        }
        for found in EPISODES.captures_iter(metadata) {
            record_range(
                &found[1],
                found.get(2).map(|value| value.as_str()),
                &found[0],
                "episode_label",
            );
        }
        for found in X_EPISODES.captures_iter(metadata) {
            analysis.season.push(
                found[1].parse().unwrap(),
                evidence(&found[0], "season_x_episode_label"),
                false,
            );
            record_range(
                &found[2],
                found.get(3).map(|value| value.as_str()),
                &found[0],
                "season_x_episode_label",
            );
        }
        for found in PART.captures_iter(metadata) {
            if let Some(value) = part_number(&found[1]) {
                analysis
                    .part
                    .push(value, evidence(&found[0], "explicit_part_label"), false);
            }
        }
        if end.is_some() {
            if let Some(token) = canonical_words(metadata).first() {
                if let Some(value) = part_number(token) {
                    analysis.part.push(
                        value,
                        evidence(token, "part_suffix_after_requested_title"),
                        true,
                    );
                }
            }
        }
        for found in RESOLUTION.captures_iter(metadata) {
            let height = found
                .get(1)
                .map(|value| value.as_str().parse().unwrap())
                .unwrap_or_else(|| {
                    if found[2].eq_ignore_ascii_case("4k") {
                        2160
                    } else {
                        4320
                    }
                });
            analysis
                .resolution
                .push(height, evidence(&found[0], "resolution_label"), false);
        }
        for found in CODEC.find_iter(metadata) {
            let key = canonical_words(found.as_str()).join("");
            let value = match key.as_str() {
                "x264" | "h264" | "avc" => "h264",
                "x265" | "h265" | "hevc" => "hevc",
                "av1" => "av1",
                "mpeg2" => "mpeg2",
                _ => "xvid",
            };
            analysis.video_codec.push(
                value.into(),
                evidence(found.as_str(), "video_codec_label"),
                false,
            );
        }
        for found in AUDIO.captures_iter(metadata) {
            analysis.audio_language.push(
                language(&found[1]),
                evidence(&found[0], "explicit_audio_language_label"),
                false,
            );
        }
        for found in SUBTITLE.captures_iter(metadata) {
            analysis.subtitle_language.push(
                language(&found[1]),
                evidence(&found[0], "explicit_subtitle_language_label"),
                false,
            );
        }
        for found in FORMAT.find_iter(metadata) {
            analysis.source_format.push(
                canonical_words(found.as_str()).join(""),
                evidence(found.as_str(), "source_format_label"),
                false,
            );
        }
        if !title_ranges.is_empty() {
            title_ranges.sort_by_key(|range| (range.start, range.end));
            let mut merged: Vec<EpisodeRange> = Vec::new();
            for range in title_ranges {
                if let Some(previous) = merged.last_mut()
                    && range.start <= previous.end.saturating_add(1)
                {
                    previous.end = previous.end.max(range.end);
                } else {
                    merged.push(range);
                }
            }
            let title_ranges = merged;
            for found in range_evidence {
                analysis.episodes.push(title_ranges.clone(), found, false);
            }
            let coverage =
                if title_ranges.len() == 1 && title_ranges[0].start == title_ranges[0].end {
                    CoverageKind::SingleEpisode
                } else {
                    CoverageKind::EpisodeBatch
                };
            analysis.coverage.push(
                coverage,
                evidence(metadata, "coverage_from_episode_labels"),
                true,
            );
        } else if let Some(found) = COMPLETE.find(metadata) {
            analysis.coverage.push(
                CoverageKind::CompleteClaim,
                evidence(found.as_str(), "complete_is_only_a_title_claim"),
                false,
            );
        } else if let Some(found) = SEASON.find(metadata) {
            analysis.coverage.push(
                CoverageKind::SeasonPack,
                evidence(found.as_str(), "season_without_episode_label"),
                true,
            );
        }
        analysis.titles.push(title);
    }
    analysis.issues.sort();
    analysis.issues.dedup();
    analysis
}
