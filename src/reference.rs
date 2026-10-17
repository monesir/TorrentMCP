use crate::{SearchError, TorrentReference};
use data_encoding::BASE32_NOPAD;
use url::Url;

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
