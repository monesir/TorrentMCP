//! Native ports of the pinned qBittorrent search plugins. Python remains a
//! selectable implementation, and unknown plugins retain their Python adapter.
mod advanced_sources;
mod catalogue_tables;
mod community;
mod enriched_feeds;
mod fitgirl;
mod game_cards;
mod html;
mod jackett;
mod json;
mod legacy_media;
mod public_apis;
mod public_tables;
mod regional;
mod spanish;
mod tracker_scrape;

use super::{SearchSource, SourceOutcome};
use crate::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SourceDescriptor, SourceObservation,
    SourceReport, SourceStatus, TorrentReference, reference::parse_reference,
};
use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

pub const NATIVE_IDS: &[&str] = &[
    "eztv",
    "jackett",
    "limetorrents",
    "piratebay",
    "solidtorrents",
    "torlock",
    "torrentproject",
    "torrentscsv",
    "nyaasi",
    "sukebeisi",
    "cloudtorrents",
    "animetosho",
    "animetosho_community_006",
    "mikan",
    "subsplease",
    "acgrip",
    "thepiratebay",
    "torrentclaw",
    "divxtotal",
    "dontorrent",
    "esmeraldatorrent",
    "naranjatorrent",
    "tomadivx",
    "calidadtorrent",
    "pediatorrent",
    "glotorrents",
    "kickasstorrents",
    "pirateiro",
    "rockbox",
    "torrentdownload",
    "yourbittorrent",
    "magnetdl",
    "magnetdl_community_031",
    "solidtorrents_community_053",
    "nekobt",
    "tsukihime",
    "yts",
    "yggtracker",
    "goggames",
    "snowfl",
    "snowfl_community_052",
    "btdig",
    "anidex",
    "dmhy",
    "dmhy_community_018",
    "mikanani",
    "nyaapantsu",
    "nyaa",
    "torrentgalaxy",
    "pantsu",
    "sukebei",
    "uindex",
    "uniondht",
    "dodi_repacks",
    "onlinefix",
    "apachetorrent",
    "redetorrent",
    "smallgames",
    "torrenflix",
    "bitsearch",
    "therarbg",
    "torrentdownloads",
    "traht",
    "xxxclubto",
    "bt4gprx",
    "linuxtracker",
    "sktorrent",
    "tokyotoshokan",
    "academictorrents",
    "audiobookbay",
    "darklibria",
    "eztvx",
    "thepiratebay_community_059",
    "torrentdownload_community_066",
    "cpasbien",
    "torrent9",
    "dontorrent_community_020",
    "elitetorrent",
    "maxitorrent",
    "mejortorrent",
    "pediatorrent_community_043",
    "ali213",
    "mypornclub",
    "rutor",
    "zooqle",
    "fitgirl_repacks",
];

#[derive(Clone, Copy, Debug, clap::ValueEnum, PartialEq, Eq)]
pub enum SourceBackend {
    Auto,
    Rust,
    Python,
}

pub fn configure(
    sources: Vec<Arc<dyn SearchSource>>,
    assets: &Path,
    backend: SourceBackend,
    lime_override: Option<SourceBackend>,
) -> Result<Vec<Arc<dyn SearchSource>>, SearchError> {
    sources
        .into_iter()
        .map(|source| {
            let selected = if source.descriptor().id == "limetorrents" {
                lime_override.unwrap_or(backend)
            } else {
                backend
            };
            let descriptor = source.descriptor().clone();
            if selected == SourceBackend::Python {
                return Ok(source);
            }
            if descriptor.id == "limetorrents" {
                return Ok(
                    Arc::new(super::limetorrents::LimeTorrentsSource::new(descriptor)?)
                        as Arc<dyn SearchSource>,
                );
            }
            if NATIVE_IDS.contains(&descriptor.id.as_str()) {
                return Ok(
                    Arc::new(NativeSource::new(descriptor, assets)?) as Arc<dyn SearchSource>
                );
            }
            if selected == SourceBackend::Rust {
                // Keep this as a per-source failure so selecting one supported
                // source is not prevented by unrelated Python-only plugins.
                return Ok(Arc::new(UnavailableNative(descriptor)) as Arc<dyn SearchSource>);
            }
            Ok(source)
        })
        .collect()
}

pub struct NativeSource {
    descriptor: SourceDescriptor,
    client: reqwest::Client,
    assets: PathBuf,
}

impl NativeSource {
    pub fn new(mut descriptor: SourceDescriptor, assets: &Path) -> Result<Self, SearchError> {
        if descriptor.id == "jackett" {
            if let Ok(config) = jackett::configuration(assets) {
                descriptor.site_url = config.url.trim_end_matches('/').to_owned();
            }
        }
        let base = chrono::NaiveDate::from_ymd_opt(2024, 4, 16).unwrap();
        let version = 125 + (chrono::Utc::now().date_naive() - base).num_days() / 30;
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:{version}.0) Gecko/20100101 Firefox/{version}.0"))
            .connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 { attempt.error("Too many redirects") }
                else if matches!(attempt.url().scheme(), "http" | "https") { attempt.follow() }
                else { attempt.stop() }
            }));
        if descriptor.id == "jackett" {
            builder = builder.no_proxy();
        }
        if descriptor.id == "zooqle" {
            builder = builder.cookie_store(true);
        }
        if descriptor.id == "rutor" {
            if let Ok(text) = std::fs::read_to_string(assets.join("qbit/engines/rutor.json")) {
                let config: serde_json::Value = serde_json::from_str(&text)?;
                if let Some(ua) = config["ua"].as_str() {
                    builder = builder.user_agent(ua);
                }
                if config["proxy"].as_bool() == Some(true) {
                    let proxies = config["proxies"]
                        .as_object()
                        .ok_or_else(|| runtime("Rutor proxy is enabled but missing"))?;
                    let mut configured = false;
                    for scheme in ["http", "https"] {
                        if let Some(value) = proxies
                            .get(scheme)
                            .and_then(|s| s.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            let proxy = if scheme == "http" {
                                reqwest::Proxy::http(value)
                            } else {
                                reqwest::Proxy::https(value)
                            }
                            .map_err(|_| runtime("Invalid Rutor proxy"))?;
                            builder = builder.proxy(proxy);
                            configured = true;
                        }
                    }
                    if !configured {
                        return Err(runtime("Rutor proxy is enabled but not configured"));
                    }
                }
            }
        }
        if descriptor.id == "torrentclaw" {
            if let Ok(key) = std::env::var("TORRENTCLAW_API_KEY") {
                if !key.is_empty() {
                    let mut value =
                        reqwest::header::HeaderValue::from_str(&format!("Bearer {key}"))
                            .map_err(|_| runtime("Invalid TorrentClaw API key header"))?;
                    value.set_sensitive(true);
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                    headers.insert(
                        reqwest::header::ACCEPT,
                        reqwest::header::HeaderValue::from_static("application/json"),
                    );
                    builder = builder
                        .default_headers(headers)
                        .user_agent("qBittorrent (TorrentClaw plugin)");
                }
            }
        }
        let client = builder.build().map_err(http_error)?;
        Ok(Self {
            descriptor,
            client,
            assets: assets.into(),
        })
    }

    async fn fetch(&self, url: &str, post: Option<&str>) -> Result<String, SearchError> {
        self.fetch_bounded(url, post, 4 * 1024 * 1024).await
    }

    async fn fetch_bounded(
        &self,
        url: &str,
        post: Option<&str>,
        max_bytes: usize,
    ) -> Result<String, SearchError> {
        self.fetch_inner(url, post, None, max_bytes).await
    }

    async fn fetch_referer(
        &self,
        url: &str,
        post: Option<&str>,
        referer: Option<&str>,
    ) -> Result<String, SearchError> {
        self.fetch_inner(url, post, referer, 4 * 1024 * 1024).await
    }

    async fn fetch_inner(
        &self,
        url: &str,
        post: Option<&str>,
        referer: Option<&str>,
        max_bytes: usize,
    ) -> Result<String, SearchError> {
        self.fetch_document(url, post, referer, max_bytes)
            .await
            .map(|(text, _)| text)
    }
    async fn fetch_document(
        &self,
        url: &str,
        post: Option<&str>,
        referer: Option<&str>,
        max_bytes: usize,
    ) -> Result<(String, String), SearchError> {
        let request = match post {
            Some(body) => self
                .client
                .post(url)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(body.to_owned()),
            None => self.client.get(url),
        };
        let request = if let Some(referer) = referer {
            request.header(reqwest::header::REFERER, referer)
        } else {
            request
        };
        let mut response = request.send().await.map_err(http_error)?;
        if response.status().is_redirection() {
            if let Some(magnet) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|h| h.to_str().ok())
                .filter(|h| h.starts_with("magnet:"))
            {
                return Ok((magnet.to_owned(), response.url().to_string()));
            }
        }
        response = response.error_for_status().map_err(http_error)?;
        let charset = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .split(';')
            .find_map(|p| {
                p.trim()
                    .split_once('=')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("charset"))
                    .map(|(_, v)| v.trim_matches('"').to_owned())
            })
            .unwrap_or_else(|| "utf-8".into());
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if bytes.len() + chunk.len() > max_bytes {
                return Err(runtime(format!(
                    "Source response exceeds {max_bytes} bytes"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err(runtime("Source returned an empty HTTP response"));
        }
        let encoding =
            encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
        Ok((
            encoding.decode(&bytes).0.into_owned(),
            response.url().to_string(),
        ))
    }

    async fn collect(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        if !self.descriptor.categories.contains(&request.category) {
            return Err(runtime("Source does not support the selected category"));
        }
        match self.descriptor.id.as_str() {
            "fitgirl_repacks" => self.collect_fitgirl(request, limit, results).await,
            id if advanced_sources::supports(id) => {
                self.collect_advanced_source(request, limit, results).await
            }
            id if legacy_media::supports(id) => {
                self.collect_legacy_media(request, limit, results).await
            }
            id if enriched_feeds::supports(id) => {
                self.collect_enriched_feed(request, limit, results).await
            }
            id if catalogue_tables::supports(id) => {
                self.collect_catalogue_tables(request, limit, results).await
            }
            id if game_cards::supports(id) => {
                self.collect_game_cards(request, limit, results).await
            }
            id if regional::supports(id) => self.collect_regional(request, limit, results).await,
            id if public_apis::supports(id) => {
                self.collect_public_api(request, limit, results).await
            }
            id if public_tables::supports(id) => {
                self.collect_public_tables(request, limit, results).await
            }
            id if spanish::supports(id) => self.collect_spanish(request, limit, results).await,
            id if community::supports(id) => self.collect_community(request, limit, results).await,
            "torrentscsv" | "piratebay" => {
                let (url, description) = json::search_url(&self.descriptor, request)?;
                let response = self.fetch(&url, None).await?;
                let parsed = json::parse(&response, &self.descriptor, &description, now())?;
                Ok(append(results, parsed, limit))
            }
            "jackett" => self.collect_jackett(request, limit, results).await,
            id => {
                let (pages, start, minimum) = match id {
                    "eztv" => (1, 1, 0),
                    "torrentproject" => (5, 0, 20),
                    _ => (4, 1, if id == "solidtorrents" { 15 } else { 20 }),
                };
                for page in start..start + pages {
                    let url = html::search_url(&self.descriptor, request, page)?;
                    let response = self
                        .fetch(
                            &url,
                            if id == "eztv" {
                                Some("layout=def_wlinks")
                            } else {
                                None
                            },
                        )
                        .await?;
                    let parsed = html::parse(&response, &self.descriptor, now())?;
                    let count = parsed.len();
                    if append(results, parsed, limit) {
                        return Ok(true);
                    }
                    if count < minimum {
                        break;
                    }
                }
                Ok(false)
            }
        }
    }
}

#[async_trait]
impl SearchSource for NativeSource {
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
        let timeout = if self.descriptor.id == "fitgirl_repacks" {
            limits.source_timeout.min(Duration::from_secs(40))
        } else {
            limits.source_timeout
        };
        let (status, limited, error) = tokio::select! {
            biased;
            _=cancellation.cancelled() => (SourceStatus::Cancelled,false,Some("Search cancelled".into())),
            value=tokio::time::timeout(timeout,self.collect(request,limits.results_per_source,&mut results)) => match value {
                Ok(Ok(limited))=>(SourceStatus::Completed,limited,None),
                Ok(Err(e))=>(SourceStatus::Failed,false,Some(e.to_string())),
                Err(_)=>(SourceStatus::TimedOut,false,Some("Source deadline exceeded".into())),
            }
        };
        SourceOutcome {
            report: SourceReport {
                source_id: self.descriptor.id.clone(),
                status,
                elapsed_ms: started.elapsed().as_millis() as u64,
                result_count: results.len(),
                limited,
                error,
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
        let url = url::Url::parse(uri).map_err(|e| runtime(e.to_string()))?;
        if url.path().ends_with(".torrent") {
            // Direct metadata URLs need no network resolution. The download
            // service consumes HTTP references; do not fetch payloads here.
            return Ok(reference);
        }
        let operation = async {
            let mut allowed = self.descriptor.site_url.clone();
            if matches!(self.descriptor.id.as_str(), "cpasbien" | "torrent9") {
                allowed = self.legacy_site().await?;
            }
            let site = url::Url::parse(&allowed).map_err(|e| runtime(e.to_string()))?;
            let rutor_download = self.descriptor.id == "rutor"
                && url.host_str() == Some("d.rutor.info")
                && url.path().starts_with("/download/");
            if url.origin() != site.origin() && !rutor_download {
                return Err(runtime("Reference must belong to the selected source"));
            }
            if self.descriptor.id == "zooqle" && url.path() == "/torfile/" {
                let hid = url
                    .fragment()
                    .and_then(|f| {
                        url::form_urlencoded::parse(f.as_bytes())
                            .find(|(k, _)| k == "hid")
                            .map(|(_, v)| v.into_owned())
                    })
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| runtime("Zooqle POST reference has no download identifier"))?;
                let mut result = reference.clone();
                if let TorrentReference::Http { post_form, .. } = &mut result {
                    *post_form = Some(std::collections::BTreeMap::from([("hid".into(), hid)]));
                }
                return Ok(result);
            }
            if rutor_download
                || (self.descriptor.id == "rutor" && url.path().starts_with("/download/"))
                || (self.descriptor.id == "smallgames" && url.path() == "/getTorrent.php")
                || (self.descriptor.id == "rockbox" && url.path().ends_with("download.php"))
                || (self.descriptor.id == "traht" && url.path().contains("download"))
            {
                return Ok(reference.clone());
            }
            if self.descriptor.id == "thepiratebay_community_059" {
                return self.resolve_piratebay_category(&url).await;
            }
            let mut current = uri.to_owned();
            let mut seen = std::collections::HashSet::new();
            for _ in 0..5 {
                if !seen.insert(current.clone()) {
                    return Err(runtime("Source resolver link cycle"));
                }
                let text = self.fetch(&current, None).await?;
                if text.starts_with("magnet:") {
                    return parse_reference(text.trim(), &self.descriptor.id);
                }
                if let Ok(magnet) = html::resolve_magnet(&text, &self.descriptor.id) {
                    return Ok(magnet);
                }
                let next = public_tables::detail_download(&text, &current, &self.descriptor.id)?;
                let reference = parse_reference(&next, &self.descriptor.id)?;
                if next.contains(".torrent")
                    || matches!(reference, TorrentReference::Magnet { .. })
                    || matches!(self.descriptor.id.as_str(), "cpasbien" | "torrent9")
                {
                    return Ok(reference);
                }
                current = next;
            }
            Err(runtime("Source resolver exceeded five detail pages"))
        };
        tokio::select! {
            biased;
            _=cancellation.cancelled()=>Err(runtime("Resolution cancelled")),
            value=tokio::time::timeout(limits.source_timeout,operation)=>value.map_err(|_|runtime("Resolution deadline exceeded"))?,
        }
    }
}

fn append(results: &mut Vec<SearchResult>, parsed: Vec<SearchResult>, limit: usize) -> bool {
    let available = limit.saturating_sub(results.len());
    let limited = parsed.len() >= available;
    results.extend(parsed.into_iter().take(available));
    limited
}
fn runtime(message: impl Into<String>) -> SearchError {
    SearchError::Runtime(message.into())
}
fn http_error(error: reqwest::Error) -> SearchError {
    runtime(error.without_url().to_string())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn candidate(
    descriptor: &SourceDescriptor,
    raw_title: String,
    uri: String,
    description: Option<String>,
    size: Option<u64>,
    seeders: Option<u64>,
    leechers: Option<u64>,
    published: Option<u64>,
    observed: u64,
) -> Result<SearchResult, SearchError> {
    let uri = if uri.to_ascii_lowercase().starts_with("http%3a//")
        || uri.to_ascii_lowercase().starts_with("https%3a//")
    {
        percent_encoding::percent_decode_str(&uri)
            .decode_utf8_lossy()
            .into_owned()
    } else {
        uri
    };
    let title: String = raw_title.chars().filter(|c| !c.is_control()).collect();
    if title.trim().is_empty() || title.len() > 16 * 1024 {
        return Err(runtime("Source result has an empty title"));
    }
    Ok(SearchResult {
        title: title.clone(),
        reference: parse_reference(&uri, &descriptor.id)?,
        observations: vec![SourceObservation {
            source_id: descriptor.id.clone(),
            title,
            raw_title: Some(raw_title),
            site_url: descriptor.site_url.clone(),
            description_url: description.filter(|uri| {
                matches!(
                    parse_reference(uri, &descriptor.id),
                    Ok(TorrentReference::Http { .. })
                )
            }),
            size_bytes: size,
            seeders,
            leechers,
            published_unix: published,
            observed_at_unix: observed,
        }],
    })
}

struct UnavailableNative(SourceDescriptor);
#[async_trait]
impl SearchSource for UnavailableNative {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.0
    }
    async fn search(
        &self,
        _: &SearchRequest,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> SourceOutcome {
        SourceOutcome {
            results: vec![],
            report: SourceReport {
                source_id: self.0.id.clone(),
                status: SourceStatus::Unsupported,
                elapsed_ms: 0,
                result_count: 0,
                limited: false,
                error: Some(
                    "Native Rust implementation is not available; choose Python or auto".into(),
                ),
            },
        }
    }
    async fn resolve(
        &self,
        _: &str,
        _: &SearchLimits,
        _: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        Err(runtime("Native implementation is unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str) -> SourceDescriptor {
        SourceDescriptor {
            id: id.into(),
            name: id.into(),
            site_url: "http://127.0.0.1:1".into(),
            categories: vec!["all".into()],
            resolves_http: false,
        }
    }

    #[tokio::test]
    async fn cancellation_prevents_native_network_requests() {
        let source = NativeSource::new(descriptor("torrentscsv"), Path::new(".")).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let outcome = source
            .search(
                &SearchRequest {
                    query: "fixture".into(),
                    category: "all".into(),
                    sources: vec![],
                },
                &SearchLimits::default(),
                cancellation,
            )
            .await;
        assert_eq!(outcome.report.status, SourceStatus::Cancelled);
        assert!(outcome.results.is_empty());
    }

    #[tokio::test]
    async fn unknown_native_source_is_an_explicit_per_source_failure() {
        let sources: Vec<Arc<dyn SearchSource>> =
            vec![Arc::new(UnavailableNative(descriptor("community_fixture")))];
        let sources = configure(sources, Path::new("."), SourceBackend::Rust, None).unwrap();
        let outcome = sources[0]
            .search(
                &SearchRequest {
                    query: "fixture".into(),
                    category: "all".into(),
                    sources: vec![],
                },
                &SearchLimits::default(),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(outcome.report.status, SourceStatus::Unsupported);
        assert!(outcome.report.error.unwrap().contains("Python"));
    }

    #[test]
    fn auto_keeps_a_python_only_source_available() {
        let original: Arc<dyn SearchSource> =
            Arc::new(UnavailableNative(descriptor("community_fixture")));
        let sources = configure(
            vec![original.clone()],
            Path::new("."),
            SourceBackend::Auto,
            None,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&sources[0], &original));
    }
}
