//! Quality markers in release names: source format, HDR, bit depth, audio
//! codecs and channels, audio language tags, release group.
use super::{Fact, QualitySignals, ReleaseAnalysis};
use regex::Regex;
use std::sync::LazyLock;

static FORMAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(bd[ ._-]?remux|remux|blu[ ._-]?ray|bdrip|brrip|bd(?:25|50)|uhd[ ._-]?bd|web[ ._-]?dl[ ._-]?rip|web[ ._-]?dl|webmux|dlmux|web[ ._-]?rip|hdtv|pdtv|hdrip|dlrip|dvd[ ._-]?rip|dvd(?:5|9)|dvd[ ._-]?scr|screener|hd[ ._-]?cam|cam[ ._-]?rip|hd[ ._-]?ts|telesync|ts[ ._-]?rip|hd[ ._-]?tc|telecine|web)\b",
    )
    .unwrap()
});
// Short capture tags are common words in other cases ("Cam", "Ts"), so only
// their usual upper-case release form counts.
static FORMAT_UPPER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(CAM|TS|TC|SCR|PDVD)\b").unwrap());
static HDR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(hdr10\+|hdr10[ ._-]?plus|hdr10|hdr|hlg|dovi|dolby[ ._-]?vision)").unwrap()
});
static DOLBY_VISION_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bDV\b").unwrap());
static BIT_DEPTH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(8|10|12)[ ._-]?bits?\b").unwrap());
static AUDIO_CODEC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(true[ ._-]?hd|dts[ ._-]?hd[ ._-]?ma|dts[ ._-]?hd|dts[ ._:-]?x|dts|ddp|dd\+|e[ ._-]?ac[ ._-]?3|dd|ac[ ._-]?3|aac|opus|flac|atmos)",
    )
    .unwrap()
});
static CHANNELS_AFTER_CODEC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ ._-]?([1-9])[ ._]([0-9])\b").unwrap());
static CHANNELS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:([2-9])\.([01])|([2-8])ch)\b").unwrap());
static MULTI_AUDIO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(multi|dual(?:[ ._-]?audio)?)\b").unwrap());
// Upscaled video claims a resolution its source never had.
static UPSCALE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(upscaled?|upscl|ai[ ._-]?upscaled?)\b|\bA[Ii]\b").unwrap());
static REPACK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(repack|proper|rerip)\b").unwrap());
static RELEASE_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-([A-Za-z0-9][A-Za-z0-9.]{1,24})(?:\s*\[[^\]]*\])*(?:\.(?:mkv|mp4|avi))?\s*$")
        .unwrap()
});
static LANGUAGE_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\p{L}+\b").unwrap());

fn next_is_letter(text: &str, end: usize) -> bool {
    text[end..].chars().next().is_some_and(char::is_alphabetic)
}

/// Canonical source format for a matched tag.
fn format_value(token: &str) -> &'static str {
    let key: String = token
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    match key.as_str() {
        "bdremux" | "remux" => "remux",
        "bluray" | "bdrip" | "brrip" | "bd25" | "bd50" | "uhdbd" => "bluray",
        "webdl" | "webmux" | "dlmux" | "web" => "webdl",
        "webrip" | "webdlrip" => "webrip",
        "hdtv" | "pdtv" => "hdtv",
        "hdrip" | "dlrip" => "hdrip",
        "dvdrip" | "dvd5" | "dvd9" => "dvdrip",
        "dvdscr" | "screener" | "scr" => "screener",
        "hdcam" | "camrip" | "cam" => "cam",
        "hdts" | "telesync" | "tsrip" | "ts" | "pdvd" => "telesync",
        "hdtc" | "telecine" | "tc" => "telecine",
        _ => unreachable!("FORMAT alternatives are mapped: {key}"),
    }
}

/// Source formats named in one title, with the tag that named each. A remux
/// tag supersedes the bluray/web tag that describes the same disc or stream.
pub(super) fn source_formats(metadata: &str) -> Vec<(&'static str, String)> {
    let mut found: Vec<(&'static str, String)> = Vec::new();
    for matched in FORMAT
        .find_iter(metadata)
        .chain(FORMAT_UPPER.find_iter(metadata))
    {
        let value = format_value(matched.as_str());
        if !found.iter().any(|(existing, _)| *existing == value) {
            found.push((value, matched.as_str().to_owned()));
        }
    }
    if found.iter().any(|(value, _)| *value == "remux") {
        found.retain(|(value, _)| !matches!(*value, "bluray" | "webdl"));
    }
    found
}

/// ISO 639-1 code for a language name or release-name tag ("ITA", "Latino").
pub(super) fn language_code(word: &str) -> Option<&'static str> {
    Some(match word.to_lowercase().as_str() {
        "ita" | "italian" | "italiano" => "it",
        "eng" | "english" | "ingles" | "inglés" => "en",
        "spa" | "esp" | "spanish" | "espanol" | "español" | "castellano" | "latino" => "es",
        "fre" | "fra" | "french" | "vff" | "truefrench" => "fr",
        "ger" | "deu" | "german" | "deutsch" => "de",
        "rus" | "russian" => "ru",
        "ukr" | "ukrainian" => "uk",
        "hin" | "hindi" => "hi",
        "tam" | "tamil" => "ta",
        "tel" | "telugu" => "te",
        "jpn" | "jap" | "japanese" => "ja",
        "kor" | "korean" => "ko",
        "chi" | "chinese" | "mandarin" | "cantonese" => "zh",
        "ara" | "arabic" => "ar",
        "tur" | "turkish" => "tr",
        "por" | "portuguese" | "dublado" => "pt",
        "pol" | "polish" => "pl",
        "dut" | "dutch" => "nl",
        "thai" => "th",
        "persian" | "farsi" => "fa",
        "hebrew" => "he",
        "vietnamese" => "vi",
        "indonesian" => "id",
        _ => return None,
    })
}

/// Bare audio language tags such as "ITA ENG" or "Latino": inferred audio
/// languages. A tag followed by "sub"/"subs" names subtitles instead.
pub(super) fn audio_language_tags(metadata: &str) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for matched in LANGUAGE_WORD.find_iter(metadata) {
        let Some(code) = language_code(matched.as_str()) else {
            continue;
        };
        let rest = metadata[matched.end()..]
            .trim_start_matches([' ', '.', '_', '-'])
            .to_lowercase();
        if rest.starts_with("sub") || rest.starts_with("sottotitoli") {
            continue;
        }
        found.push((code, matched.as_str().to_owned()));
    }
    found
}

fn channel_rank(layout: &str) -> u8 {
    layout
        .split('.')
        .map(|part| part.parse::<u8>().unwrap_or(0))
        .sum()
}

fn note_channels(signals: &mut QualitySignals, layout: String) {
    if signals
        .audio_channels
        .as_deref()
        .is_none_or(|current| channel_rank(&layout) > channel_rank(current))
    {
        signals.audio_channels = Some(layout);
    }
}

fn add_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|existing| existing == value) {
        list.push(value.into());
    }
}

/// Merge the quality markers of one title into the result's signals.
pub(super) fn scan(metadata: &str, full_title: &str, signals: &mut QualitySignals) {
    for found in HDR.find_iter(metadata) {
        if next_is_letter(metadata, found.end()) {
            continue; // HDRip, HDR-like prefixes of other words
        }
        let key: String = found
            .as_str()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+')
            .collect::<String>()
            .to_ascii_lowercase();
        let value = match key.as_str() {
            "hdr10+" | "hdr10plus" => "hdr10_plus",
            "hdr10" => "hdr10",
            "hdr" => "hdr",
            "hlg" => "hlg",
            _ => "dolby_vision",
        };
        add_unique(&mut signals.hdr, value);
    }
    if DOLBY_VISION_TAG.is_match(metadata) {
        add_unique(&mut signals.hdr, "dolby_vision");
    }
    for found in BIT_DEPTH.captures_iter(metadata) {
        let depth: u8 = found[1].parse().unwrap();
        signals.bit_depth = Some(signals.bit_depth.map_or(depth, |current| current.max(depth)));
    }
    for found in AUDIO_CODEC.find_iter(metadata) {
        if next_is_letter(metadata, found.end()) {
            continue; // "DDR", "AACS", ...
        }
        let key: String = found
            .as_str()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+')
            .collect::<String>()
            .to_ascii_lowercase();
        let value = match key.as_str() {
            "truehd" => Some("truehd"),
            "dtshdma" => Some("dts_hd_ma"),
            "dtshd" => Some("dts_hd"),
            "dtsx" => Some("dts_x"),
            "dts" => Some("dts"),
            "ddp" | "dd+" | "eac3" => Some("eac3"),
            "dd" | "ac3" => Some("ac3"),
            "aac" => Some("aac"),
            "opus" => Some("opus"),
            "flac" => Some("flac"),
            _ => None, // atmos
        };
        match value {
            Some(value) => add_unique(&mut signals.audio_codecs, value),
            None => signals.atmos = true,
        }
        if let Some(layout) = CHANNELS_AFTER_CODEC.captures(&metadata[found.end()..]) {
            note_channels(signals, format!("{}.{}", &layout[1], &layout[2]));
        }
    }
    for found in CHANNELS.captures_iter(metadata) {
        let layout = match found.get(3) {
            Some(count) => match count.as_str() {
                "2" => "2.0",
                "6" => "5.1",
                "8" => "7.1",
                _ => continue,
            }
            .to_owned(),
            None => format!("{}.{}", &found[1], &found[2]),
        };
        note_channels(signals, layout);
    }
    signals.multi_audio |= MULTI_AUDIO.is_match(metadata);
    signals.repack |= REPACK.is_match(metadata);
    signals.upscaled |= UPSCALE.is_match(metadata);
    if signals.release_group.is_none()
        && let Some(found) = RELEASE_GROUP.captures(full_title) {
            let group = found[1].trim_end_matches('.');
            let lower = group.to_ascii_lowercase();
            if !group.bytes().all(|b| b.is_ascii_digit())
                && ![
                    "x264", "x265", "h264", "h265", "hevc", "avc", "web", "dl", "rip", "hd",
                    "sdr", "hdr", "mkv", "mp4",
                ]
                .contains(&lower.as_str())
            {
                signals.release_group = Some(group.to_owned());
            }
        }
}

fn single<T: Copy>(fact: &Fact<T>, worst: impl Fn(T, T) -> T) -> Option<T> {
    fact.claims.iter().map(|claim| claim.value).reduce(worst)
}

/// Estimated quality from title claims (0..~140). Conflicting claims count at
/// their lowest value so that a mislabelled release does not rank up.
pub fn quality_score(analysis: &ReleaseAnalysis) -> u32 {
    let format_points = |format: &str| match format {
        "remux" => 60,
        "bluray" => 50,
        "webdl" => 42,
        "webrip" => 34,
        "hdtv" => 22,
        "hdrip" => 18,
        "dvdrip" => 12,
        "screener" => 4,
        "telecine" => 2,
        "telesync" => 1,
        _ => 0, // cam
    };
    let format = analysis
        .source_format
        .claims
        .iter()
        .map(|claim| format_points(&claim.value))
        .min()
        .unwrap_or(25);
    let quality = &analysis.quality;
    // An upscale has at most the detail of a 1080p source.
    let height = single(&analysis.resolution, u16::min)
        .map(|height| if quality.upscaled { height.min(1080) } else { height });
    let resolution = match height {
        Some(4320) => 45,
        Some(2160) => 40,
        Some(1440) => 32,
        Some(1080) => 28,
        Some(720) => 16,
        Some(576) => 6,
        Some(_) => 4,
        None => 10,
    };
    let hdr = quality
        .hdr
        .iter()
        .map(|value| match value.as_str() {
            "dolby_vision" => 12,
            "hdr10_plus" => 10,
            "hlg" => 6,
            _ => 8,
        })
        .max()
        .unwrap_or(0);
    let depth = if quality.bit_depth.is_some_and(|depth| depth >= 10) {
        3
    } else {
        0
    };
    let audio = quality
        .audio_codecs
        .iter()
        .map(|codec| match codec.as_str() {
            "truehd" | "dts_hd_ma" | "dts_x" => 10,
            "flac" => 8,
            "dts_hd" => 7,
            "eac3" | "dts" => 6,
            "ac3" => 4,
            "opus" => 3,
            _ => 2,
        })
        .max()
        .unwrap_or(0);
    let atmos = if quality.atmos { 4 } else { 0 };
    let channels = match quality.audio_channels.as_deref().map(channel_rank) {
        Some(rank) if rank >= 8 => 3,
        Some(rank) if rank >= 6 => 2,
        _ => 0,
    };
    format + resolution + hdr + depth + audio + atmos + channels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(title: &str) -> QualitySignals {
        let mut signals = QualitySignals::default();
        scan(title, title, &mut signals);
        signals
    }

    #[test]
    fn reads_formats_and_lets_remux_supersede_its_disc_tag() {
        let values = |title: &str| {
            source_formats(title)
                .into_iter()
                .map(|(value, _)| value)
                .collect::<Vec<_>>()
        };
        assert_eq!(values("UHD BluRay 2160p TrueHD REMUX-FraMeSToR"), ["remux"]);
        assert_eq!(values("1080p HDTS x264"), ["telesync"]);
        assert_eq!(values("NEW ENG CAM 1080p"), ["cam"]);
        assert_eq!(values("HQ-CAM x264"), ["cam"]);
        assert_eq!(values("1080p WEB H264-FW"), ["webdl"]);
        assert_eq!(values("BDRip HDTV"), ["bluray", "hdtv"]);
        assert_eq!(values("DSNP WEB-DLRip x264"), ["webrip"]);
        assert_eq!(values("Rus WEB DLRip"), ["webrip"]);
        assert_eq!(values("Cam Newton Ts Highlights HDRip"), ["hdrip"]);
        assert!(values("Charlotte's Webbing 1080p").is_empty());
    }

    #[test]
    fn reads_hdr_depth_audio_channels_and_group() {
        let s = signals("2160p WEB-DL DDP5 1 Atmos DV HDR H 265-FLUX[TGx]");
        assert_eq!(s.hdr, ["hdr", "dolby_vision"]);
        assert_eq!(s.audio_codecs, ["eac3"]);
        assert_eq!(s.audio_channels.as_deref(), Some("5.1"));
        assert!(s.atmos);
        assert_eq!(s.release_group.as_deref(), Some("FLUX"));
        let s = signals("UHD BluRay 2160p TrueHD Atmos 7 1 DV HEVC REMUX-FraMeSToR");
        assert_eq!(s.audio_codecs, ["truehd"]);
        assert_eq!(s.audio_channels.as_deref(), Some("7.1"));
        let s = signals("COMPLETE S01-S05 2160p WEB-DL DTS-HD MA HDR10+ 10bit 6CH");
        assert_eq!(s.audio_codecs, ["dts_hd_ma"]);
        assert_eq!(s.hdr, ["hdr10_plus"]);
        assert_eq!(s.bit_depth, Some(10));
        assert_eq!(s.audio_channels.as_deref(), Some("5.1"));
        let s = signals("1080p HDRip x264 - ProLover");
        assert!(s.hdr.is_empty());
        assert_eq!(s.release_group, None);
        assert!(signals("Shogun 2024 S01 MULTi 1080p WEB H264-FW").multi_audio);
        assert!(signals("2160p UPSCALE WEB HEVC").upscaled);
        assert!(signals("Dune: Part Two 2024 4K HDR Ai").upscaled);
        assert!(!signals("Aida 1080p").upscaled);
        assert!(!signals("1080p ENG-ITA MultiSub x264").multi_audio);
    }

    #[test]
    fn language_tags_skip_subtitle_labels() {
        let codes = |title: &str| {
            audio_language_tags(title)
                .into_iter()
                .map(|(code, _)| code)
                .collect::<Vec<_>>()
        };
        assert_eq!(codes("S03 ITA ENG 1080p"), ["it", "en"]);
        assert_eq!(codes("WEB-DL ENG LATINO CASTELLANO"), ["en", "es", "es"]);
        assert!(codes("Complete English Subs 1080p").is_empty());
        assert!(codes("1080p ENG-SUB").is_empty());
    }
}
