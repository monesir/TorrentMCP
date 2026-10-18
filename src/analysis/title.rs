use super::{AnalyzedTitle, SearchTarget, TitleCorrection};
use encoding_rs::WINDOWS_1252;
use unicode_normalization::UnicodeNormalization;

fn corruption_score(text: &str) -> usize {
    ["Ã", "Â", "â€", "â…", "ðŸ", "ï¿½", "�"]
        .iter()
        .map(|marker| text.matches(marker).count())
        .sum()
}

pub(super) fn normalize_title(raw: &str, source_id: &str) -> AnalyzedTitle {
    let mut title = AnalyzedTitle {
        source_id: source_id.into(),
        raw: raw.into(),
        normalized: raw.into(),
        corrections: vec![],
        issues: vec![],
    };
    for _ in 0..2 {
        if corruption_score(&title.normalized) == 0 {
            break;
        }
        let (bytes, _, lossy) = WINDOWS_1252.encode(&title.normalized);
        if lossy {
            break;
        }
        let Ok(repaired) = std::str::from_utf8(&bytes) else {
            break;
        };
        if corruption_score(repaired) >= corruption_score(&title.normalized) {
            break;
        }
        title.corrections.push(TitleCorrection {
            rule: "strict_windows1252_utf8_roundtrip".into(),
            before: title.normalized.clone(),
            after: repaired.into(),
        });
        title.normalized = repaired.into();
    }
    if corruption_score(&title.normalized) > 0 {
        title
            .issues
            .push("possible_unrepaired_encoding_damage".into());
    }
    let normalized: String = title
        .normalized
        .nfkc()
        .map(|character| {
            if character.is_control() || character == '_' {
                ' '
            } else {
                character
            }
        })
        .collect();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized != title.normalized {
        title.corrections.push(TitleCorrection {
            rule: "unicode_nfkc_and_whitespace".into(),
            before: title.normalized.clone(),
            after: normalized.clone(),
        });
        title.normalized = normalized;
    }
    title
}

fn words(text: &str) -> Vec<(usize, usize, String)> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            words.push((begin, index, fold(&text[begin..index])));
        }
    }
    words
}

/// Lowercase without diacritics, so "Shōgun" and "Shogun" compare equal.
fn fold(word: &str) -> String {
    word.nfd()
        .filter(|character| !unicode_normalization::char::is_combining_mark(*character))
        .collect::<String>()
        .to_lowercase()
}

static SITE_PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^\s*[\[(]?\s*(?:www\.)?[a-z0-9-]+(?:\.[a-z0-9-]+)*\.(?:com|org|net|to|io|me|info|cc|xyz|ru|lol|club|site|ws|se|pro|gg|mx|in|li|is|nz|eu)\b\s*[\])]?\s*[-–:|]?",
    )
    .unwrap()
});

pub(super) fn canonical_words(text: &str) -> Vec<String> {
    words(&text.nfkc().collect::<String>())
        .into_iter()
        .map(|(_, _, word)| word)
        .collect()
}

/// Return a byte boundary in the analysis title, preserving separators in metadata.
/// Match only a full token prefix, optionally after a leading release-group tag.
pub(super) fn title_prefix_end(text: &str, target: &SearchTarget) -> Option<usize> {
    let tokens = words(text);
    let group_end = text
        .strip_prefix('[')
        .and_then(|rest| rest.find(']').map(|end| end + 2));
    // Uploader sites prefix names: "www.UIndex.org - The Bear S03 ...".
    let site_end = SITE_PREFIX.find(text).map(|found| found.end());
    let token_index = |end: usize| {
        tokens
            .iter()
            .take_while(|(start, _, _)| *start < end)
            .count()
    };
    let starts = [
        Some(0),
        group_end.map(token_index),
        site_end.map(token_index),
    ];
    let mut best = None;
    for alias in std::iter::once(&target.title).chain(target.aliases.iter()) {
        let alias = canonical_words(alias);
        if alias.is_empty() {
            continue;
        }
        for start in starts.into_iter().flatten() {
            if tokens.len() < start + alias.len() {
                continue;
            }
            if tokens[start..start + alias.len()]
                .iter()
                .map(|(_, _, token)| token)
                .eq(alias.iter())
            {
                let end = tokens[start + alias.len() - 1].1;
                if best.is_none_or(|previous| end > previous) {
                    best = Some(end);
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repairs_observed_roman_corruption_without_touching_legitimate_multilingual_titles() {
        assert_eq!(
            normalize_title("Nirvana in Fire â…¡", "fixture").normalized,
            "Nirvana in Fire II"
        );
        for text in ["Château", "São Paulo", "琅琊榜", "اختبار | عربي"] {
            let title = normalize_title(text, "fixture");
            assert_eq!(title.normalized, text);
            assert!(title.corrections.is_empty());
        }
    }
}
