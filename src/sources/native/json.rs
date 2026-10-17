use super::{candidate, runtime};
use crate::{SearchError, SearchRequest, SearchResult, SourceDescriptor};
use serde_json::Value;

pub(super) use crate::reference::form_value;

pub(super) fn search_url(
    descriptor: &SourceDescriptor,
    request: &SearchRequest,
) -> Result<(String, String), SearchError> {
    let base = descriptor.site_url.trim_end_matches('/');
    if descriptor.id == "torrentscsv" {
        let query = percent_encoding::utf8_percent_encode(&request.query, super::html::QUERY_SAFE)
            .to_string();
        Ok((
            format!("{base}/service/search?size=100&q={query}"),
            format!("{base}/#/search/torrent/{query}/1"),
        ))
    } else {
        // apibay is a distinct upstream API. A configured mirror remains the
        // endpoint for explicit replay/testing instead of escaping to the Internet.
        let api = if base == "https://thepiratebay.org" {
            "https://apibay.org"
        } else {
            base
        };
        let mut url =
            url::Url::parse(&format!("{api}/q.php")).map_err(|e| runtime(e.to_string()))?;
        let mut query = format!("q={}", form_value(&request.query));
        let category = match request.category.as_str() {
            "all" => None,
            "music" => Some("100"),
            "movies" => Some("200"),
            "games" => Some("400"),
            "software" => Some("300"),
            _ => return Err(runtime("Unsupported Pirate Bay category")),
        };
        if let Some(category) = category {
            query.push_str(&format!("&cat={category}"));
        }
        url.set_query(Some(&query));
        Ok((url.into(), String::new()))
    }
}

pub(super) fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))
}

pub(super) fn parse(
    text: &str,
    descriptor: &SourceDescriptor,
    description: &str,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    // The original helper unescapes entities before JSON parsing and protects
    // &quot; so that source titles containing it remain valid JSON strings.
    let escaped = text.replace("&quot;", "\\\"");
    let decoded = html_escape::decode_html_entities(&escaped);
    let response: Value = serde_json::from_str(&decoded)?;
    let csv = descriptor.id == "torrentscsv";
    let items = if csv {
        response.get("torrents").and_then(Value::as_array)
    } else {
        response.as_array()
    }
    .ok_or_else(|| runtime("Unexpected source JSON schema"))?;
    items
        .iter()
        .filter_map(|item| {
            let hash = item[if csv { "infohash" } else { "info_hash" }].as_str();
            if !csv && hash == Some("0000000000000000000000000000000000000000") {
                return None;
            }
            Some((|| {
                let hash = hash.ok_or_else(|| runtime("Result has no infohash"))?;
                let title = item["name"]
                    .as_str()
                    .ok_or_else(|| runtime("Result has no title"))?
                    .to_owned();
                let mut parameters = format!("dn={}", form_value(&title));
                for tracker in crate::reference::PUBLIC_TRACKERS {
                    parameters.push_str(&format!("&tr={}", form_value(tracker)));
                }
                let uri = format!("magnet:?xt=urn:btih:{hash}&{parameters}");
                let description = if csv {
                    description.to_owned()
                } else {
                    format!(
                        "{}/description.php?id={}",
                        descriptor.site_url.trim_end_matches('/'),
                        string(&item["id"]).ok_or_else(|| runtime("Result has no id"))?
                    )
                };
                candidate(
                    descriptor,
                    title,
                    uri,
                    Some(description),
                    number(&item[if csv { "size_bytes" } else { "size" }]),
                    number(&item["seeders"]),
                    number(&item["leechers"]),
                    number(&item[if csv { "created_unix" } else { "added" }]),
                    now,
                )
            })())
        })
        .collect()
}
