use super::{NativeSource, append, candidate, runtime};
use crate::{SearchError, SearchRequest, SearchResult, SourceDescriptor};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub(super) struct Configuration {
    pub url: String,
    api_key: String,
    #[serde(default)]
    tracker_first: bool,
    #[serde(default = "default_threads")]
    thread_count: usize,
}
fn default_threads() -> usize {
    20
}

pub(super) fn configuration(assets: &Path) -> Result<Configuration, SearchError> {
    let path = assets.join("qbit/engines/jackett.json");
    let bytes = std::fs::read(path).map_err(|_| {
        runtime("Jackett requires local qbit/engines/jackett.json with url and api_key")
    })?;
    let configuration: Configuration = serde_json::from_slice(&bytes)
        .map_err(|_| runtime("Invalid local Jackett configuration"))?;
    let url = url::Url::parse(&configuration.url).map_err(|_| runtime("Invalid Jackett URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(runtime(
            "Jackett URL must be HTTP(S) without credentials, query or fragment",
        ));
    }
    if configuration.api_key.is_empty() || configuration.api_key == "YOUR_API_KEY_HERE" {
        return Err(runtime("Jackett API key is not configured"));
    }
    if !(1..=64).contains(&configuration.thread_count) {
        return Err(runtime("Jackett thread_count must be between 1 and 64"));
    }
    Ok(configuration)
}

impl NativeSource {
    pub(super) async fn collect_jackett(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let config = configuration(&self.assets)?;
        let endpoint = |indexer: &str| -> Result<url::Url, SearchError> {
            url::Url::parse(&format!(
                "{}/api/v2.0/indexers/{indexer}/results/torznab/api",
                config.url.trim_end_matches('/')
            ))
            .map_err(|_| runtime("Invalid Jackett endpoint"))
        };
        let indexers = if config.thread_count > 1 {
            let mut url = endpoint("all")?;
            url.set_query(Some(&format!(
                "apikey={}&t=indexers&configured=true",
                super::json::form_value(&config.api_key)
            )));
            let response = self.fetch(url.as_str(), None).await?;
            let document = roxmltree::Document::parse(&response)
                .map_err(|_| runtime("Invalid Jackett indexer XML"))?;
            if document.descendants().any(|n| n.has_tag_name("error")) {
                return Err(runtime("Jackett rejected the indexer request"));
            }
            document
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("indexer"))
                .filter_map(|n| n.attribute("id"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            vec!["all".into()]
        };
        let category = match request.category.as_str() {
            "all" => None,
            "anime" => Some("5070"),
            "books" => Some("8000"),
            "games" => Some("1000,4000"),
            "movies" => Some("2000"),
            "music" => Some("3000"),
            "software" => Some("4000"),
            "tv" => Some("5000"),
            _ => return Err(runtime("Unsupported Jackett category")),
        };
        // The plugin uses one request per configured indexer. Bounded async
        // tasks preserve that behavior without launching Python worker threads.
        let mut pending = indexers.into_iter();
        let mut tasks = tokio::task::JoinSet::new();
        let mut first_error = None;
        loop {
            while tasks.len() < config.thread_count {
                let Some(indexer) = pending.next() else { break };
                if !indexer
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                {
                    return Err(runtime("Invalid Jackett indexer identifier"));
                }
                let mut url = endpoint(&indexer)?;
                let mut query = format!(
                    "apikey={}&q={}",
                    super::json::form_value(&config.api_key),
                    super::json::form_value(&request.query)
                );
                if let Some(category) = category {
                    query.push_str(&format!("&cat={}", super::json::form_value(category)));
                }
                url.set_query(Some(&query));
                let source = NativeSource {
                    descriptor: self.descriptor.clone(),
                    assets: self.assets.clone(),
                    client: self.client.clone(),
                };
                let tracker_first = config.tracker_first;
                tasks.spawn(async move {
                    let response = source.fetch(url.as_str(), None).await?;
                    parse(&response, &source.descriptor, tracker_first, super::now())
                });
            }
            let Some(outcome) = tasks.join_next().await else {
                break;
            };
            match outcome {
                Ok(Ok(parsed)) => {
                    if append(results, parsed, limit) {
                        return Ok(true);
                    }
                }
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    first_error.get_or_insert(runtime("Jackett indexer task failed"));
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(false)
    }
}

pub(super) fn parse(
    response: &str,
    source: &SourceDescriptor,
    tracker_first: bool,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let document =
        roxmltree::Document::parse(response).map_err(|_| runtime("Invalid Jackett search XML"))?;
    if document.descendants().any(|n| n.has_tag_name("error")) {
        return Err(runtime("Jackett rejected the search request"));
    }
    let mut results = vec![];
    for item in document
        .descendants()
        .filter(|node| node.has_tag_name("item"))
    {
        let value = |tag: &str| {
            item.children()
                .find(|n| n.has_tag_name(tag))
                .and_then(|n| n.text())
        };
        let attr = |name: &str| {
            item.children()
                .find(|n| {
                    n.has_tag_name(("http://torznab.com/schemas/2015/feed", "attr"))
                        && n.attribute("name") == Some(name)
                })
                .and_then(|n| n.attribute("value"))
        };
        let Some(title) = value("title") else {
            continue;
        };
        let Some(uri) = attr("magneturl").or_else(|| value("link")) else {
            continue;
        };
        let tracker = value("jackettindexer").unwrap_or("");
        let title = if tracker_first {
            format!("[{tracker}] {title}")
        } else {
            format!("{title} [{tracker}]")
        }
        .replace('|', "%7C");
        let number = |v: Option<&str>| v.and_then(|s| s.parse::<u64>().ok());
        let seeders = number(attr("seeders"));
        let peers = number(attr("peers"));
        let leechers = match (peers, seeders) {
            (Some(p), Some(s)) => p.checked_sub(s),
            _ => peers,
        };
        let published = value("pubDate")
            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
            .and_then(|d| d.timestamp().try_into().ok());
        results.push(candidate(
            source,
            title,
            uri.replace('|', "%7C"),
            value("comments")
                .or_else(|| value("guid"))
                .map(|s| s.replace('|', "%7C")),
            number(value("size")),
            seeders,
            leechers,
            published,
            now,
        )?);
    }
    Ok(results)
}
