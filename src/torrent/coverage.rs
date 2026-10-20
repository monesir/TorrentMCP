//! What a torrent's file list proves about its episodes, and whether the
//! files `find` sampled from one torrent carry the same streams. Release names
//! only claim a full season; the catalog shows which episodes are there.

use super::{TorrentCatalog, workflow::main_videos};
use crate::analysis::{EpisodeRange, SearchTarget};
use crate::media::MediaInspectionReport;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpisodeCoverage {
    /// Main video files: episodes, without samples and extras.
    pub main_files: usize,
    /// Episodes found in file names, per season; `season` is absent when the
    /// paths name none.
    pub seasons: Vec<SeasonCoverage>,
    /// Main files whose names carry no episode number.
    pub unnumbered: usize,
    /// Requested episodes (`analysis.target.episodes`) with no file. Absent
    /// when no range was requested; empty when every one is there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<Vec<EpisodeRange>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeasonCoverage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<u16>,
    pub episodes: Vec<EpisodeRange>,
    /// Holes between the first and last episode found.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<EpisodeRange>,
    /// Episodes present in more than one file (versions, or a split episode).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub duplicates: Vec<u16>,
}

static SEASON_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bs(\d{1,2})[ ._-]?e(\d{1,4})(?:[ ._-]?-?[ ._-]?e(\d{1,4}))?").unwrap()
});
static X_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(\d{1,2})x(\d{2,3})\b").unwrap());
static EPISODE_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:episode|ep|e)[ ._-]*(\d{1,4})\b").unwrap());
// Fansub style: "Title - 05 (1080p)", "Title - 05v2 [CRC]", "Title_-_05_".
static DASH_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ _])-[ _]+(\d{1,4})(?:v\d)?(?:[ _.\[\(]|$)").unwrap());
static CJK_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"第\s*(\d{1,4})\s*[話话集回]").unwrap());
static SEASON_IN_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:season|s)[ ._-]?(\d{1,2})\b").unwrap());
// Bracketed checksums change from file to file and are not episode numbers.
static CHECKSUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\[\(][0-9A-Fa-f]{8}[\]\)]").unwrap());
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

struct Numbered {
    season: Option<u16>,
    episodes: Option<(u16, u16)>,
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn number(text: &str) -> Option<u16> {
    text.parse().ok().filter(|n| *n > 0)
}

fn parse(path: &str) -> Numbered {
    let name = stem(path);
    let mut season = None;
    let mut episodes = None;
    if let Some(found) = SEASON_EPISODE.captures(name) {
        // S00 holds specials.
        season = found[1].parse().ok();
        if let Some(first) = number(&found[2]) {
            let last = found
                .get(3)
                .and_then(|m| number(m.as_str()))
                .unwrap_or(first);
            episodes = Some((first, last.max(first)));
        }
    } else if let Some(found) = X_EPISODE.captures(name) {
        season = number(&found[1]);
        episodes = number(&found[2]).map(|n| (n, n));
    } else if let Some(found) = EPISODE_LABEL
        .captures(name)
        .or_else(|| DASH_NUMBER.captures(name))
        .or_else(|| CJK_EPISODE.captures(name))
    {
        episodes = number(&found[1]).map(|n| (n, n));
    }
    if season.is_none() {
        // A season folder: "Show/Season 2/05.mkv".
        season = SEASON_IN_PATH
            .captures_iter(path)
            .last()
            .and_then(|found| number(&found[1]));
    }
    Numbered { season, episodes }
}

/// For names with no recognizable label ("Show.05.mkv", "[Group] Show 05"),
/// the episode is the one number that changes from file to file.
fn varying_numbers(names: &[&str]) -> Option<Vec<u16>> {
    if names.len() < 2 {
        return None;
    }
    let tokens: Vec<Vec<String>> = names
        .iter()
        .map(|name| {
            NUMBER
                .find_iter(&CHECKSUM.replace_all(stem(name), ""))
                .map(|m| m.as_str().to_owned())
                .collect()
        })
        .collect();
    let width = tokens[0].len();
    if width == 0 || tokens.iter().any(|t| t.len() != width) {
        return None;
    }
    let varying: Vec<usize> = (0..width)
        .filter(|&i| tokens.iter().any(|t| t[i] != tokens[0][i]))
        .collect();
    let [index] = varying[..] else {
        return None;
    };
    tokens.iter().map(|t| number(&t[index])).collect()
}

fn merge(mut numbers: Vec<u16>) -> Vec<EpisodeRange> {
    numbers.sort_unstable();
    numbers.dedup();
    let mut ranges: Vec<EpisodeRange> = Vec::new();
    for n in numbers {
        match ranges.last_mut() {
            Some(last) if n == last.end + 1 => last.end = n,
            _ => ranges.push(EpisodeRange { start: n, end: n }),
        }
    }
    ranges
}

fn holes(ranges: &[EpisodeRange]) -> Vec<EpisodeRange> {
    ranges
        .windows(2)
        .map(|pair| EpisodeRange {
            start: pair[0].end + 1,
            end: pair[1].start - 1,
        })
        .collect()
}

/// Which episodes the catalog's main video files hold, and which requested
/// ones are missing. None when the files carry no episode numbers and no
/// season or episodes were requested (a film, an album).
pub fn episode_coverage(
    catalog: &TorrentCatalog,
    target: &SearchTarget,
) -> Option<EpisodeCoverage> {
    let files = main_videos(catalog);
    // Audio-only torrents number tracks, not episodes.
    if files.first().is_some_and(|f| f.media_kind != "video") {
        return None;
    }
    let mut parsed: Vec<Numbered> = files.iter().map(|f| parse(&f.path)).collect();
    let unlabeled: Vec<usize> = (0..parsed.len())
        .filter(|&i| parsed[i].episodes.is_none())
        .collect();
    let names: Vec<&str> = unlabeled.iter().map(|&i| files[i].path.as_str()).collect();
    if let Some(numbers) = varying_numbers(&names) {
        for (&i, n) in unlabeled.iter().zip(numbers) {
            parsed[i].episodes = Some((n, n));
        }
    }
    let mut by_season: BTreeMap<Option<u16>, Vec<u16>> = BTreeMap::new();
    let mut unnumbered = 0;
    for file in &parsed {
        match file.episodes {
            Some((first, last)) => by_season
                .entry(file.season)
                .or_default()
                .extend(first..=last.min(first.saturating_add(500))),
            None => unnumbered += 1,
        }
    }
    if by_season.is_empty() && target.season.is_none() && target.episodes.is_none() {
        return None;
    }
    let missing = target.episodes.map(|wanted| {
        let have: Vec<u16> = by_season
            .iter()
            .filter(|(season, _)| {
                target.season.is_none() || season.is_none() || **season == target.season
            })
            .flat_map(|(_, numbers)| numbers.iter().copied())
            .collect();
        merge(
            (wanted.start..=wanted.end)
                .filter(|n| !have.contains(n))
                .collect(),
        )
    });
    let seasons = by_season
        .into_iter()
        .map(|(season, numbers)| {
            let mut seen = std::collections::HashSet::new();
            let mut duplicates: Vec<u16> = numbers
                .iter()
                .copied()
                .filter(|n| !seen.insert(*n))
                .collect();
            duplicates.sort_unstable();
            duplicates.dedup();
            let episodes = merge(numbers);
            SeasonCoverage {
                season,
                gaps: holes(&episodes),
                episodes,
                duplicates,
            }
        })
        .collect();
    Some(EpisodeCoverage {
        main_files: files.len(),
        seasons,
        unnumbered,
        missing,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDifference {
    /// The sampled file that differs from the compared one.
    pub file: String,
    /// video, audio_languages, audio_codecs or subtitle_languages.
    pub field: String,
    pub compared: String,
    pub this_file: String,
}

fn signature(media: &MediaInspectionReport) -> [(&'static str, String); 4] {
    let of = |kind: &'static str| media.streams.iter().filter(move |s| s.kind == kind);
    let list = |mut values: Vec<String>| {
        values.sort();
        values.dedup();
        if values.is_empty() {
            "none".to_owned()
        } else {
            values.join(",")
        }
    };
    let video = of("video")
        .next()
        .map(|s| {
            format!(
                "{} {}x{}",
                s.codec.as_deref().unwrap_or("unknown"),
                s.width.unwrap_or(0),
                s.height.unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "none".into());
    let language = |s: &crate::media::MediaStream| s.language.clone().unwrap_or("und".into());
    [
        ("video", video),
        ("audio_languages", list(of("audio").map(language).collect())),
        (
            "audio_codecs",
            list(of("audio").filter_map(|s| s.codec.clone()).collect()),
        ),
        (
            "subtitle_languages",
            list(of("subtitle").map(language).collect()),
        ),
    ]
}

/// How each other sampled file differs from the compared one: a different
/// resolution, a missing dub or subtitle track.
pub fn file_differences(
    compared: &MediaInspectionReport,
    others: &[(String, &MediaInspectionReport)],
) -> Vec<FileDifference> {
    let reference = signature(compared);
    others
        .iter()
        .flat_map(|(file, media)| {
            signature(media)
                .into_iter()
                .zip(reference.clone())
                .filter(|((_, this), (_, base))| this != base)
                .map(|((field, this), (_, base))| FileDifference {
                    file: file.clone(),
                    field: field.into(),
                    compared: base,
                    this_file: this,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TorrentReference;
    use crate::torrent::TorrentFile;

    fn catalog(paths: &[&str]) -> TorrentCatalog {
        let reference = TorrentReference::Magnet {
            uri: "magnet:?xt=urn:btih:0000000000000000000000000000000000000000".into(),
            info_hash_v1: None,
        };
        TorrentCatalog {
            schema_version: 1,
            source_reference: reference.clone(),
            resolved_reference: reference,
            info_hash_v1: "0".repeat(40),
            name: None,
            files: paths
                .iter()
                .enumerate()
                .map(|(id, path)| TorrentFile {
                    id,
                    path: (*path).into(),
                    size_bytes: if path.contains("sample") { 1 } else { 1000 },
                    media_kind: crate::torrent::media_kind(path).into(),
                })
                .collect(),
        }
    }

    fn target(season: Option<u16>, episodes: Option<(u16, u16)>) -> SearchTarget {
        SearchTarget {
            title: "Show".into(),
            aliases: vec![],
            year: None,
            season,
            part: None,
            episodes: episodes.map(|(start, end)| EpisodeRange { start, end }),
        }
    }

    fn range(start: u16, end: u16) -> EpisodeRange {
        EpisodeRange { start, end }
    }

    #[test]
    fn reads_episode_numbers_in_the_usual_naming_styles() {
        for (path, season, episodes) in [
            ("Show.S02E05.1080p.WEB.x265.mkv", Some(2), Some((5, 5))),
            ("Show.S01E01-E02.mkv", Some(1), Some((1, 2))),
            ("Show 2x07 HDTV.avi", Some(2), Some((7, 7))),
            (
                "[SubsPlease] Frieren - 05 (1080p) [A1B2C3D4].mkv",
                None,
                Some((5, 5)),
            ),
            (
                "[Group] Frieren - 12v2 [BD 1080p].mkv",
                None,
                Some((12, 12)),
            ),
            ("Show/Season 3/Episode 4.mkv", Some(3), Some((4, 4))),
            ("葬送のフリーレン 第05話.mp4", None, Some((5, 5))),
            ("Movie.2023.2160p.mkv", None, None),
        ] {
            let found = parse(path);
            assert_eq!(found.season, season, "{path}");
            assert_eq!(found.episodes, episodes, "{path}");
        }
    }

    #[test]
    fn finds_missing_and_duplicate_episodes_in_a_season_pack() {
        let pack = catalog(&[
            "Show S01/Show.S01E01.mkv",
            "Show S01/Show.S01E02.mkv",
            "Show S01/Show.S01E04.mkv",
            "Show S01/Show.S01E04.REPACK.mkv",
            "Show S01/sample.mkv",
            "Show S01/Show.S01E05.nfo",
        ]);
        let coverage = episode_coverage(&pack, &target(Some(1), Some((1, 5)))).unwrap();
        assert_eq!(coverage.main_files, 4);
        assert_eq!(coverage.unnumbered, 0);
        assert_eq!(coverage.seasons.len(), 1);
        let season = &coverage.seasons[0];
        assert_eq!(season.season, Some(1));
        assert_eq!(season.episodes, vec![range(1, 2), range(4, 4)]);
        assert_eq!(season.gaps, vec![range(3, 3)]);
        assert_eq!(season.duplicates, vec![4]);
        assert_eq!(coverage.missing, Some(vec![range(3, 3), range(5, 5)]));
    }

    #[test]
    fn the_changing_number_is_the_episode_when_names_have_no_label() {
        let pack = catalog(&[
            "[Group] Show 01 [1080p][ABCDEF01].mkv",
            "[Group] Show 02 [1080p][12345678].mkv",
            "[Group] Show 03 [1080p][9ABCDEF0].mkv",
        ]);
        let coverage = episode_coverage(&pack, &target(None, Some((1, 3)))).unwrap();
        assert_eq!(coverage.seasons[0].episodes, vec![range(1, 3)]);
        assert_eq!(coverage.missing, Some(vec![]));
    }

    #[test]
    fn a_film_has_no_episode_coverage() {
        let film = catalog(&["Movie.2023.2160p.mkv", "sample.mkv"]);
        assert_eq!(episode_coverage(&film, &target(None, None)), None);
    }
}
