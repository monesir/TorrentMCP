//! FitGirl WordPress search, detail cache, magnet enrichment and tracker stats.
use super::{NativeSource, candidate, html, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult, TorrentReference};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};
pub(super) const TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.dler.org:6969/announce",
];
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("Static selector")
}
fn clean(s: &str) -> String {
    s.replace('|', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn raw(n: ElementRef<'_>) -> String {
    n.text().collect::<String>()
}
fn flag(s: &str) -> bool {
    !std::env::var(s)
        .is_ok_and(|v| ["0", "false", "no", "off"].contains(&v.to_lowercase().as_str()))
}
fn join(base: &str, href: &str) -> Option<String> {
    url::Url::parse(base)
        .ok()?
        .join(href)
        .ok()
        .map(String::from)
}
fn quote(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, html::QUERY_SAFE)
        .to_string()
        .replace('/', "%2F")
}
#[derive(Clone, Serialize, Deserialize)]
struct Post {
    title: String,
    page: String,
    magnet: Option<String>,
    torrent: Option<String>,
    hash: Option<String>,
    bytes: Option<u64>,
    date: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct Cached {
    saved: u64,
    post: Post,
}
fn bytes(s: &str) -> Option<u64> {
    let re = regex::Regex::new(
        r"(?i)Repack\s+Size[^\d]*?(\d[\d.,]*)\s*(?:/\s*\d[\d.,]*\s*)?([KMGT]?)i?B\b",
    )
    .unwrap();
    let c = re.captures(s)?;
    let mut n = c[1].trim_end_matches(['.', ',']).to_owned();
    if n.contains(',') && n.contains('.') {
        if n.rfind(',') > n.rfind('.') {
            n = n.replace('.', "").replace(',', ".")
        } else {
            n = n.replace(',', "")
        }
    } else if n.contains(',') {
        if regex::Regex::new(r"^\d{1,3}(?:,\d{3})+$")
            .unwrap()
            .is_match(&n)
        {
            n = n.replace(',', "")
        } else {
            n = n.replace(',', ".")
        }
    } else if n.matches('.').count() > 1 {
        n = n.replace('.', "")
    }
    let power = match c[2].to_uppercase().as_str() {
        "K" => 1,
        "M" => 2,
        "G" => 3,
        "T" => 4,
        _ => 0,
    };
    let n = n.parse::<f64>().ok()? * 1024f64.powi(power);
    (n.is_finite() && n >= 0. && n <= u64::MAX as f64).then_some(n as u64)
}
fn timestamp(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
                .ok()
                .map(|d| d.and_utc().timestamp())
        })
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|d| d.and_utc().timestamp())
        })?
        .try_into()
        .ok()
}
fn hash(magnet: &str) -> Option<String> {
    match crate::reference::parse_reference(magnet, "fitgirl_repacks").ok()? {
        TorrentReference::Magnet { info_hash_v1, .. } => info_hash_v1,
        _ => None,
    }
}
fn extract_hash(s: &str) -> Option<String> {
    let re=regex::Regex::new(r"(?i)(?:torrage\.info/torrent\.php\?h=|itorrents\.org/torrent/|btcache\.me/torrent/|infohash=|\bbtih:)([a-f0-9]{40}|[a-z2-7]{32})").unwrap();
    for c in re.captures_iter(s) {
        if let Some(h) = hash(&format!("magnet:?xt=urn:btih:{}", &c[1])) {
            return Some(h);
        }
    }
    let re = regex::Regex::new(r"[a-fA-F0-9]{40}").unwrap();
    for m in re.find_iter(s) {
        let before = s[..m.start()].chars().next_back();
        let after = s[m.end()..].chars().next();
        if !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_alphanumeric())
        {
            return Some(m.as_str().to_lowercase());
        }
    }
    None
}
fn parse(response: &str, base: &str, query: &str, standalone: bool) -> Vec<Post> {
    let doc = Html::parse_document(response);
    let articles: Vec<_> = doc.select(&sel("article")).collect();
    let nodes = if articles.is_empty() && standalone {
        vec![doc.root_element()]
    } else {
        articles
    };
    let mut out = Vec::new();
    for article in nodes
        .into_iter()
        .take(if standalone { 1 } else { usize::MAX })
    {
        let title_node = article
            .select(&sel("h1.entry-title, h2.entry-title"))
            .next();
        let title = title_node.map(raw).unwrap_or_else(|| {
            doc.select(&sel("meta[property='og:title']"))
                .next()
                .and_then(|n| n.value().attr("content"))
                .unwrap_or("")
                .into()
        });
        let mut title = clean(&title);
        let lower = title.to_lowercase();
        if title.is_empty()
            || ["faq", "donate"].contains(&lower.as_str())
            || ["updates digest", "updates list", "day of requests"]
                .iter()
                .any(|p| lower.starts_with(p))
            || article
                .select(&sel("a[rel='category tag']"))
                .any(|n| raw(n).to_lowercase() == "updates digest")
        {
            continue;
        }
        let compact = |s: &str| {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
                .to_lowercase()
        };
        if !compact(query).is_empty()
            && compact(&title).contains(&compact(query))
            && !title.to_lowercase().contains(&query.to_lowercase())
        {
            title = format!("{title} [{}]", clean(query));
        }
        let page = title_node
            .and_then(|n| n.select(&sel("a[href]")).next())
            .and_then(|n| n.value().attr("href"))
            .and_then(|s| join(base, s))
            .or_else(|| {
                doc.select(&sel("link[rel=canonical][href]"))
                    .next()
                    .and_then(|n| n.value().attr("href"))
                    .and_then(|s| join(base, s))
            })
            .unwrap_or_else(|| base.into());
        let links = article
            .select(&sel("a[href]"))
            .filter_map(|n| n.value().attr("href"))
            .collect::<Vec<_>>();
        let magnet = links
            .iter()
            .find(|h| h.to_lowercase().starts_with("magnet:?"))
            .map(|s| s.to_string());
        let torrent = links
            .iter()
            .find(|h| {
                regex::Regex::new(r"(?i)\.torrent(?:\?|$)")
                    .unwrap()
                    .is_match(h)
            })
            .and_then(|s| join(&page, s));
        // The Python parser inspects link targets and visible text, never image
        // URLs or CSS attributes (an avatar checksum is not a torrent hash).
        let hash_material = format!(
            "{} {}",
            links.join(" "),
            article.text().collect::<Vec<_>>().join(" ")
        );
        let infohash = magnet
            .as_deref()
            .and_then(hash)
            .or_else(|| extract_hash(&hash_material));
        let date = article
            .select(&sel("time.entry-date"))
            .next()
            .and_then(|n| {
                n.value()
                    .attr("datetime")
                    .or_else(|| n.value().attr("data-datetime"))
            })
            .and_then(timestamp);
        out.push(Post {
            title,
            page,
            magnet,
            torrent,
            hash: infohash,
            bytes: bytes(&article.text().collect::<Vec<_>>().join(" ")),
            date,
        });
    }
    out
}
fn merge(target: &mut Post, from: Post) {
    if target.magnet.is_none() {
        target.magnet = from.magnet;
    }
    if target.torrent.is_none() {
        target.torrent = from.torrent;
    }
    if target.hash.is_none() {
        target.hash = from.hash;
    }
    if target.bytes.is_none() {
        target.bytes = from.bytes;
    }
    if target.date.is_none() {
        target.date = from.date;
    }
}
fn enrich(post: &mut Post) {
    if post.magnet.is_none() {
        post.magnet = post
            .hash
            .as_ref()
            .map(|h| format!("magnet:?xt=urn:btih:{h}&dn={}", quote(&post.title)));
    }
    if let Some(magnet) = &mut post.magnet {
        let (base, fragment) = magnet
            .split_once('#')
            .map(|(a, b)| (a.to_owned(), format!("#{b}")))
            .unwrap_or((magnet.clone(), String::new()));
        let existing = url::Url::parse(&base)
            .ok()
            .map(|u| {
                u.query_pairs()
                    .filter(|(k, _)| k.eq_ignore_ascii_case("tr"))
                    .map(|(_, v)| v.into_owned())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let mut value = base;
        for tracker in TRACKERS {
            if !existing.contains(*tracker) {
                value.push_str(&format!("&tr={}", quote(tracker)));
            }
        }
        *magnet = format!("{value}{fragment}");
    }
}
impl NativeSource {
    async fn flush_fitgirl(
        &self,
        resolved: &mut Vec<(usize, Post)>,
        seen: &mut HashSet<String>,
        cache: &mut BTreeMap<String, Cached>,
        cache_path: &std::path::Path,
        cap: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let mut fresh = Vec::new();
        for (_, post) in resolved.drain(..) {
            let Some(uri) = post.magnet.as_ref().or(post.torrent.as_ref()) else {
                continue;
            };
            let key = post.hash.clone().unwrap_or_else(|| uri.clone());
            if !seen.insert(key) {
                continue;
            }
            if flag("FITGIRL_CACHE") {
                cache.insert(
                    post.page.clone(),
                    Cached {
                        saved: now(),
                        post: post.clone(),
                    },
                );
            }
            fresh.push(post);
        }
        if flag("FITGIRL_CACHE") {
            while cache.len() > 500 {
                let key = cache
                    .iter()
                    .min_by_key(|(_, v)| v.saved)
                    .map(|(k, _)| k.clone())
                    .unwrap();
                cache.remove(&key);
            }
            if let Some(parent) = cache_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let tmp = cache_path.with_extension(format!("{}.tmp", std::process::id()));
            tokio::fs::write(&tmp, serde_json::to_vec(&*cache)?).await?;
            tokio::fs::rename(tmp, &cache_path).await?;
        }
        let stats = if flag("FITGIRL_SCRAPE") {
            self.scrape_trackers(
                &fresh
                    .iter()
                    .filter_map(|p| p.magnet.clone())
                    .collect::<Vec<_>>(),
                &fresh
                    .iter()
                    .filter_map(|p| p.hash.clone())
                    .collect::<Vec<_>>(),
            )
            .await
        } else {
            BTreeMap::new()
        };
        for post in fresh {
            let peers = post.hash.as_ref().and_then(|h| stats.get(h));
            results.push(candidate(
                &self.descriptor,
                post.title,
                post.magnet.or(post.torrent).unwrap(),
                Some(post.page),
                post.bytes,
                peers.map(|p| p.0),
                peers.map(|p| p.1),
                post.date,
                now(),
            )?);
            if results.len() >= cap {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) async fn collect_fitgirl(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let cap = limit.min(30);
        let base = format!("{}/", self.descriptor.site_url.trim_end_matches('/'));
        let q = percent_encoding::utf8_percent_encode(request.query.trim(), html::QUERY_SAFE)
            .to_string();
        let cache_path = super::enriched_feeds::cache_file("fitgirl-posts.json");
        let cache_enabled = flag("FITGIRL_CACHE");
        let mut cache: BTreeMap<String, Cached> = if cache_enabled {
            tokio::fs::read_to_string(&cache_path)
                .await
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        cache.retain(|_, v| now().saturating_sub(v.saved) <= 3 * 86400);
        let mut seen = HashSet::new();
        for page in 1..=3 {
            let url = if page == 1 {
                format!("{base}?s={q}")
            } else {
                format!("{base}page/{page}/?s={q}")
            };
            let mut fetched = None;
            let mut error = None;
            for attempt in 0..2 {
                match tokio::time::timeout(
                    Duration::from_secs(8),
                    self.fetch_document(&url, None, None, 4 * 1024 * 1024),
                )
                .await
                {
                    Ok(Ok(s)) => {
                        fetched = Some(s);
                        break;
                    }
                    Ok(Err(e)) => error = Some(e),
                    Err(_) => error = Some(runtime("FitGirl search request timed out")),
                }
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
            let (response, final_url) =
                fetched.ok_or_else(|| error.unwrap_or_else(|| runtime("FitGirl search failed")))?;
            let standalone =
                !url::Url::parse(&final_url).is_ok_and(|u| u.query_pairs().any(|(k, _)| k == "s"));
            let last = if standalone {
                1
            } else {
                regex::Regex::new(r#"class=["'][^"']*page-numbers[^"']*["'][^>]*>\s*(\d+)\s*</a>"#)
                    .unwrap()
                    .captures_iter(&response)
                    .filter_map(|c| c[1].parse::<usize>().ok())
                    .max()
                    .unwrap_or(1)
                    .min(3)
            };
            let candidates = parse(&response, &final_url, &request.query, standalone);
            if candidates.is_empty() {
                break;
            }
            let mut tasks = tokio::task::JoinSet::new();
            let mut iterator = candidates
                .into_iter()
                .take(cap.saturating_sub(results.len()))
                .enumerate();
            let mut resolved = Vec::new();
            let streaming = flag("FITGIRL_STREAMING");
            let scrape = flag("FITGIRL_SCRAPE");
            let mut batch_started = tokio::time::Instant::now();
            loop {
                while tasks.len() < 8 {
                    let Some((index, mut post)) = iterator.next() else {
                        break;
                    };
                    if let Some(cached) = cache.get(&post.page) {
                        merge(&mut post, cached.post.clone());
                    }
                    let source = NativeSource {
                        descriptor: self.descriptor.clone(),
                        client: self.client.clone(),
                        assets: self.assets.clone(),
                    };
                    let query = request.query.clone();
                    tasks.spawn(async move {
                        if post.magnet.is_none() && post.hash.is_none() {
                            if let Ok(Ok((details, final_url))) = tokio::time::timeout(
                                Duration::from_secs(6),
                                source.fetch_document(&post.page, None, None, 4 * 1024 * 1024),
                            )
                            .await
                            {
                                if let Some(parsed) =
                                    parse(&details, &final_url, &query, true).into_iter().next()
                                {
                                    merge(&mut post, parsed);
                                }
                            }
                        }
                        enrich(&mut post);
                        (index, post)
                    });
                }
                if tasks.is_empty() {
                    break;
                }
                let next = if streaming && !resolved.is_empty() {
                    tokio::time::timeout_at(
                        batch_started + Duration::from_millis(1500),
                        tasks.join_next(),
                    )
                    .await
                    .ok()
                    .flatten()
                } else {
                    tasks.join_next().await
                };
                if let Some(value) = next {
                    if resolved.is_empty() {
                        batch_started = tokio::time::Instant::now();
                    }
                    resolved.push(value.map_err(|e| runtime(e.to_string()))?);
                }
                if streaming
                    && !resolved.is_empty()
                    && (!scrape
                        || resolved.len() >= 6
                        || batch_started.elapsed() >= Duration::from_millis(1500))
                {
                    if self
                        .flush_fitgirl(
                            &mut resolved,
                            &mut seen,
                            &mut cache,
                            &cache_path,
                            cap,
                            results,
                        )
                        .await?
                    {
                        tasks.abort_all();
                        return Ok(true);
                    }
                }
            }
            if !flag("FITGIRL_STREAMING") {
                resolved.sort_by_key(|(i, _)| *i);
            }
            if self
                .flush_fitgirl(
                    &mut resolved,
                    &mut seen,
                    &mut cache,
                    &cache_path,
                    cap,
                    results,
                )
                .await?
            {
                return Ok(true);
            }
            if page >= last
                || url::Url::parse(&final_url)
                    .ok()
                    .zip(url::Url::parse(&base).ok())
                    .is_some_and(|(a, b)| a.host_str() != b.host_str())
            {
                break;
            }
        }
        Ok(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_repack_units_and_only_repack_size() {
        assert_eq!(
            bytes("Original Size: 99 GB Repack Size: 1,5 / 2 GB"),
            Some(1610612736)
        );
        assert_eq!(bytes("Original Size: 99 GB"), None);
        assert_eq!(bytes("Repack Size: 1.234,5 KB"), Some(1264128));
    }
    #[test]
    fn image_checksums_do_not_become_torrent_identities() {
        let page = "<article><h2 class='entry-title'><a href='/post'>Fixture</a></h2><img src='/avatar/1234567890abcdef1234567890abcdef12345678'></article>";
        let posts = parse(page, "https://example.org/?s=fixture", "fixture", false);
        assert_eq!(posts.len(), 1);
        assert!(posts[0].hash.is_none());
        let page=page.replace("</article>","<a href='https://itorrents.org/torrent/1234567890abcdef1234567890abcdef12345678.torrent'>file</a></article>");
        assert_eq!(
            parse(&page, "https://example.org/?s=fixture", "fixture", false)[0]
                .hash
                .as_deref(),
            Some("1234567890abcdef1234567890abcdef12345678")
        );
    }
}
