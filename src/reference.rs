use crate::{SearchError, TorrentReference};
use data_encoding::BASE32_NOPAD;
use regex::Regex;
use std::sync::LazyLock;
use url::Url;

/// Public trackers added to magnets built from a bare infohash.
pub const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.internetwarriors.net:1337/announce",
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://p4p.arenabg.ch:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://www.torrent.eu.org:451/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://retracker.lanta-net.ru:2710/announce",
    "udp://open.stealth.si:80/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
];

/// application/x-www-form-urlencoded value encoding, as qBittorrent plugins use.
pub fn form_value(value: &str) -> String {
    const SAFE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    percent_encoding::utf8_percent_encode(value, SAFE)
        .to_string()
        .replace("%20", "+")
}

/// A magnet for a known v1 infohash, with a display name and public trackers.
pub fn magnet_from_hash(hash: &str, title: &str) -> String {
    let mut uri = format!(
        "magnet:?xt=urn:btih:{}&dn={}",
        hash.to_ascii_lowercase(),
        form_value(title)
    );
    for tracker in PUBLIC_TRACKERS {
        uri.push_str(&format!("&tr={}", form_value(tracker)));
    }
    uri
}

// Only URL shapes known to carry the torrent's own infohash: a hash as the first
// path segment (torrentdownload.info) or a hash-named download (itorrents, YTS,
// academictorrents). Other 40-hex strings, such as image checksums, are ignored.
static HASH_SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/([0-9A-Fa-f]{40})(?:/|$)").unwrap());
static HASH_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/(?:torrent|download)/([0-9A-Fa-f]{40})(?:\.torrent)?$").unwrap()
});

/// The v1 infohash embedded in an HTTP reference, in lowercase hex.
pub fn info_hash_in_url(uri: &str) -> Option<String> {
    let parsed = Url::parse(uri).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let path = parsed.path();
    HASH_SEGMENT
        .captures(path)
        .or_else(|| HASH_FILE.captures(path))
        .map(|found| found[1].to_ascii_lowercase())
}

/// True when the HTTP reference names the .torrent file itself rather than a page.
pub fn is_torrent_file_url(uri: &str) -> bool {
    Url::parse(uri).is_ok_and(|parsed| parsed.path().ends_with(".torrent"))
}

pub fn parse_reference(uri: &str, source_id: &str) -> Result<TorrentReference, SearchError> {
    if uri.len() > 32 * 1024 || uri.chars().any(char::is_control) {
        return Err(SearchError::Runtime("Invalid torrent reference".into()));
    }
    let parsed = Url::parse(uri)
        .map_err(|_| SearchError::Runtime("Invalid torrent reference URL".into()))?;
    match parsed.scheme() {
        "magnet" => {
            let mut info_hash_v1 = None;
            let mut has_identity = false;
            for (key, value) in parsed.query_pairs() {
                if key != "xt" {
                    continue;
                }
                if let Some(hash) = value.strip_prefix("urn:btih:") {
                    let normalized =
                        if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                            Some(hash.to_ascii_lowercase())
                        } else if hash.len() == 32 {
                            BASE32_NOPAD
                                .decode(hash.to_ascii_uppercase().as_bytes())
                                .ok()
                                .filter(|bytes| bytes.len() == 20)
                                .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect())
                        } else {
                            None
                        };
                    let hash = normalized
                        .ok_or_else(|| SearchError::Runtime("Invalid magnet v1 infohash".into()))?;
                    if info_hash_v1
                        .as_ref()
                        .is_some_and(|previous| previous != &hash)
                    {
                        return Err(SearchError::Runtime("Conflicting magnet identities".into()));
                    }
                    info_hash_v1 = Some(hash);
                    has_identity = true;
                } else if let Some(hash) = value.strip_prefix("urn:btmh:1220") {
                    has_identity |= hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
                }
            }
            if !has_identity {
                return Err(SearchError::Runtime(
                    "Magnet has no supported torrent identity".into(),
                ));
            }
            Ok(TorrentReference::Magnet {
                uri: uri.into(),
                info_hash_v1,
            })
        }
        "https" | "http"
            if parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none() =>
        {
            Ok(TorrentReference::Http {
                uri: uri.into(),
                source_id: source_id.into(),
                post_form: None,
            })
        }
        _ => Err(SearchError::Runtime(
            "Only magnet and HTTP(S) references are supported".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hex_and_base32_without_changing_the_download_uri() {
        let hex = "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&dn=A";
        let base32 = "magnet:?xt=urn:btih:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&dn=B";
        let hash = |reference| match reference {
            TorrentReference::Magnet { info_hash_v1, .. } => info_hash_v1,
            _ => None,
        };
        assert_eq!(
            hash(parse_reference(hex, "a").unwrap()),
            hash(parse_reference(base32, "b").unwrap())
        );
        assert_eq!(parse_reference(base32, "b").unwrap().uri(), base32);
    }

    #[test]
    fn finds_infohash_only_in_known_url_shapes() {
        let hash = "0ed31ccd8167f56a39d6471eb0140439d1b32b5d";
        for uri in [
            "https://www.torrentdownload.info/0ED31CCD8167F56A39D6471EB0140439D1B32B5D/The-Bear",
            "http://itorrents.net/torrent/0ED31CCD8167F56A39D6471EB0140439D1B32B5D.torrent?title=x",
            "https://academictorrents.com/download/0ed31ccd8167f56a39d6471eb0140439d1b32b5d.torrent",
            "https://yts.gg/torrent/download/0ED31CCD8167F56A39D6471EB0140439D1B32B5D",
        ] {
            assert_eq!(info_hash_in_url(uri).as_deref(), Some(hash), "{uri}");
        }
        for uri in [
            "https://example.org/img/0ed31ccd8167f56a39d6471eb0140439d1b32b5d.png",
            "https://example.org/a/0ed31ccd8167f56a39d6471eb0140439d1b32b5d/",
            "https://example.org/torrent/0ed31ccd8167f56a39d6471eb0140439d1b32b5.torrent",
            "https://example.org/torrent/0ed31ccd8167f56a39d6471eb0140439d1b32b5dxtorrent",
            "magnet:?xt=urn:btih:0ed31ccd8167f56a39d6471eb0140439d1b32b5d",
        ] {
            assert_eq!(info_hash_in_url(uri), None, "{uri}");
        }
        let magnet = magnet_from_hash("0ED31CCD8167F56A39D6471EB0140439D1B32B5D", "A & B");
        match parse_reference(&magnet, "a").unwrap() {
            TorrentReference::Magnet { info_hash_v1, .. } => {
                assert_eq!(info_hash_v1.as_deref(), Some(hash))
            }
            other => panic!("{other:?}"),
        }
        assert!(magnet.contains("dn=A+%26+B"));
    }

    #[test]
    fn rejects_invalid_and_conflicting_magnets_and_unsafe_schemes() {
        for uri in [
            "file:///C:/a.torrent",
            "javascript:alert(1)",
            "magnet:?dn=test",
            "magnet:?xt=urn:btih:abc",
            "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&xt=urn:btih:1111111111111111111111111111111111111111",
        ] {
            assert!(parse_reference(uri, "source").is_err(), "{uri}");
        }
    }
}
