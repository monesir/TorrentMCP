//! Source-specific public HTML tables, with bounded pagination and pacing.
use super::{NativeSource, candidate, html, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use chrono::TimeZone;
use scraper::{ElementRef, Html, Selector};
use std::{collections::HashSet, time::Duration};

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "glotorrents"
            | "kickasstorrents"
            | "pirateiro"
            | "rockbox"
            | "torrentdownload"
            | "yourbittorrent"
            | "magnetdl"
            | "magnetdl_community_031"
            | "solidtorrents_community_053"
            | "torrentdownload_community_066"
    )
}
struct Row {
    title: String,
    uri: String,
    description: String,
    size: Option<u64>,
    seeds: Option<u64>,
    leeches: Option<u64>,
    date: Option<u64>,
    detail: bool,
}
fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("Static selector")
}
fn first<'a>(node: ElementRef<'a>, value: &str) -> Option<ElementRef<'a>> {
    node.select(&selector(value)).next()
}
fn text(node: ElementRef<'_>) -> String {
    node.text().collect::<String>().trim().to_owned()
}
fn field(node: ElementRef<'_>, value: &str) -> String {
    first(node, value).map(text).unwrap_or_default()
}
fn count(value: &str) -> Option<u64> {
    value.trim().replace(',', "").parse().ok()
}
fn size(value: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(&value.replace(',', ""))
}
fn row_size(row: ElementRef<'_>) -> Option<u64> {
    row.select(&selector("td")).find_map(|cell| {
        capture(r"(?i)^([\d,.]+\s*[TGMK]B)$", &text(cell))
            .as_deref()
            .and_then(size)
    })
}
fn join(base: &str, href: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|u| u.join(href))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}
fn quote(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, html::QUERY_SAFE).to_string()
}
fn date(value: &str) -> Option<u64> {
    let date = chrono::NaiveDate::parse_from_str(value, "%d/%m/%Y")
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&date)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}
fn capture(pattern: &str, value: &str) -> Option<String> {
    regex::Regex::new(pattern)
        .ok()?
        .captures(value)?
        .get(1)
        .map(|m| m.as_str().to_owned())
}

impl NativeSource {
    pub(super) async fn collect_public_tables(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let encoded = quote(&request.query);
        let query = if matches!(id, "magnetdl" | "magnetdl_community_031") {
            encoded.to_lowercase().replace("%20", "-").replace('+', "-")
        } else if id == "yourbittorrent" {
            encoded.replace("%20", "-")
        } else if id == "kickasstorrents" {
            encoded
        } else {
            encoded.replace("%20", "+")
        };
        let start: usize = if matches!(id, "glotorrents" | "rockbox" | "kickasstorrents") {
            0
        } else {
            1
        };
        let cap = match id {
            "pirateiro" => 9,
            "torrentdownload" => 10,
            "yourbittorrent" => 1,
            "magnetdl" | "magnetdl_community_031" => 30,
            "torrentdownload_community_066" => 9,
            _ => usize::MAX,
        };
        let mut seen = HashSet::new();
        let mut total = None;
        for page in start..start.saturating_add(cap) {
            let url = route(id, base, &query, &request.category, page)?;
            let response = self.fetch(&url, None).await?;
            static WHITESPACE: std::sync::LazyLock<regex::Regex> =
                std::sync::LazyLock::new(|| regex::Regex::new(r"\s+").unwrap());
            let normalized = WHITESPACE.replace_all(&response, " ");
            let parsed = parse(id, base, &normalized, &request.category, &mut seen)?;
            let count = parsed.len();
            if total.is_none() {
                total = match id {
                    "torrentdownload" => {
                        capture(r"<h1>[\d,]+ - [\d,]+ of ([\d,]+) for", &normalized)
                            .and_then(|s| s.replace(',', "").parse().ok())
                    }
                    "magnetdl" => {
                        capture(r"Found <strong>([\d,]+)</strong> Magnet Links", &normalized)
                            .and_then(|s| s.replace(',', "").parse().ok())
                    }
                    "solidtorrents_community_053" => {
                        capture(r"<b>(\d+)</b>", &normalized).and_then(|s| s.parse().ok())
                    }
                    _ => None,
                };
            }
            for mut row in parsed {
                if row.detail {
                    let response = self.fetch(&row.description, None).await?;
                    row.uri = html::resolve_magnet(&response, id)?.uri().to_owned();
                }
                results.push(candidate(
                    &self.descriptor,
                    row.title,
                    row.uri,
                    Some(row.description),
                    row.size,
                    row.seeds,
                    row.leeches,
                    row.date,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
                if id == "kickasstorrents" {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
            if (count == 0 && id != "torrentdownload_community_066")
                || total.is_some_and(|total| seen.len().max(results.len()) >= total)
            {
                break;
            }
            if id == "magnetdl_community_031" && !normalized.contains("Next Page ") {
                break;
            }
            let delay = match id {
                "glotorrents" | "rockbox" => 3,
                "torrentdownload" => 1,
                _ => 0,
            };
            if delay > 0 {
                tokio::time::sleep(Duration::from_secs(delay)).await;
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
        "glotorrents" => {
            let category = match category {
                "all" => "0",
                "movies" => "1",
                "tv" => "41",
                "music" => "22",
                "games" => "10",
                "anime" => "28",
                "software" => "18",
                "books" => "51",
                "pictures" => "70",
                _ => return Err(runtime("Unsupported GloTorrents category")),
            };
            format!(
                "{base}/search_results.php?search={query}&cat={category}&page={page}&incldead=0&inclexternal=0"
            )
        }
        "rockbox" => {
            format!("{base}/torrents.php?active=0&search={query}&options=0&order=data&page={page}")
        }
        "kickasstorrents" => {
            let category = if category == "all" {
                String::new()
            } else {
                format!(
                    "category/{}/",
                    if category == "software" {
                        "apps"
                    } else {
                        category
                    }
                )
            };
            format!("{base}/search/{query}/{category}{page}/")
        }
        "pirateiro" => {
            let suffix = match category {
                "all" => "",
                "anime" => "&category=2",
                "games" => "&category=3",
                "movies" => "&category=1",
                "music" => "&category=4",
                "software" => "&category=6",
                "tv" => "&category=5",
                _ => return Err(runtime("Unsupported Pirateiro category")),
            };
            format!("{base}/search?query={query}&page={page}{suffix}")
        }
        "torrentdownload" | "torrentdownload_community_066" => {
            format!("{base}/search?q={query}&p={page}")
        }
        "yourbittorrent" => {
            let suffix = match category {
                "all" => "",
                "movies" => "&c=1",
                "tv" => "&c=3",
                "music" => "&c=2",
                "games" => "&c=4",
                "anime" => "&c=6",
                "software" => "&c=5",
                _ => return Err(runtime("Unsupported YourBittorrent category")),
            };
            format!("{base}/?q={query}{suffix}")
        }
        "magnetdl" => format!(
            "{base}/{}/{query}/{page}",
            query.chars().next().ok_or_else(|| runtime("Empty query"))?
        ),
        "magnetdl_community_031" => format!(
            "{base}/{}/{query}/se/desc/{page}/",
            query.chars().next().ok_or_else(|| runtime("Empty query"))?
        ),
        "solidtorrents_community_053" => format!("{base}/search?q={query}&page={page}"),
        _ => return Err(runtime("No public table route")),
    })
}

fn parse(
    id: &str,
    base: &str,
    response: &str,
    category: &str,
    seen: &mut HashSet<String>,
) -> Result<Vec<Row>, SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let mut results = vec![];
    let rows = match id {
        "glotorrents" => "tr.t-row",
        "kickasstorrents" => "tr.odd, tr.even",
        "yourbittorrent" => ".table-responsive tr.table-default",
        "solidtorrents_community_053" => "li.search-result",
        _ => "tr",
    };
    let selected: Vec<_> = if id == "yourbittorrent" {
        document
            .select(&selector(".table-responsive"))
            .nth(1)
            .map(|table| table.select(&selector("tr.table-default")).collect())
            .unwrap_or_default()
    } else {
        document.select(&selector(rows)).collect()
    };
    for row in selected {
        let value = match id {
            "torrentdownload_community_066" => {
                let Some(title) = first(row, "td.tdleft .tt-name a[href]") else {
                    continue;
                };
                let tags = first(row, ".smallish")
                    .map(|n| n.text().collect::<String>())
                    .unwrap_or_default();
                let pattern = match category {
                    "all" => "",
                    "anime" => "Anime|Animation|Hentai",
                    "books" => "book",
                    "games" => "game",
                    "movies" => "movie",
                    "music" => "music",
                    "pictures" => "Picture|Wallpaper",
                    "software" => "Application|Software|Windows|Linux|Android|PC",
                    "tv" => "TV|Television",
                    _ => return Err(runtime("Unknown TorrentDownload category")),
                };
                if !pattern.is_empty()
                    && !pattern
                        .split('|')
                        .any(|word| tags.to_lowercase().contains(&word.to_lowercase()))
                {
                    continue;
                }
                let uri = format!("{base}{}", title.value().attr("href").unwrap_or(""));
                Row {
                    // The category label (" » TV shows") is not part of the release name.
                    title: text(title),
                    uri: uri.clone(),
                    description: uri,
                    size: row_size(row),
                    seeds: count(&field(row, ".tdseed")),
                    leeches: count(&field(row, ".tdleech")),
                    date: None,
                    detail: false,
                }
            }
            "pirateiro" => {
                let Some(link) = row
                    .select(&selector("a[href] > h6"))
                    .next()
                    .and_then(|h| h.parent().and_then(ElementRef::wrap))
                else {
                    continue;
                };
                let href = link.value().attr("href").unwrap_or("");
                if !href.contains("/torrent/") {
                    continue;
                }
                let uri = join(base, href)?;
                Row {
                    title: text(link),
                    uri: uri.clone(),
                    description: uri,
                    size: None,
                    seeds: count(&field(row, ".btn-seed-home")),
                    leeches: count(&field(row, ".btn-leech-home")),
                    date: None,
                    detail: false,
                }
            }
            "glotorrents" => {
                let (Some(title), Some(magnet)) = (
                    first(row, "a[title][href]:not([href^='magnet:'])"),
                    first(row, "a[href^='magnet:']"),
                ) else {
                    continue;
                };
                let peers: Vec<_> = row.select(&selector("font b")).map(text).collect();
                Row {
                    title: title.value().attr("title").unwrap_or("").into(),
                    uri: magnet.value().attr("href").unwrap().into(),
                    description: format!("{base}/{}", title.value().attr("href").unwrap()),
                    size: row_size(row),
                    seeds: peers.first().and_then(|s| count(s)),
                    leeches: peers.get(1).and_then(|s| count(s)),
                    date: None,
                    detail: false,
                }
            }
            "kickasstorrents" => {
                let Some(title) = first(row, ".torrentname a.cellMainLink") else {
                    continue;
                };
                let description = format!("{base}/{}", title.value().attr("href").unwrap_or(""));
                Row {
                    title: text(title),
                    uri: description.clone(),
                    description,
                    size: row_size(row),
                    seeds: count(&field(row, "td.green.center")),
                    leeches: count(&field(row, "td.red.lasttd.center")),
                    date: None,
                    detail: true,
                }
            }
            "magnetdl" | "magnetdl_community_031" => {
                let (Some(title), Some(magnet)) =
                    (first(row, "td.n a"), first(row, "a[href^='magnet:']"))
                else {
                    continue;
                };
                let cells: Vec<_> = row.select(&selector("td")).collect();
                if cells.len() < 8 {
                    continue;
                }
                let kind = text(cells[3]);
                if id.ends_with("031") && category != "all" {
                    let wanted = match category {
                        "books" => "E-Book",
                        "games" => "Game",
                        "movies" => "Movie",
                        "music" => "Music",
                        "software" => "Software",
                        "tv" => "TV",
                        _ => "",
                    };
                    if !kind.eq_ignore_ascii_case(wanted) {
                        continue;
                    }
                }
                let title = if id == "magnetdl" {
                    title.value().attr("title").unwrap_or("").replace('|', "")
                } else {
                    format!("{} » {kind}", text(title))
                };
                Row {
                    title,
                    uri: magnet.value().attr("href").unwrap().into(),
                    description: join(
                        base,
                        first(row, "td.n a").unwrap().value().attr("href").unwrap(),
                    )?,
                    size: size(&text(cells[5])),
                    seeds: count(&field(row, "td.s")),
                    leeches: count(&field(row, "td.l")),
                    date: None,
                    detail: false,
                }
            }
            "torrentdownload" => {
                if row.value().attr("bgcolor").is_some() {
                    continue;
                }
                // The live markup nests the name in div.tt-name inside td.tdleft.
                let Some(title) = first(row, ".tt-name a[href]") else {
                    continue;
                };
                let href = title.value().attr("href").unwrap_or("");
                let parts: Vec<_> = href.split('/').collect();
                if parts.len() < 3
                    || parts[1].len() != 40
                    || !parts[1].bytes().all(|b| b.is_ascii_hexdigit())
                {
                    continue;
                }
                let hash = parts[1].to_lowercase();
                if !seen.insert(hash.clone()) {
                    continue;
                }
                let label = field(row, ".smallish").to_lowercase();
                let matched = [
                    ("anime", &["anime"][..]),
                    ("books", &["book"][..]),
                    ("movies", &["movie"][..]),
                    ("tv", &["tv", "television"][..]),
                    ("music", &["music", "audio", "lossless"][..]),
                    ("games", &["game", "xbox", "playstation"][..]),
                    ("software", &["applic", "software"][..]),
                ]
                .into_iter()
                .find(|(_, needles)| needles.iter().any(|n| label.contains(n)))
                .map(|(category, _)| category);
                if category != "all" && matched != Some(category) {
                    continue;
                }
                let normals: Vec<_> = row.select(&selector("td.tdnormal")).collect();
                let trackers = "&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=udp%3A%2F%2Fopen.demonii.com%3A1337%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce&tr=udp%3A%2F%2Fexodus.desync.com%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.birkenwald.de%3A6969%2Fannounce&tr=udp%3A%2F%2Fexplodie.org%3A6969%2Fannounce";
                Row {
                    title: text(title),
                    uri: format!("magnet:?xt=urn:btih:{hash}&dn={}{}", parts[2], trackers),
                    description: format!("{base}{href}"),
                    size: normals.get(1).map(|&n| text(n)).as_deref().and_then(size),
                    seeds: count(&field(row, "td.tdseed")),
                    leeches: count(&field(row, "td.tdleech")),
                    date: None,
                    detail: false,
                }
            }
            "yourbittorrent" => {
                let Some(link) = first(row, "a[href][title]") else {
                    continue;
                };
                let description = format!("{base}/{}", link.value().attr("href").unwrap());
                Row {
                    title: link
                        .value()
                        .attr("title")
                        .unwrap()
                        .replace("<b>", "")
                        .replace("</b>", "")
                        .replace("<span style=color:#39a8bb>", "")
                        .replace("</span>", ""),
                    uri: quote(&description),
                    description,
                    size: row_size(row),
                    seeds: count(&field(row, ".sd")),
                    leeches: count(&field(row, ".pr")),
                    date: None,
                    detail: false,
                }
            }
            "rockbox" => {
                let Some(link) = row.select(&selector("a[href^='details']")).next() else {
                    continue;
                };
                let Some(download) = first(row, "a[href^='download']") else {
                    continue;
                };
                let peers: Vec<_> = row
                    .select(&selector("a[title='peers details']"))
                    .map(text)
                    .collect();
                Row {
                    title: link
                        .value()
                        .attr("title")
                        .unwrap_or("")
                        .trim_start_matches("details:")
                        .trim_start()
                        .into(),
                    uri: quote(&format!(
                        "{base}/{}",
                        download.value().attr("href").unwrap()
                    )),
                    description: format!("{base}/{}", link.value().attr("href").unwrap()),
                    size: row_size(row),
                    seeds: peers.first().and_then(|s| s.parse().ok()),
                    leeches: peers.get(1).and_then(|s| s.parse().ok()),
                    date: row
                        .select(&selector("td.lista"))
                        .map(text)
                        .find_map(|s| date(&s)),
                    detail: false,
                }
            }
            "solidtorrents_community_053" => {
                let (Some(link), Some(magnet)) =
                    (first(row, ".info h5 a"), first(row, ".links a.dl-magnet"))
                else {
                    continue;
                };
                let stats: Vec<_> = first(row, ".stats")
                    .map(|n| n.select(&selector("div")).map(text).collect())
                    .unwrap_or_default();
                Row {
                    title: text(link),
                    uri: magnet.value().attr("href").unwrap_or("").into(),
                    description: format!("{base}{}", link.value().attr("href").unwrap_or("")),
                    size: stats.get(1).and_then(|s| size(s)),
                    seeds: stats.get(2).and_then(|s| count(s)),
                    leeches: stats.get(3).and_then(|s| count(s)),
                    date: None,
                    detail: false,
                }
            }
            _ => return Err(runtime("No public table parser")),
        };
        results.push(value);
    }
    Ok(results)
}

pub(super) fn detail_download(
    response: &str,
    base: &str,
    source: &str,
) -> Result<String, SearchError> {
    let document = Html::parse_document(response);
    for link in document.select(&selector("a[href]")) {
        let href = link.value().attr("href").unwrap();
        if href.ends_with(".torrent") {
            return join(base, href);
        }
        if source == "cpasbien" && href.starts_with("/get_torrent/")
            || source == "torrent9" && link.value().classes().any(|c| c == "download")
        {
            return join(base, href);
        }
        if source == "pirateiro" && link.value().classes().any(|c| c == "btn-down") {
            return join(base, &href.replace("kickasstorrents", "katcr"));
        }
    }
    Err(runtime(
        "Source detail has no magnet or torrent download URL",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from www.torrentdownload.info on 2026-10-18 (search "the bear s03").
    const PAGE: &str = include_str!("../../../tests/fixtures/torrentdownload-the-bear-s03.html");

    fn rows(id: &str, category: &str) -> Vec<Row> {
        let normalized = regex::Regex::new(r"\s+").unwrap().replace_all(PAGE, " ");
        parse(
            id,
            "https://www.torrentdownload.info",
            &normalized,
            category,
            &mut HashSet::new(),
        )
        .unwrap()
    }

    #[test]
    fn both_torrentdownload_parsers_read_the_live_markup_without_ads_or_labels() {
        for id in ["torrentdownload", "torrentdownload_community_066"] {
            let rows = rows(id, "tv");
            assert!(rows.len() >= 10, "{id}: {}", rows.len());
            let kontrast = rows
                .iter()
                .find(|row| row.title.contains("KONTRAST"))
                .unwrap_or_else(|| panic!("{id}: missing release"));
            assert_eq!(kontrast.title, "The Bear S03 1080p WEBRip x265 KONTRAST");
            assert_eq!(kontrast.seeds, Some(346));
            assert!(
                rows.iter()
                    .all(|row| !row.title.contains('»') && !row.title.contains("Download"))
            );
        }
    }
}
