//! Native LimeTorrents adapter. Search routes, pagination and field semantics
//! follow the inspected qBittorrent plugin 4.14; original Python stays available.
use super::{SearchSource, SourceOutcome};
use crate::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SourceDescriptor, SourceObservation,
    SourceReport, SourceStatus, TorrentReference, reference::parse_reference,
};
use async_trait::async_trait;
use chrono::{Local, NaiveDate};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use regex::Regex;
use scraper::{Html, Selector};
use std::{
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

const URL_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b':')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
const QUERY_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;

pub struct LimeTorrentsSource {
    descriptor: SourceDescriptor,
    client: reqwest::Client,
}

impl LimeTorrentsSource {
    pub fn new(descriptor: SourceDescriptor) -> Result<Self, SearchError> {
        let base = NaiveDate::from_ymd_opt(2024, 4, 16).expect("Valid date");
        let version = 125 + (Local::now().date_naive() - base).num_days() / 30;
        let user_agent = format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:{version}.0) Gecko/20100101 Firefox/{version}.0"
        );
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .dns_resolver(crate::dns::resolver())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|error| SearchError::Runtime(error.to_string()))?;
        Ok(Self { descriptor, client })
    }

    async fn fetch(&self, url: &str) -> Result<String, SearchError> {
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| SearchError::Runtime(error.to_string()))?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| SearchError::Runtime(error.to_string()))?
        {
            if bytes.len() + chunk.len() > MAX_PAGE_BYTES {
                return Err(SearchError::Runtime(
                    "LimeTorrents page exceeds 2 MiB".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err(SearchError::Runtime("Empty LimeTorrents response".into()));
        }
        let charset = content_type
            .split(';')
            .find_map(|part| part.trim().strip_prefix("charset="))
            .unwrap_or("utf-8");
        let encoding =
            encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
        let (text, _, _) = encoding.decode(&bytes);
        // The original helpers.retrieve_url unescapes once before HTMLParser.
        Ok(html_escape::decode_html_entities(&text).into_owned())
    }

    fn search_url(&self, request: &SearchRequest, page: usize) -> Result<String, SearchError> {
        let category = match request.category.as_str() {
            "all" => "all",
            "anime" => "anime",
            "software" => "applications",
            "games" => "games",
            "movies" => "movies",
            "music" => "music",
            "tv" => "tv",
            _ => {
                return Err(SearchError::InvalidRequest(
                    "Unsupported LimeTorrents category".into(),
                ));
            }
        };
        let query = utf8_percent_encode(&request.query, QUERY_SAFE)
            .to_string()
            .replace("%20", "-");
        Ok(format!(
            "{}/search/{category}/{query}/seeds/{page}/",
            self.descriptor.site_url.trim_end_matches('/')
        ))
    }

    async fn collect(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        for page in 1..=4 {
            let html = self.fetch(&self.search_url(request, page)?).await?;
            let parsed = parse_page(&html, &self.descriptor, now_unix())?;
            let page_count = parsed.len();
            for result in parsed {
                if results.len() >= limit {
                    return Ok(true);
                }
                results.push(result);
            }
            if results.len() >= limit {
                return Ok(true);
            }
            if page_count < 20 {
                break;
            }
        }
        Ok(false)
    }
}

#[async_trait]
impl SearchSource for LimeTorrentsSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    async fn search(
        &self,
        request: &SearchRequest,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> SourceOutcome {
        let started = Instant::now();
        let mut results = Vec::new();
        let (status, limited, error) = tokio::select! {
            biased;
            _ = cancellation.cancelled() => (SourceStatus::Cancelled, false, Some("Search cancelled".into())),
            value = tokio::time::timeout(limits.source_timeout, self.collect(request, limits.results_per_source, &mut results)) => match value {
                Ok(Ok(limited)) => (SourceStatus::Completed, limited, None),
                Ok(Err(error)) => (SourceStatus::Failed, false, Some(error.to_string())),
                Err(_) => (SourceStatus::TimedOut, false, Some("Source deadline exceeded".into())),
            },
        };
        SourceOutcome {
            report: SourceReport {
                source_id: self.descriptor.id.clone(),
                status,
                elapsed_ms: started.elapsed().as_millis() as u64,
                result_count: results.len(),
                limited,
                error,
                queried_category: None,
                failure_kind: None,
            },
            results,
        }
    }

    async fn resolve(
        &self,
        uri: &str,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        let reference = parse_reference(uri, &self.descriptor.id)?;
        if matches!(reference, TorrentReference::Magnet { .. }) {
            return Ok(reference);
        }
        let url =
            url::Url::parse(uri).map_err(|error| SearchError::InvalidRequest(error.to_string()))?;
        let site = url::Url::parse(&self.descriptor.site_url)
            .map_err(|error| SearchError::InvalidRequest(error.to_string()))?;
        if url.host_str() != site.host_str() {
            return Err(SearchError::InvalidRequest(
                "Reference must belong to the selected source".into(),
            ));
        }
        let operation = async {
            let text = self.fetch(uri).await?;
            let document = Html::parse_document(&text);
            let selector = Selector::parse("a[href]").expect("Static selector");
            for anchor in document.select(&selector) {
                if let Some(magnet) = anchor
                    .value()
                    .attr("href")
                    .filter(|href| href.starts_with("magnet:"))
                {
                    return parse_reference(magnet, &self.descriptor.id);
                }
            }
            Err(SearchError::Runtime(
                "LimeTorrents returned no magnet link".into(),
            ))
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(SearchError::Runtime("Resolution cancelled".into())),
            result = tokio::time::timeout(limits.source_timeout, operation) => result.map_err(|_| SearchError::Runtime("Resolution deadline exceeded".into()))?,
        }
    }
}

pub fn parse_page(
    html: &str,
    descriptor: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    static ROWS: OnceLock<Selector> = OnceLock::new();
    static CELLS: OnceLock<Selector> = OnceLock::new();
    static LINKS: OnceLock<Selector> = OnceLock::new();
    let document = Html::parse_document(html);
    let rows = ROWS.get_or_init(|| Selector::parse("table.table2 tr[bgcolor]").unwrap());
    let cells_selector = CELLS.get_or_init(|| Selector::parse("td").unwrap());
    let links = LINKS.get_or_init(|| Selector::parse("a[href]").unwrap());
    let mut results = Vec::new();
    for row in document.select(rows) {
        if !matches!(row.value().attr("bgcolor"), Some("#F4F4F4" | "#FFFFFF")) {
            continue;
        }
        let cells: Vec<_> = row.select(cells_selector).collect();
        if cells.len() < 5 {
            continue;
        }
        let Some(anchor) = cells[0].select(links).find(|anchor| {
            anchor
                .value()
                .attr("href")
                .is_some_and(|href| href.ends_with(".html"))
        }) else {
            continue;
        };
        let href = anchor.value().attr("href").unwrap();
        let uri = utf8_percent_encode(
            &format!("{}{}", descriptor.site_url.trim_end_matches('/'), href),
            URL_SAFE,
        )
        .to_string();
        let raw_title = anchor.text().collect::<String>().trim().to_owned();
        let title: String = raw_title
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if title.is_empty() {
            continue;
        }
        let text = |index: usize| cells[index].text().collect::<String>().trim().to_owned();
        // The row's .torrent mirror link (itorrents) is named by the infohash.
        let reference = match cells[0]
            .select(links)
            .find_map(|anchor| crate::reference::info_hash_in_url(anchor.value().attr("href")?))
        {
            Some(hash) => parse_reference(
                &crate::reference::magnet_from_hash(&hash, &title),
                &descriptor.id,
            )?,
            None => parse_reference(&uri, &descriptor.id)?,
        };
        results.push(SearchResult {
            title: title.clone(),
            reference,
            observations: vec![SourceObservation {
                source_id: descriptor.id.clone(),
                raw_title: Some(raw_title),
                title,
                site_url: descriptor.site_url.clone(),
                description_url: Some(uri),
                size_bytes: parse_size(&text(2)),
                seeders: parse_count(&text(3)),
                leechers: parse_count(&text(4)),
                published_unix: parse_age(&text(1), now),
                observed_at_unix: now,
            }],
        });
    }
    Ok(results)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn parse_count(text: &str) -> Option<u64> {
    text.replace(',', "").parse().ok()
}
pub(super) fn parse_size(text: &str) -> Option<u64> {
    static SIZE: OnceLock<Regex> = OnceLock::new();
    let captures = SIZE
        .get_or_init(|| Regex::new(r"(?i)^(\d*\.?\d+)\s*([a-z]+)?").unwrap())
        .captures(text.trim())?;
    let size: f64 = captures[1].parse().ok()?;
    let power = match captures
        .get(2)
        .and_then(|unit| unit.as_str().bytes().next())
        .map(|byte| byte.to_ascii_uppercase())
    {
        Some(b'T') => 40,
        Some(b'G') => 30,
        Some(b'M') => 20,
        Some(b'K') => 10,
        _ => 0,
    };
    let bytes = size * 2_f64.powi(power);
    if bytes.is_finite() && bytes >= 0. && bytes < u64::MAX as f64 {
        Some(bytes.round_ties_even() as u64)
    } else {
        None
    }
}
fn parse_age(text: &str, now: u64) -> Option<u64> {
    static AGE: OnceLock<Regex> = OnceLock::new();
    let text = text.to_ascii_lowercase();
    let days = if text.starts_with("yesterday") {
        1.
    } else if text.starts_with("last month") {
        30.
    } else {
        let captures = AGE
            .get_or_init(|| Regex::new(r"^(\d+)\s+(years?|months?|days?|hours?|minutes?)").unwrap())
            .captures(&text)?;
        let amount: f64 = captures[1].parse().ok()?;
        amount
            * match &captures[2] {
                "year" | "years" => 365.,
                "month" | "months" => 30.,
                "day" | "days" => 1.,
                "hour" | "hours" => 1. / 24.,
                _ => 1. / 1440.,
            }
    };
    now.checked_sub((days * 86400.) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_captured_real_page_and_preserves_unknowns() {
        let html = include_str!(
            "../../verification/limetorrents-benchmark/fixtures/nirvana-in-fire-1.html"
        );
        let descriptor = SourceDescriptor {
            id: "limetorrents".into(),
            name: "LimeTorrents".into(),
            site_url: "https://www.limetorrents.lol".into(),
            categories: vec!["all".into()],
            resolves_http: true,
            scope: Default::default(),
            language: None,
            alternate_of: None,
        };
        let results = parse_page(html, &descriptor, 1792196186).unwrap();
        assert_eq!(results.len(), 31);
        assert_eq!(results[0].observations[0].size_bytes, Some(5315022029));
        assert_eq!(results[0].observations[0].seeders, Some(26));
        // The row's itorrents link gives the infohash, so no detail fetch is needed.
        match &results[0].reference {
            TorrentReference::Magnet { info_hash_v1, .. } => assert_eq!(
                info_hash_v1.as_deref(),
                Some("ceeaeba512923796776233ea6b7d49b26b5d6ec8")
            ),
            other => panic!("expected magnet, got {other:?}"),
        }
        assert!(
            results[0].observations[0]
                .description_url
                .as_deref()
                .unwrap()
                .contains("10349792.html")
        );
        assert!(
            results
                .iter()
                .all(|result| matches!(result.reference, TorrentReference::Magnet { .. }))
        );
        assert_eq!(parse_size("unknown"), None);
        assert_eq!(parse_age("unknown", 100), None);
        assert_eq!(parse_size("1.5 KB"), Some(1536));
    }

    #[test]
    fn preserves_raw_titles_and_matches_shared_display_sanitization() {
        let descriptor = SourceDescriptor {
            id: "limetorrents".into(),
            name: "LimeTorrents".into(),
            site_url: "https://example.org".into(),
            categories: vec!["all".into()],
            resolves_http: true,
            scope: Default::default(),
            language: None,
            alternate_of: None,
        };
        let html = "<table class='table2'><tr bgcolor='#FFFFFF'><td><a href='/item.html'>中文 | A\u{008d}B</a></td><td>unknown</td><td>unknown</td><td>1,234</td><td>0</td></tr></table>";
        let rows = parse_page(html, &descriptor, 100).unwrap();
        assert_eq!(rows[0].title, "中文 | AB");
        assert_eq!(
            rows[0].observations[0].raw_title.as_deref(),
            Some("中文 | A\u{008d}B")
        );
        assert_eq!(rows[0].observations[0].seeders, Some(1234));
        assert_eq!(rows[0].observations[0].size_bytes, None);
    }
}
