//! Shared result-card layouts in the reviewed BurningMop public plugins.
//! Source request methods and pagination remain explicit rather than inferred.
use super::{NativeSource, candidate, html, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use scraper::{Html, Selector};
use std::time::Duration;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "divxtotal"
            | "dontorrent"
            | "esmeraldatorrent"
            | "naranjatorrent"
            | "tomadivx"
            | "calidadtorrent"
            | "pediatorrent"
    )
}

struct Detail {
    title: Option<String>,
    url: String,
}
fn text(element: scraper::ElementRef<'_>) -> String {
    element.text().collect::<String>()
}
fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("Static selector")
}
fn joined(base: &str, href: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|u| u.join(href))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}

impl NativeSource {
    pub(super) async fn collect_spanish(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let card = !matches!(id, "calidadtorrent" | "pediatorrent");
        let encoded =
            percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        let query = if card {
            encoded
        } else {
            encoded.replace("%20", "+")
        };
        let mut pages = 1;
        let mut page = 1;
        loop {
            let (route, body) = if id == "dontorrent" {
                (
                    format!("{base}/buscar"),
                    Some(format!("valor={query}&Buscar=Buscar&p={page}")),
                )
            } else if card {
                // Originals contain a trailing slash followed by /buscar.
                (format!("{base}//buscar/{query}/page/{page}"), None)
            } else if id == "calidadtorrent" {
                (format!("{base}/buscar/page/{page}?q={query}"), None)
            } else {
                let category = match request.category.as_str() {
                    "all" | "movies" => "peliculas",
                    "tv" => "series",
                    "books" => "documentales",
                    _ => return Err(runtime("Unsupported PediaTorrent category")),
                };
                (format!("{base}/{category}?query={query}"), None)
            };
            let response = self
                .fetch_referer(
                    &route,
                    body.as_deref(),
                    Some(if id == "dontorrent" { &route } else { base }),
                )
                .await?;
            if response.contains("No se ha encontrado ningún resultado")
                || response.contains("No se ha encontrado ningun resultado")
            {
                return Ok(false);
            }
            if page == 1 && card {
                let pattern =
                    super::static_regex(r"(?s)Se han encontrado.*?<b>(\d+)</b>.*?resultados");
                let Some(count) = pattern
                    .captures(&response)
                    .and_then(|m| m[1].parse::<usize>().ok())
                else {
                    if id == "dontorrent" {
                        return Ok(false);
                    }
                    return Err(runtime("Spanish source result count is missing"));
                };
                pages = count.div_ceil(10);
            }
            let links = if card {
                card_details(&response, base)?
            } else {
                overlay_details(&response, id, &request.category, base)?
            };
            let empty = links.is_empty();
            for detail in links {
                let response = self.fetch_referer(&detail.url, None, Some(base)).await?;
                let (title, download, size) = parse_detail(&response, base, detail.title)?;
                results.push(candidate(
                    &self.descriptor,
                    title,
                    download,
                    Some(detail.url),
                    size,
                    None,
                    None,
                    None,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
            }
            if id == "pediatorrent" || (card && page >= pages) || empty {
                return Ok(false);
            }
            page += 1;
            if card {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

fn card_details(response: &str, base: &str) -> Result<Vec<Detail>, SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let mut details = vec![];
    for row in document.select(&selector("#buscador .card .card-body p:not([class])")) {
        let Some(link) = row.select(&selector("span a[href]")).next() else {
            continue;
        };
        let mut title = text(link);
        if let Some(container) = row.select(&selector("span")).next() {
            for decoration in container.select(&selector("span")) {
                let value = text(decoration);
                if decoration.value().classes().any(|c| c.contains("badge")) {
                    title.push_str(&format!(" [{value}]"));
                } else if decoration.value().attr("class").is_none_or(str::is_empty) {
                    title.push_str(&format!(" ({value})"));
                }
            }
        }
        details.push(Detail {
            title: Some(title),
            url: joined(base, link.value().attr("href").unwrap())?,
        });
    }
    Ok(details)
}

fn overlay_details(
    response: &str,
    id: &str,
    category: &str,
    base: &str,
) -> Result<Vec<Detail>, SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let container = if id == "calidadtorrent" {
        ".result-list [x-data='{ showDetail: true }'] .relative a[href]"
    } else {
        match category {
            "all" | "movies" => ".movie-list [x-data='{ showDetail: false }'] .relative a[href]",
            "tv" => ".serie-list [x-data='{ showDetail: false }'] .relative a[href]",
            _ => ".doc-list [x-data='{ showDetail: false }'] .relative a[href]",
        }
    };
    document
        .select(&selector(container))
        .map(|link| {
            Ok(Detail {
                title: None,
                url: joined(base, link.value().attr("href").unwrap())?,
            })
        })
        .collect()
}

fn parse_detail(
    response: &str,
    base: &str,
    title: Option<String>,
) -> Result<(String, String, Option<u64>), SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let card = title.is_some();
    let title = title
        .or_else(|| document.select(&selector("h1")).next().map(text))
        .ok_or_else(|| runtime("Spanish source detail title missing"))?;
    let download = document
        .select(&selector("a[href]"))
        .filter_map(|a| a.value().attr("href"))
        .find(|href| href.ends_with(".torrent"))
        .map(|s| {
            if card && s.starts_with("//") {
                Ok(format!("https:{s}"))
            } else {
                joined(base, s)
            }
        })
        .transpose()?
        .ok_or_else(|| runtime("Spanish source detail has no torrent URL"))?;
    let size = if card {
        document.select(&selector("p")).find_map(|p| {
            let value = text(p);
            value.split_once("Tamaño:").and_then(|(_, value)| {
                super::super::limetorrents::parse_size(&value.trim().replace(',', "."))
            })
        })
    } else {
        Some(0)
    };
    Ok((title, download, size))
}
