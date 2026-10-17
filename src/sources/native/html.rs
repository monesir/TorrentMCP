use super::{candidate, runtime};
use crate::{
    SearchError, SearchRequest, SearchResult, SourceDescriptor, TorrentReference,
    reference::parse_reference,
};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use scraper::{ElementRef, Html, Selector};

pub(super) const QUERY_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub(super) fn search_url(
    source: &SourceDescriptor,
    request: &SearchRequest,
    page: usize,
) -> Result<String, SearchError> {
    let base = source.site_url.trim_end_matches('/');
    let query = utf8_percent_encode(&request.query, QUERY_SAFE).to_string();
    Ok(match source.id.as_str() {
        "eztv" => format!("{base}/search/{}", query.replace("%20", "-")),
        "solidtorrents" => {
            let category = match request.category.as_str() {
                "all" => "all",
                "music" => "Audio",
                "books" => "eBook",
                _ => return Err(runtime("Unsupported SolidTorrents category")),
            };
            format!(
                "{base}/search?q={query}&category={category}&sort=seeders&sort=desc&page={page}"
            )
        }
        "torlock" => {
            let category = match request.category.as_str() {
                "all" => "all",
                "anime" => "anime",
                "software" => "software",
                "games" => "game",
                "movies" => "movie",
                "music" => "music",
                "tv" => "television",
                "books" => "ebooks",
                _ => return Err(runtime("Unsupported TorLock category")),
            };
            format!(
                "{base}/{category}/torrents/{}.html?sort=seeds&page={page}",
                query.replace("%20", "-")
            )
        }
        "torrentproject" => format!("{base}/browse?t={}&p={page}", query.replace("%20", "+")),
        _ => return Err(runtime("No native HTML adapter for source")),
    })
}

fn select<'a>(node: ElementRef<'a>, selector: &str) -> Option<ElementRef<'a>> {
    node.select(&Selector::parse(selector).expect("Static selector"))
        .next()
}
fn text(node: ElementRef<'_>) -> String {
    node.text().collect::<String>().trim().to_owned()
}
fn field(node: ElementRef<'_>, selector: &str) -> String {
    select(node, selector).map(text).unwrap_or_default()
}
fn absolute(base: &str, href: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|base| base.join(href))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}
fn count(value: &str) -> Option<u64> {
    value.trim().replace(',', "").parse().ok()
}
fn size(value: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(value)
}
fn local_date(value: &str, format: &str) -> Option<u64> {
    let date = NaiveDate::parse_from_str(value.trim(), format)
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    local_datetime(date)
}
fn local_datetime(date: NaiveDateTime) -> Option<u64> {
    Local
        .from_local_datetime(&date)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}

pub(super) fn parse(
    response: &str,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    // A challenge must not masquerade as a successful zero-result search.
    if document
        .select(&Selector::parse("table, article, .search-result, #similarfiles").unwrap())
        .next()
        .is_none()
        && ["cf-chl-", "Just a moment...", "captcha"]
            .iter()
            .any(|needle| response.contains(needle))
    {
        return Err(runtime(
            "Source returned an access challenge instead of search results",
        ));
    }
    match source.id.as_str() {
        "eztv" => parse_eztv(&document, source, now),
        "torlock" => parse_torlock(&document, source, now),
        "solidtorrents" => parse_solid(&document, source, now),
        "torrentproject" => parse_project(&document, source, now),
        _ => Err(runtime("No parser for source")),
    }
}

fn parse_eztv(
    document: &Html,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let rows = Selector::parse("tr.forum_header_border[name=hover]").unwrap();
    let mut results = vec![];
    for row in document.select(&rows) {
        let (Some(info), Some(magnet)) = (select(row, "a.epinfo"), select(row, "a.magnet")) else {
            continue;
        };
        let title = info
            .value()
            .attr("title")
            .unwrap_or("")
            .split(" (")
            .next()
            .unwrap_or("")
            .to_owned();
        let mut bytes = None;
        let mut seeds = None;
        let mut published = None;
        for fragment in row.text() {
            let value = fragment.replace(',', "");
            if [" KB", " MB", " GB"]
                .iter()
                .any(|unit| value.ends_with(unit))
            {
                bytes = size(&value);
            } else if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
                seeds = count(&value);
            } else if let Some(age) = eztv_age(&value) {
                published = now.checked_sub(age);
            }
        }
        results.push(candidate(
            source,
            title,
            magnet.value().attr("href").unwrap_or("").into(),
            Some(absolute(
                &source.site_url,
                info.value().attr("href").unwrap_or(""),
            )?),
            bytes,
            seeds,
            None,
            published,
            now,
        )?);
    }
    Ok(results)
}
fn eztv_age(value: &str) -> Option<u64> {
    use std::sync::OnceLock;
    static AGE: OnceLock<regex::Regex> = OnceLock::new();
    let capture = AGE
        .get_or_init(|| {
            regex::Regex::new(r"^(\d+)(h|d)\s+(\d+)(m|h)|^(\d+)\s+(weeks?|mo|years?)").unwrap()
        })
        .captures(value)?;
    if let Some(a) = capture.get(1) {
        let first: u64 = a.as_str().parse().ok()?;
        let second: u64 = capture[3].parse().ok()?;
        Some(if &capture[2] == "h" {
            first * 3600 + second * 60
        } else {
            first * 86400 + second * 3600
        })
    } else {
        let amount: u64 = capture[5].parse().ok()?;
        Some(
            amount
                * 86400
                * match &capture[6] {
                    "week" | "weeks" => 7,
                    "mo" => 30,
                    _ => 365,
                },
        )
    }
}

fn parse_torlock(
    document: &Html,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let rows = Selector::parse("article tr").unwrap();
    let mut results = vec![];
    for row in document.select(&rows) {
        let Some(link) = select(row, "a[href^='/torrent/']") else {
            continue;
        };
        if link.value().attr("rel") == Some("nofollow") {
            continue;
        }
        let href = link.value().attr("href").unwrap();
        let Some(id) = href.split('/').nth(2).filter(|id| !id.is_empty()) else {
            continue;
        };
        let date = field(row, "td.td");
        let published = if date == "Today" || date == "Yesterday" {
            let date = Local::now().date_naive()
                - chrono::Duration::days(if date == "Yesterday" { 1 } else { 0 });
            date.and_hms_opt(0, 0, 0).and_then(local_datetime)
        } else {
            local_date(&date, "%m/%d/%Y")
        };
        results.push(candidate(
            source,
            text(link),
            format!("{}/tor/{id}.torrent", source.site_url.trim_end_matches('/')),
            Some(absolute(&source.site_url, href)?),
            size(&field(row, "td.ts")),
            count(&field(row, "td.tul")),
            count(&field(row, "td.tdl")),
            published,
            now,
        )?);
    }
    Ok(results)
}

fn parse_solid(
    document: &Html,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let rows = Selector::parse(".search-result").unwrap();
    let mut results = vec![];
    for row in document.select(&rows) {
        let (Some(title), Some(magnet)) = (select(row, "h5.title a"), select(row, "a.dl-magnet"))
        else {
            continue;
        };
        let stats: Vec<_> = select(row, ".stats")
            .map(|stats| stats.select(&Selector::parse("div").unwrap()).collect())
            .unwrap_or_default();
        let value = |index: usize| stats.get(index).copied().map(text).unwrap_or_default();
        // The plugin counts descendant divs, starting with stats itself at 0.
        let published = local_date(&value(4), "%b %d, %Y");
        let seed = stats
            .get(2)
            .and_then(|&cell| select(cell, "font"))
            .map(text);
        let leech = stats
            .get(3)
            .and_then(|&cell| select(cell, "font"))
            .map(text);
        results.push(candidate(
            source,
            text(title),
            magnet.value().attr("href").unwrap_or("").into(),
            Some(absolute(
                &source.site_url,
                title.value().attr("href").unwrap_or(""),
            )?),
            size(&value(1)),
            seed.as_deref().and_then(count),
            leech.as_deref().and_then(count),
            published,
            now,
        )?);
    }
    Ok(results)
}

fn parse_project(
    document: &Html,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let rows = Selector::parse("#similarfiles div").unwrap();
    let mut results = vec![];
    for row in document.select(&rows) {
        if row.value().classes().any(|c| c == "gac_bb") {
            continue;
        }
        let spans: Vec<_> = row
            .select(&Selector::parse("span:not([title='verified'])").unwrap())
            .collect();
        if spans.len() < 6 {
            continue;
        }
        // Only a leaf result owns this span group; parent containers are not
        // duplicate results. Nested markup within an individual span is fine.
        if select(row, "div").is_some() {
            continue;
        }
        let Some(link) = select(spans[0], "a[href]") else {
            continue;
        };
        let title = text(spans[0]);
        if title.eq_ignore_ascii_case("nome") {
            continue;
        }
        let uri = absolute(&source.site_url, link.value().attr("href").unwrap())?;
        let published = NaiveDateTime::parse_from_str(&text(spans[4]), "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(local_datetime);
        results.push(candidate(
            source,
            title,
            uri.clone(),
            Some(uri),
            size(&text(spans[5])),
            count(&text(spans[2])),
            count(&text(spans[3])),
            published,
            now,
        )?);
    }
    Ok(results)
}

pub(super) fn resolve_magnet(
    response: &str,
    source: &str,
) -> Result<TorrentReference, SearchError> {
    let document = Html::parse_document(response);
    let selector = Selector::parse("a[href]").unwrap();
    let mut first_error = None;
    for link in document.select(&selector) {
        if let Some(uri) = link.value().attr("href") {
            // The HTML parser already handles &amp;. Percent escapes inside a
            // magnet belong to its query values, not to the surrounding HTML.
            // Decode only a wrapper that escaped the entire URI, exactly once.
            let uri = if uri.to_ascii_lowercase().starts_with("magnet%3a") {
                percent_encoding::percent_decode_str(uri).decode_utf8_lossy()
            } else {
                std::borrow::Cow::Borrowed(uri)
            };
            if uri
                .get(..7)
                .is_some_and(|s| s.eq_ignore_ascii_case("magnet:"))
            {
                match parse_reference(&uri, source) {
                    Ok(reference) => return Ok(reference),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
        }
    }
    Err(first_error.unwrap_or_else(|| {
        runtime("Source returned no magnet; the HTTP torrent reference remains unresolved")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_captured_torrentdownloads_magnets() {
        // Captured from the live Python report on 2026-10-17. Decoding these
        // display names produces C1 controls although the escaped URI is valid.
        let magnets: Vec<String> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/torrentdownloads-magnets.json"
        ))
        .unwrap();
        assert_eq!(magnets.len(), 4);
        for uri in magnets {
            let html = format!(
                "<a href=\"{}\">Download</a>",
                html_escape::encode_double_quoted_attribute(&uri)
            );
            assert_eq!(
                resolve_magnet(&html, "torrentdownloads").unwrap().uri(),
                uri
            );
        }
    }

    #[test]
    fn preserves_magnet_query_boundaries_and_encoded_trackers() {
        let uri = "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&dn=A%26B%3D1%2520&tr=https%3A%2F%2Fexample.org%2Fannounce%3Fkey%3Da%26pass%3Db";
        let html = format!("<a href=\"{}\">Download</a>", uri.replace('&', "&amp;"));
        assert_eq!(
            resolve_magnet(&html, "torrentdownloads").unwrap().uri(),
            uri
        );
    }

    #[test]
    fn accepts_a_whole_uri_wrapper_without_decoding_inner_escapes_twice() {
        let uri = "magnet:?xt=urn:btih:0000000000000000000000000000000000000000&dn=A%26B%C2%8D";
        let wrapped =
            percent_encoding::utf8_percent_encode(uri, percent_encoding::NON_ALPHANUMERIC);
        let html = format!("<a href=\"{wrapped}\">Download</a>");
        assert_eq!(
            resolve_magnet(&html, "torrentdownloads").unwrap().uri(),
            uri
        );
    }

    #[test]
    fn tries_later_magnets_but_reports_an_all_invalid_page() {
        let good = "magnet:?xt=urn:btih:0000000000000000000000000000000000000000";
        let bad = "<a href=\"magnet:?xt=urn:btih:invalid\">Bad</a>";
        let html = format!("{bad}<a href=\"{good}\">Good</a>");
        assert_eq!(
            resolve_magnet(&html, "torrentdownloads").unwrap().uri(),
            good
        );
        assert!(resolve_magnet(bad, "torrentdownloads").is_err());
        assert!(resolve_magnet("<p>No download</p>", "torrentdownloads").is_err());
    }
}
