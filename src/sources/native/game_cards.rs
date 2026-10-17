//! Public game and media catalogues with detail-page download variants.
use super::{NativeSource, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "apachetorrent" | "redetorrent" | "smallgames" | "torrenflix"
    )
}
fn sel(value: &str) -> Selector {
    Selector::parse(value).expect("Static selector")
}
fn raw(node: ElementRef<'_>) -> String {
    node.text().collect::<String>()
}
fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn join(base: &str, href: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|u| u.join(href))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}
fn title_clean(value: &str) -> String {
    regex::Regex::new(r"(?i)\s*Download\s*$")
        .unwrap()
        .replace(&clean(value), "")
        .into_owned()
}

impl NativeSource {
    pub(super) async fn collect_game_cards(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let base = self.descriptor.site_url.trim_end_matches('/');
        let id = self.descriptor.id.as_str();
        let query =
            percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        if id == "smallgames" {
            let search_base = if base == "http://small-games.info" {
                "https://small-games.info"
            } else {
                base
            };
            let response = self
                .fetch(
                    &format!("{search_base}/?go=search&go=search&search_text={query}"),
                    None,
                )
                .await?;
            let pattern = regex::Regex::new(
                r#"<a title="(.*?)"\shref="/.*?i=(\d*).*?Скачать\sигру\s\((.{2,11})\)"#,
            )
            .unwrap();
            let ascii_name = regex::Regex::new(r"[A-Za-z0-9].*").unwrap();
            for row in pattern.captures_iter(&response) {
                let Some(title) = ascii_name.find(&row[1]) else {
                    continue;
                };
                let value = row[3]
                    .chars()
                    .take(row[3].chars().count().saturating_sub(3))
                    .collect::<String>();
                results.push(candidate(
                    &self.descriptor,
                    title.as_str().into(),
                    format!("{base}/getTorrent.php?direct=1&gid={}", &row[2]),
                    Some(format!("{base}/?go=game&c=61&i={}", &row[2])),
                    super::super::limetorrents::parse_size(&format!("{value}MB")),
                    None,
                    None,
                    None,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        // These plugins re-quote qBittorrent's already encoded query.
        let quoted = query.replace("%20", "+");
        let query = if id == "torrenflix" {
            quoted
        } else {
            json::form_value(&quoted).replace("%2B", "+")
        };
        let search_url = if id == "torrenflix" {
            format!("{base}/?s={query}")
        } else {
            format!("{base}/index.php?s={query}")
        };
        let response = self.fetch(&search_url, None).await?;
        let cards: Vec<(String, String)> = {
            let doc = Html::parse_document(&response);
            let selector = match id {
                "apachetorrent" => "div.capaname",
                "redetorrent" => "div.capa_lista",
                _ => "main ul.post-lst > li",
            };
            doc.select(&sel(selector))
                .filter_map(|card| {
                    let anchor = card
                        .select(&sel(if id == "torrenflix" {
                            "a.lnk-blk"
                        } else {
                            "a[href]"
                        }))
                        .find(|a| {
                            id != "apachetorrent"
                                || a.value().attr("href").is_some_and(|h| {
                                    h.starts_with(base) && h.contains("baixar-torrent")
                                })
                        })?;
                    let href = join(&format!("{base}/"), anchor.value().attr("href")?).ok()?;
                    let title = match id {
                        "apachetorrent" => {
                            let text = raw(anchor);
                            if text.trim().is_empty() {
                                anchor.value().attr("title").unwrap_or("").into()
                            } else {
                                text
                            }
                        }
                        "redetorrent" => raw(card.select(&sel("h2[itemprop=headline]")).next()?),
                        _ => raw(card.select(&sel("h2")).next()?),
                    };
                    Some((title_clean(&title), href))
                })
                .collect()
        };
        for (title, desc) in cards {
            let response = self.fetch(&desc, None).await?;
            let variants: Vec<(String, String)> = if id == "torrenflix" {
                let doc = Html::parse_document(&response);
                let mut variants = BTreeMap::new();
                for row in doc.select(&sel("div.download-links table tbody tr")) {
                    let cells: Vec<_> = row.select(&sel("td")).collect();
                    if cells.len() < 4 {
                        continue;
                    }
                    // Server badges are decorative and excluded by the original parser.
                    let server = cells[0]
                        .children()
                        .filter_map(|n| n.value().as_text())
                        .map(|t| -> &str { t })
                        .collect::<String>();
                    let parts = [clean(&server), clean(&raw(cells[1])), clean(&raw(cells[2]))];
                    let key = parts
                        .iter()
                        .filter(|p| !p.is_empty())
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join("|");
                    if let Some(href) = cells[3]
                        .select(&sel("a[href]"))
                        .next()
                        .and_then(|a| a.value().attr("href"))
                    {
                        variants.insert(key, join(&desc, href)?);
                    }
                }
                variants
                    .into_iter()
                    .map(|(key, href)| (format!("{title} [{key}]"), href))
                    .collect()
            } else {
                let doc = Html::parse_document(&response);
                let area = if id == "apachetorrent" {
                    "div#lista_links a[href^='magnet:']"
                } else {
                    "p#lista_download a[href^='magnet:']"
                };
                doc.select(&sel(area))
                    .filter_map(|a| {
                        let uri = a.value().attr("href")?.to_owned();
                        let prefix = if id == "apachetorrent" {
                            "BAIXAR"
                        } else {
                            "DOWNLOAD"
                        };
                        let context = a
                            .parent()
                            .and_then(ElementRef::wrap)
                            .map(|p| {
                                p.children()
                                    .take_while(|n| n.id() != a.id())
                                    .filter_map(|n| n.value().as_text())
                                    .map(|t| -> &str { t })
                                    .collect::<String>()
                            })
                            .unwrap_or_default();
                        let label = clean(
                            a.value()
                                .attr("title")
                                .filter(|s| !s.is_empty())
                                .unwrap_or(&context),
                        );
                        let label = if label.to_uppercase().starts_with(&format!("{prefix} ")) {
                            clean(&label[prefix.len()..])
                        } else {
                            label
                        };
                        let label = if label.is_empty() {
                            url::Url::parse(&uri)
                                .ok()?
                                .query_pairs()
                                .find(|(k, _)| k == "dn")
                                .map(|(_, v)| clean(&v.replace('.', " ")))
                                .unwrap_or_default()
                        } else {
                            label
                        };
                        Some((
                            if label.is_empty() {
                                title.clone()
                            } else {
                                format!("{title} - {label}")
                            },
                            uri,
                        ))
                    })
                    .collect()
            };
            for (title, uri) in variants {
                results.push(candidate(
                    &self.descriptor,
                    title,
                    uri,
                    Some(desc.clone()),
                    if id == "torrenflix" { Some(0) } else { None },
                    None,
                    None,
                    None,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
