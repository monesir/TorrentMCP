//! Regional public index layouts. Keep per-plugin quirks and field provenance.
use super::{NativeSource, candidate, html, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use chrono::TimeZone;
use scraper::{ElementRef, Html, Selector};
use std::time::Duration;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "btdig"
            | "anidex"
            | "dmhy"
            | "dmhy_community_018"
            | "mikanani"
            | "nyaapantsu"
            | "nyaa"
            | "torrentgalaxy"
            | "uindex"
            | "uniondht"
    )
}
fn sel(value: &str) -> Selector {
    Selector::parse(value).expect("Static selector")
}
fn first<'a>(node: ElementRef<'a>, selector: &str) -> Option<ElementRef<'a>> {
    node.select(&sel(selector)).next()
}
fn raw(node: ElementRef<'_>) -> String {
    node.text().collect::<String>()
}
fn field(node: ElementRef<'_>, selector: &str) -> String {
    first(node, selector)
        .map(raw)
        .unwrap_or_default()
        .trim()
        .into()
}
fn count(value: &str) -> Option<u64> {
    value.trim().replace(',', "").parse().ok()
}
fn size(value: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(&value.trim().replace(',', "").replace('\u{a0}', " "))
}
fn capture(pattern: &str, value: &str) -> Option<String> {
    regex::Regex::new(pattern)
        .ok()?
        .captures(value)?
        .get(1)
        .map(|m| m.as_str().into())
}
fn join(base: &str, href: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|u| u.join(href))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}
struct Row {
    title: String,
    uri: String,
    desc: String,
    size: Option<u64>,
    seeds: Option<u64>,
    leeches: Option<u64>,
    date: Option<u64>,
}

impl NativeSource {
    pub(super) async fn collect_regional(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let query =
            percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        let mut page = if matches!(id, "btdig" | "torrentgalaxy") {
            0
        } else {
            1
        };
        let mut total = None;
        let mut count_all = 0;
        if id == "nyaa" {
            let response = self
                .fetch(&format!("{base}/?q={query}&f=0&c=0_0"), None)
                .await?;
            total = capture(r"Displaying results 1-\d+ out of (\d+) results", &response)
                .and_then(|s| s.parse::<usize>().ok());
        }
        loop {
            let url = route(id, base, &query, &request.category, page)?;
            let response = self.fetch(&url, None).await?;
            let (rows, next) = parse(id, base, &response)?;
            if total.is_none() {
                total = match id {
                    "btdig" => capture(r">(\d+) results found", &response)
                        .and_then(|s| s.parse::<usize>().ok())
                        .map(|n| n.div_ceil(10)),
                    "anidex" => capture(r"Showing[^f]+f\s*([\d,]+)\s*torrents", &response)
                        .and_then(|s| s.replace(',', "").parse::<usize>().ok())
                        .map(|n| n.div_ceil(50)),
                    "torrentgalaxy" => capture(r"steelblue[^>]+>(.*?)<", &response)
                        .and_then(|s| s.replace(' ', "").parse::<usize>().ok())
                        .map(|n| n.div_ceil(50)),
                    "nyaa" => capture(r"Displaying results 1-\d+ out of (\d+) results", &response)
                        .and_then(|s| s.parse::<usize>().ok()),
                    "uniondht" => {
                        capture(r#"(?s)<p class=['"]floatR['"]>[^:]*:\s*(\d+)"#, &response)
                            .and_then(|s| s.parse::<usize>().ok())
                    }
                    _ => None,
                };
            }
            let length = rows.len();
            count_all += length;
            for mut row in rows {
                if row.uri.is_empty() {
                    let response = self.fetch(&row.desc, None).await?;
                    row.uri = html::resolve_magnet(&response, id)?.uri().into();
                }
                results.push(candidate(
                    &self.descriptor,
                    row.title,
                    row.uri,
                    Some(row.desc),
                    row.size,
                    row.seeds,
                    row.leeches,
                    row.date,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
            }
            let stop = match id {
                "btdig" | "torrentgalaxy" => page + 1 >= total.unwrap_or(1),
                "anidex" => page >= total.unwrap_or(1),
                "dmhy" => length < 80,
                "dmhy_community_018" => !next,
                "nyaapantsu" => length < 300,
                "nyaa" => count_all >= total.unwrap_or(length) || length == 0,
                "uniondht" => page >= 20 || page * 50 >= total.unwrap_or(0),
                _ => true,
            };
            if stop {
                break;
            }
            page += 1;
            if matches!(id, "btdig" | "anidex") {
                tokio::time::sleep(Duration::from_secs(if id == "anidex" { 2 } else { 1 })).await;
            }
        }
        Ok(false)
    }
}

fn route(
    id: &str,
    base: &str,
    query: &str,
    category: &str,
    page: usize,
) -> Result<String, SearchError> {
    Ok(match id {
        "uindex" => {
            let code = match category {
                "all" => "0",
                "movies" => "1",
                "tv" => "2",
                "games" => "3",
                "music" => "4",
                "software" => "5",
                "anime" => "7",
                _ => return Err(runtime("Unsupported UIndex category")),
            };
            format!(
                "{base}/search.php?search={}&c={code}",
                query.replace("%20", "+")
            )
        }
        "uniondht" => format!("{base}/tracker.php?nm={query}&start={}", (page - 1) * 50),
        "btdig" => {
            if page == 0 {
                format!("{base}/search?q={query}&order=0")
            } else {
                format!("{base}/search?q={query}&p={page}&order=0")
            }
        }
        "anidex" => {
            let category = match category {
                "all" => "",
                "music" => "id=9,10,11&",
                "games" => "id=12&",
                "anime" => "id=1,2,3&",
                "software" => "id=13&",
                "pictures" => "id=14&",
                "books" => "id=6,7,8&",
                _ => return Err(runtime("Unsupported AniDex category")),
            };
            let mut url = format!("{base}/?s=seeders&o=desc&{category}q={query}");
            if page > 1 {
                url.push_str(&format!("&offset={}", (page - 1) * 50));
            }
            url
        }
        "dmhy" => format!("{base}/topics/list/page/{page}?keyword={query}"),
        "dmhy_community_018" => {
            if page == 1 {
                format!("{base}/topics/list?keyword={query}")
            } else {
                format!("{base}/topics/list/page/{page}?keyword={query}")
            }
        }
        "mikanani" => format!("{base}/Home/Search?page=1&searchstr={query}&subgroupid={category}"),
        "nyaapantsu" => {
            let category = match category {
                "all" => "_",
                "anime" => "3_",
                "books" => "4_",
                "music" => "2_",
                "pictures" => "6_",
                "software" => "1_",
                "tv" | "movies" => "5_",
                _ => return Err(runtime("Unsupported Pantsu category")),
            };
            format!("{base}/search/{page}?s=0&sort=5&order=false&max=300&c={category}&q={query}")
        }
        "nyaa" => format!("{base}/?q={query}&f=0&c=0_0&p={page}"),
        "torrentgalaxy" => {
            let category = match category {
                "all" => "",
                "movies" => "c3=1&c46=1&c45=1&c42=1&c4=1&c1=1&",
                "tv" => "c41=1&c5=1&c6=1&c7=1&",
                "music" => "c23=1&c24=1&c25=1&c26=1&c17=1&",
                "games" => "c43=1&c10=1&",
                "anime" => "c28=1&",
                "software" => "c20=1&c21=1&c18=1&",
                "pictures" => "c37=1&",
                "books" => "c13=1&c19=1&c12=1&c14=1&c15=1&",
                _ => return Err(runtime("Unsupported TorrentGalaxy category")),
            };
            let mut url =
                format!("{base}/torrents.php?{category}sort=seeders&order=desc&search={query}");
            if page > 0 {
                url.push_str(&format!("&page={page}"));
            }
            url
        }
        _ => return Err(runtime("No regional source route")),
    })
}

fn parse(id: &str, base: &str, response: &str) -> Result<(Vec<Row>, bool), SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let mut rows = vec![];
    let selector = match id {
        "btdig" => ".one_result",
        "nyaapantsu" => "tr[class^='torrent-info']",
        "torrentgalaxy" => ".tgxtablerow",
        "dmhy" | "dmhy_community_018" => "#topic_list tbody tr",
        "uniondht" => "tr[id^='tor']",
        _ => "table tr",
    };
    for row in document.select(&sel(selector)) {
        let value = match id {
            "uindex" => {
                let cells: Vec<_> = row.select(&sel("td")).collect();
                if cells.len() < 6 {
                    continue;
                }
                let Some(magnet) = first(cells[1], "a[href^='magnet:']") else {
                    continue;
                };
                let mut title = String::new();
                let mut description = String::new();
                for anchor in cells[1].select(&sel("a[href]")) {
                    let href = anchor.value().attr("href").unwrap();
                    if href.starts_with("magnet:")
                        || href.contains("/details.php")
                        || href.contains("/torrent/")
                    {
                        title.push_str(&anchor.text().map(str::trim).collect::<String>());
                    }
                    if href.contains("/details.php") || href.contains("/torrent/") {
                        description = join(base, href)?;
                    }
                }
                Row {
                    title,
                    uri: magnet.value().attr("href").unwrap().into(),
                    desc: description,
                    size: size(&raw(cells[2])),
                    seeds: count(&raw(cells[4]).replace('.', "")),
                    leeches: count(&raw(cells[5]).replace('.', "")),
                    date: None,
                }
            }
            "uniondht" => {
                let (Some(title), Some(download)) = (
                    first(row, "a[href^='/topic']"),
                    first(row, "a[href^='/dl.']"),
                ) else {
                    continue;
                };
                let size = raw(download).replace('\u{a0}', "");
                Row {
                    title: first(title, "b")
                        .map(raw)
                        .unwrap_or_else(|| raw(title))
                        .trim()
                        .into(),
                    uri: format!("{base}{}", download.value().attr("href").unwrap()),
                    desc: format!("{base}{}", title.value().attr("href").unwrap()),
                    size: super::super::limetorrents::parse_size(&size),
                    seeds: count(&field(row, "td[class*='seed']")),
                    leeches: count(&field(row, "td[class*='leech'] b")),
                    date: None,
                }
            }
            "btdig" => {
                let (Some(title), Some(magnet)) = (
                    first(row, ".torrent_name a[href]"),
                    first(row, "a[href^='magnet:']"),
                ) else {
                    continue;
                };
                Row {
                    title: raw(title).trim().into(),
                    uri: magnet.value().attr("href").unwrap().into(),
                    desc: title.value().attr("href").unwrap().into(),
                    size: size(&field(row, ".torrent_size")),
                    seeds: None,
                    leeches: None,
                    date: None,
                }
            }
            "anidex" => {
                let (Some(title), Some(magnet), Some(link)) = (
                    first(row, "span.span-1440[title]"),
                    first(row, "a[href^='magnet:']"),
                    first(row, "a.torrent[href]"),
                ) else {
                    continue;
                };
                Row {
                    title: title.value().attr("title").unwrap().into(),
                    uri: magnet.value().attr("href").unwrap().into(),
                    desc: join(base, link.value().attr("href").unwrap())?,
                    size: size(&field(row, "td.text-center.td-992:not([title])")),
                    seeds: count(&field(row, "td.text-success.text-right")),
                    leeches: count(&field(row, "td.text-danger.text-right")),
                    date: None,
                }
            }
            "dmhy" | "dmhy_community_018" => {
                let cells: Vec<_> = row.select(&sel("td")).collect();
                if cells.len() < 7 {
                    continue;
                }
                let Some(link) = cells[2].select(&sel("a[href]")).last() else {
                    continue;
                };
                let magnet = cells[3]
                    .select(&sel("a[href^='magnet:']"))
                    .next()
                    .and_then(|a| a.value().attr("href"))
                    .unwrap_or("");
                let uri = if id == "dmhy" {
                    let date = raw(cells[0])
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    let hash = magnet.strip_prefix("magnet:?xt=urn:btih:").unwrap_or("");
                    format!("https://dl.dmhy.org/{date}/{hash}.torrent")
                } else {
                    magnet.into()
                };
                Row {
                    title: raw(cells[2]).replace(['\n', '\t'], ""),
                    uri,
                    desc: join(base, link.value().attr("href").unwrap())?,
                    size: size(&raw(cells[4])),
                    seeds: count(&raw(cells[5])),
                    leeches: count(&raw(cells[6])),
                    date: None,
                }
            }
            "mikanani" => {
                let cells: Vec<_> = row.select(&sel("td")).collect();
                let Some(title) = first(row, "a[class^='magnet'][target][href]") else {
                    continue;
                };
                let Some(download) = first(row, "a[href$='.torrent']") else {
                    continue;
                };
                let titlecell = cells
                    .iter()
                    .position(|cell| {
                        cell.select(&sel("a[class^='magnet'][target]"))
                            .next()
                            .is_some()
                    })
                    .unwrap_or(0);
                Row {
                    title: raw(title).trim().into(),
                    uri: format!("{base}{}", download.value().attr("href").unwrap()),
                    desc: format!("{base}{}", title.value().attr("href").unwrap()),
                    size: cells.get(titlecell + 1).and_then(|&c| size(&raw(c))),
                    seeds: None,
                    leeches: None,
                    date: None,
                }
            }
            "nyaapantsu" => {
                let (Some(title), Some(magnet)) = (
                    first(row, "a[href^='/view/']"),
                    first(row, "a[href^='magnet:']"),
                ) else {
                    continue;
                };
                Row {
                    title: raw(title).trim().into(),
                    uri: magnet.value().attr("href").unwrap().into(),
                    desc: format!("{base}{}", title.value().attr("href").unwrap()),
                    size: size(&field(row, "td[class^='tr-size']")),
                    seeds: count(&field(row, "td[class^='tr-se']")),
                    leeches: count(&field(row, "td[class^='tr-le']")),
                    date: None,
                }
            }
            "nyaa" => {
                let cells: Vec<_> = row.select(&sel("td")).collect();
                if cells.len() < 7 {
                    continue;
                }
                let (Some(title), Some(magnet)) = (
                    first(cells[1], "a[href]"),
                    cells[2].select(&sel("a[href]")).last(),
                ) else {
                    continue;
                };
                let value = raw(cells[3]);
                let bytes = if value.ends_with("GiB") {
                    value
                        .trim_end_matches("GiB")
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|n| (n * 1073741824.).round_ties_even() as u64)
                } else if value.ends_with("MiB") {
                    value
                        .trim_end_matches("MiB")
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|n| (n * 1000000.).round_ties_even() as u64)
                } else {
                    Some(0)
                };
                Row {
                    title: raw(title),
                    uri: magnet.value().attr("href").unwrap().into(),
                    desc: format!("{base}{}", title.value().attr("href").unwrap()),
                    size: bytes,
                    seeds: count(&raw(cells[5])),
                    leeches: count(&raw(cells[6])),
                    date: None,
                }
            }
            "torrentgalaxy" => {
                let (Some(title), Some(download)) = (
                    first(row, "a.txlight[title]:not([id])"),
                    first(row, "a[role='button'][href]"),
                ) else {
                    continue;
                };
                let date = field(row, "[style*='text-align:right'] small");
                let date = chrono::NaiveDateTime::parse_from_str(&date, "%d/%m/%y %H:%M")
                    .ok()
                    .and_then(|d| chrono::Local.from_local_datetime(&d).earliest())
                    .and_then(|d| d.timestamp().try_into().ok());
                Row {
                    title: title.value().attr("title").unwrap().into(),
                    uri: download.value().attr("href").unwrap().into(),
                    desc: format!("{base}{}", title.value().attr("href").unwrap_or("")),
                    size: size(&field(row, "span.badge.badge-secondary")),
                    seeds: count(&field(row, "font[color='green']")),
                    leeches: count(&field(row, "font[color='#ff0000']")),
                    date,
                }
            }
            _ => return Err(runtime("No regional source parser")),
        };
        rows.push(value);
    }
    let next = document
        .select(&sel("a[href]"))
        .any(|a| raw(a).contains("下一頁"));
    Ok((rows, next))
}
