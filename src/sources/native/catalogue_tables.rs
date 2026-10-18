//! Independent public catalogues whose rows carry source-specific metadata.
use super::{NativeSource, candidate, html, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use chrono::TimeZone;
use scraper::{ElementRef, Html, Selector};
use std::{collections::HashSet, time::Duration};

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "bitsearch"
            | "therarbg"
            | "torrentdownloads"
            | "traht"
            | "xxxclubto"
            | "bt4gprx"
            | "linuxtracker"
            | "sktorrent"
            | "tokyotoshokan"
    )
}
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("Static selector")
}
fn raw(n: ElementRef<'_>) -> String {
    n.text().collect::<String>().trim().into()
}
fn first<'a>(n: ElementRef<'a>, s: &str) -> Option<ElementRef<'a>> {
    n.select(&sel(s)).next()
}
fn field(n: ElementRef<'_>, s: &str) -> String {
    first(n, s).map(raw).unwrap_or_default()
}
fn href(n: ElementRef<'_>) -> String {
    n.value().attr("href").unwrap_or("").into()
}
fn num(s: &str) -> Option<u64> {
    s.trim().replace(',', "").parse().ok()
}
fn size(s: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(s)
}
fn cap(p: &str, s: &str) -> Option<String> {
    regex::Regex::new(p)
        .ok()?
        .captures(s)?
        .get(1)
        .map(|m| m.as_str().into())
}
fn local_date(s: &str, fmt: &str) -> Option<u64> {
    let d = chrono::NaiveDate::parse_from_str(s, fmt)
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&d)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}
struct Row {
    title: String,
    uri: String,
    desc: Option<String>,
    bytes: Option<u64>,
    seeds: Option<u64>,
    leech: Option<u64>,
    date: Option<u64>,
    detail: bool,
}
impl Row {
    fn new(title: String, uri: String) -> Self {
        Self {
            title,
            uri,
            desc: None,
            bytes: None,
            seeds: None,
            leech: None,
            date: None,
            detail: false,
        }
    }
}

impl NativeSource {
    pub(super) async fn collect_catalogue_tables(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        if id == "bt4gprx" {
            // The HTML search sits behind a browser challenge; the RSS feed
            // does not, and already carries magnets: one request, no detail pages.
            let q = percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE)
                .to_string()
                .replace("%20", "+");
            let feed = self
                .fetch(
                    &format!("{base}/search?q={q}&page=rss&orderby=seeders"),
                    None,
                )
                .await?;
            for row in parse_bt4g_rss(&feed)? {
                results.push(candidate(
                    &self.descriptor,
                    row.title,
                    row.uri,
                    row.desc,
                    row.bytes,
                    None,
                    None,
                    row.date,
                    now(),
                )?);
                if results.len() >= limit {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let q = percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        let plus = q.replace("%20", "+");
        let start = if id == "sktorrent" { 0 } else { 1 };
        let mut page = start;
        let mut last = usize::MAX;
        let mut seen = HashSet::new();
        let mut first_row_error = None;
        let mut tokyo_url = None;
        loop {
            let category = request.category.as_str();
            let route = match id {
                "bitsearch" => format!("{base}/search?q={plus}&page={page}"),
                "therarbg" => {
                    let c = match category {
                        "all" => "All",
                        "tv" => "TV",
                        "software" => "Apps",
                        "movies" => "Movies",
                        "anime" => "Anime",
                        "games" => "Games",
                        "music" => "Music",
                        _ => return Err(runtime("Unknown RarBg category")),
                    };
                    format!(
                        "{base}/get-posts/order:-se:{}keywords:{q}/?page={page}",
                        if c == "All" {
                            String::new()
                        } else {
                            format!("category:{c}:")
                        }
                    )
                }
                "torrentdownloads" => {
                    let c = match category {
                        "all" => 0,
                        "anime" => 1,
                        "books" => 2,
                        "games" => 3,
                        "movies" => 4,
                        "music" => 5,
                        "software" => 7,
                        "tv" => 8,
                        _ => return Err(runtime("Unknown TorrentDownloads category")),
                    };
                    format!("{base}/search/?new=1&s_cat={c}&search={plus}&page={page}")
                }
                "traht" => format!("{base}/browse.php?search={plus}&page={page}"),
                "xxxclubto" => format!(
                    "{base}/torrents/search/{}/{q}?page={page}&sort=seeders&order=asc",
                    if category == "pictures" { "5" } else { "All" }
                ),
                "linuxtracker" => format!(
                    "{base}/index.php?page=torrents&active=1&order=5&by=2&search={q}&pages={page}"
                ),
                "sktorrent" => format!(
                    "{base}/torrent/torrents_v2.php?search={q}&category=0&zaner=&jazyk=&active=0&page={page}"
                ),
                "tokyotoshokan" => tokyo_url.clone().unwrap_or_else(|| {
                    format!(
                        "{base}/search.php?terms={q}&type={}&size_min=&size_max=&username=",
                        match category {
                            "anime" => 1,
                            "games" => 14,
                            _ => 0,
                        }
                    )
                }),
                _ => return Err(runtime("Unknown public catalogue")),
            };
            let response = self.fetch(&route, None).await?;
            if id == "sktorrent" && response.contains("Nenasli ste co ste hladali") {
                break;
            }
            if page == start {
                if id == "bitsearch" {
                    last = cap(r"Found\s+<span[^>]*>(\d+)</span>", &response)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                        .div_ceil(20)
                        .min(10);
                }
                if id == "traht" {
                    last = cap(r#"<div class="paginator_pages">(.*?)</div>"#, &response)
                        .and_then(|s| s.split(' ').nth(3)?.parse().ok())
                        .unwrap_or(0);
                }
            }
            if matches!(id, "bitsearch" | "traht") && last == 0 {
                break;
            }
            let rows = parse(id, base, &response)?;
            let count = rows.len();
            {
                for mut row in rows {
                    let description = row.desc.clone().unwrap_or_else(|| row.uri.clone());
                    let result = async {
                        if row.detail {
                            let text = self.fetch(&row.uri, None).await?;
                            row.uri = html::resolve_magnet(&text, id)?.uri().into();
                        }
                        candidate(
                            &self.descriptor,
                            row.title,
                            row.uri,
                            row.desc,
                            row.bytes,
                            row.seeds,
                            row.leech,
                            row.date,
                            now(),
                        )
                    }
                    .await;
                    match result {
                        Ok(result) => results.push(result),
                        Err(error) if id == "torrentdownloads" => {
                            // One bad detail must not discard later results.
                            // Keep the first error for an honest partial report.
                            first_row_error.get_or_insert_with(|| {
                                runtime(format!(
                                    "Skipped TorrentDownloads result {description}: {error}"
                                ))
                            });
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                    if results.len() >= limit {
                        return first_row_error.map_or(Ok(true), Err);
                    }
                }
            }
            if id == "tokyotoshokan" {
                // The original re-fetches the current page to discover continuation links.
                let navigation = self.fetch(&route, None).await?;
                let links: Vec<String> = {
                    let doc = Html::parse_document(&navigation);
                    doc.select(&sel("table.listing a[href]"))
                        .filter_map(|a| {
                            let h = href(a);
                            if h.starts_with("?lastid=")
                                && h.contains("&page=")
                                && h.contains("&terms=")
                            {
                                Some(format!("{base}/search.php{h}"))
                            } else {
                                None
                            }
                        })
                        .collect()
                };
                tokyo_url = links.into_iter().find(|h| seen.insert(h.clone()));
                if tokyo_url.is_none() {
                    break;
                }
            } else if count == 0
                || page >= last
                || id == "linuxtracker" && count < 15
                || id == "therarbg" && !super::static_regex(r"<a[^>]*>»</a>").is_match(&response)
                || id == "torrentdownloads"
                    && !super::static_regex(r"<a[^>]*>>></a>").is_match(&response)
                || id == "xxxclubto" && !response.contains("title=\"Next Page\"")
            {
                break;
            }
            let delay = match id {
                "bitsearch" if page > 1 => 750,
                "therarbg" | "torrentdownloads" | "xxxclubto" => 500,
                _ => 0,
            };
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            page += 1;
        }
        first_row_error.map_or(Ok(false), Err)
    }
}

/// BT4G RSS items: title, magnet link, and a description
/// "name<br>size<br>kind<br>infohash"; no seeder counts.
fn parse_bt4g_rss(feed: &str) -> Result<Vec<Row>, SearchError> {
    let document =
        roxmltree::Document::parse(feed).map_err(|e| runtime(format!("BT4G RSS: {e}")))?;
    let mut rows = Vec::new();
    for item in document.descendants().filter(|n| n.has_tag_name("item")) {
        let text = |tag: &str| {
            item.children()
                .find(|n| n.has_tag_name(tag))
                .and_then(|n| n.text())
                .map(str::trim)
                .unwrap_or("")
                .to_owned()
        };
        let (title, link) = (text("title"), text("link"));
        if title.is_empty() || !link.starts_with("magnet:") {
            continue;
        }
        let mut row = Row::new(title, link);
        let description = text("description");
        row.bytes = description.split("<br>").nth(1).and_then(size);
        row.date = chrono::DateTime::parse_from_str(&text("pubDate"), "%a,%d %b %Y %H:%M:%S %z")
            .ok()
            .and_then(|d| u64::try_from(d.timestamp()).ok());
        row.desc = item
            .children()
            .find(|n| n.has_tag_name("guid"))
            .and_then(|n| n.text())
            .map(str::to_owned);
        rows.push(row);
    }
    Ok(rows)
}

fn parse(id: &str, base: &str, response: &str) -> Result<Vec<Row>, SearchError> {
    let doc = Html::parse_document(response);
    let mut out = Vec::new();
    match id {
        "therarbg" | "traht" => {
            for row in doc.select(&sel(if id == "traht" {
                "#releases-table tbody#highlighted tr"
            } else {
                "table tbody tr"
            })) {
                let td: Vec<_> = row.select(&sel("td")).collect();
                if td.len() < if id == "traht" { 6 } else { 8 } {
                    continue;
                }
                let Some(a) = first(td[1], "a[href]") else {
                    continue;
                };
                let desc = format!("{base}/{}", href(a));
                let mut r = Row::new(
                    if id == "traht" {
                        field(td[1], "b")
                    } else {
                        format!("{} ({})", raw(a), field(td[2], "a"))
                    },
                    desc.clone(),
                );
                r.desc = Some(desc);
                r.detail = id == "therarbg";
                if id == "traht" {
                    let Some(dl) = first(td[2], "a[href]") else {
                        continue;
                    };
                    r.uri = format!("{base}/{}&ok=", href(dl));
                    r.bytes = size(&raw(td[4]).replace(',', "."));
                    let peers = raw(td[5]);
                    let mut values = peers.split('|');
                    r.seeds = values.next().and_then(num);
                    r.leech = values.next().and_then(num);
                } else {
                    r.bytes = size(&raw(td[5]).replace(',', ".").replace('\u{a0}', " "));
                    r.seeds = num(&raw(td[6]));
                    r.leech = num(&raw(td[7]));
                }
                out.push(r);
            }
        }
        "torrentdownloads" => {
            for row in doc.select(&sel(".inner_container div.grey_bar3")) {
                if first(row, "p b").is_some() {
                    continue;
                }
                let Some(a) = first(row, "p a[href^='/torrent/']") else {
                    continue;
                };
                let desc = format!("{base}/{}", href(a));
                let mut r = Row::new(raw(a), desc.clone());
                r.desc = Some(desc);
                r.detail = true;
                let cells: Vec<_> = row.select(&sel("span")).map(raw).collect();
                r.leech = cells.get(1).and_then(|s| num(s));
                r.seeds = cells.get(2).and_then(|s| num(s));
                r.bytes = cells.get(3).and_then(|s| size(&s.replace('\u{a0}', " ")));
                out.push(r);
            }
        }
        "xxxclubto" => {
            for row in doc.select(&sel(".browsetableinside ul li")).skip(1) {
                let cells: Vec<_> = row
                    .select(&sel("span"))
                    .skip_while(|s| s.value().attr("class") != Some("catlabe"))
                    .skip(1)
                    .collect();
                if cells.len() < 5 {
                    continue;
                }
                let Some(a) = first(cells[0], "a[href]") else {
                    continue;
                };
                let desc = format!("{base}{}", href(a));
                let mut r = Row::new(raw(a), desc.clone());
                r.desc = Some(desc);
                r.detail = true;
                r.bytes = size(&raw(cells[2]).replace(',', ""));
                r.seeds = num(&raw(cells[3]));
                r.leech = num(&raw(cells[4]));
                out.push(r);
            }
        }
        "bitsearch" => {
            for row in doc.select(&sel("main.mx-auto .space-y-4 .bg-white")) {
                let Some(a) = first(row, ".flex-1 a[href]") else {
                    continue;
                };
                let Some(m) = first(row, ".space-y-2 a[href^='magnet:']") else {
                    continue;
                };
                let mut r = Row::new(raw(a), href(m));
                r.desc = Some(format!("{base}{}", href(a)));
                let groups: Vec<_> = row.select(&sel(".items-center")).collect();
                if let Some(stats) = groups.get(1) {
                    let values: Vec<_> = stats.select(&sel("span:not([class])")).map(raw).collect();
                    r.bytes = values.get(1).and_then(|s| size(&s.replace(' ', "")));
                    r.date = values.get(2).and_then(|s| local_date(s, "%m/%d/%Y"));
                }
                if let Some(swarm) = groups.get(2) {
                    let v: Vec<_> = swarm.select(&sel("span.font-medium")).map(raw).collect();
                    r.seeds = v.first().and_then(|s| num(s));
                    r.leech = v.get(1).and_then(|s| num(s));
                }
                out.push(r);
            }
        }
        "sktorrent" => {
            let re = super::static_regex(
                r#"(?s)id=([a-f0-9]+)".*?<br>(.*?)</A>\s*<br>Velkost ([^|]+)\| Pridany (\d{2}/\d{2}/\d{4})<br>\s*Odosielaju : (\d+)<br>\s*Stahuju : (\d+)"#,
            );
            let trackers = "https%3A%2F%2Fannounce1.sktorrent.eu%2Ftorrent%2Fannounce.php%3Fpid%3D1968a215113a4d87936336557ba8e800&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Fipv4announce.sktorrent.eu%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Ftracker.dler.org%3A6969%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce&tr=udp%3A%2F%2Fopen.demonii.com%3A1337%2Fannounce";
            for m in re.captures_iter(response) {
                let mut r = Row::new(
                    m[2].into(),
                    format!("magnet:?xt=urn:btih:{}&tr={trackers}", &m[1]),
                );
                r.desc = Some(format!("{base}/torrent/details.php?id={}", &m[1]));
                r.bytes = size(m[3].trim());
                r.seeds = num(&m[5]);
                r.leech = num(&m[6]);
                r.date = local_date(&m[4], "%d/%m/%Y");
                out.push(r);
            }
        }
        "linuxtracker" => {
            // Follow the original stream parser's six <strong> boundaries.
            let tokens = super::static_regex(r"(?s)<[^>]*>|[^<]+");
            let mut current: Option<Row> = None;
            let mut strong = 0;
            let mut waiting = true;
            for token in tokens.find_iter(response) {
                let t = token.as_str();
                if t.starts_with("<a ") {
                    let frag = Html::parse_fragment(t);
                    let Some(a) = frag.select(&sel("a")).next() else {
                        continue;
                    };
                    let h = href(a);
                    if h.contains("torrent-details") && a.value().attr("title").is_some() {
                        let desc = format!("{base}/{h}");
                        let mut r = Row::new(String::new(), String::new());
                        r.desc = Some(desc);
                        current = Some(r);
                        waiting = true;
                    } else if h.starts_with("magnet:?") {
                        if let Some(mut r) = current.take() {
                            r.uri = h;
                            out.push(r)
                        }
                    } else if h.contains("peers") {
                        waiting = true;
                    }
                } else if t == "</strong>" {
                    strong += 1;
                    waiting = true;
                } else if !t.starts_with('<') && waiting {
                    let value = html_escape::decode_html_entities(t);
                    if let Some(r) = current.as_mut() {
                        match strong {
                            0 => r.title = value.trim().into(),
                            3 => r.bytes = size(&value.replace(',', "")),
                            4 => r.seeds = num(&value),
                            5 => r.leech = num(&value),
                            _ => {}
                        }
                    }
                    if strong == 6 {
                        strong = 0;
                    }
                    waiting = false;
                }
            }
        }
        "tokyotoshokan" => {
            let mut current: Option<Row> = None;
            for row in doc.select(&sel("table.listing tr")) {
                if let Some(a) = first(row, "a[type='application/x-bittorrent']") {
                    current = Some(Row::new(raw(a), String::new()));
                }
                if let Some(r) = current.as_mut() {
                    if let Some(a) = first(row, "a[href^='magnet:']") {
                        r.uri = href(a);
                    }
                    if let Some(a) = first(row, "a[href^='details']") {
                        r.desc = Some(format!("{base}/{}", href(a)));
                    }
                    if let Some(s) = cap(r"Size:\s+([^ ]*)\s", &field(row, "td.desc-bot")) {
                        r.bytes = size(&s);
                    }
                    let stats: Vec<_> = row.select(&sel("td.stats span")).map(raw).collect();
                    if stats.len() >= 2 {
                        r.seeds = num(&stats[0]);
                        r.leech = num(&stats[1]);
                    }
                    if !r.uri.is_empty() && r.desc.is_some() && stats.len() >= 2 {
                        out.push(current.take().unwrap());
                    }
                }
            }
        }
        _ => return Err(runtime("Unknown catalogue parser")),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt4g_rss_yields_magnets_sizes_and_dates() {
        let rows = parse_bt4g_rss(include_str!(
            "../../../tests/fixtures/bt4gprx-ubuntu-rss.xml"
        ))
        .unwrap();
        assert_eq!(rows.len(), 15);
        let first = &rows[0];
        assert_eq!(first.title, "ubuntu-unity-26.04-desktop-amd64.iso");
        assert!(
            first
                .uri
                .starts_with("magnet:?xt=urn:btih:a5283cf4e0f3aa287ed45e4945e67d28eac07de8")
        );
        assert!(first.bytes.is_some_and(|b| b > 4_000_000_000));
        assert!(first.date.is_some());
        assert!(
            first
                .desc
                .as_deref()
                .unwrap()
                .starts_with("https://bt4gprx.com/magnet/")
        );
    }
    use crate::{SearchLimits, SourceDescriptor, SourceStatus, sources::SearchSource};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_util::sync::CancellationToken;

    async fn search_with_one_bad_detail(limit: usize) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    let n = stream.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap();
                let body = if path.starts_with("/search/") && path.contains("page=1") {
                    format!("<div class='inner_container'>{}</div>", (1..=3).map(|i|
                        format!("<div class='grey_bar3'><p><a href='/torrent/{i}'>Result {i}</a></p></div>")
                    ).collect::<String>())
                } else if path.starts_with("/search/") {
                    "<p>End</p>".into()
                } else if path.ends_with("/2") {
                    "<a href='magnet:?xt=urn:btih:invalid'>Bad</a>".into()
                } else {
                    let hash = if path.ends_with("/1") { "0" } else { "1" }.repeat(40);
                    format!("<a href='magnet:?xt=urn:btih:{hash}&amp;dn=A%26B%C2%8D'>Good</a>")
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let source = NativeSource::new(
            SourceDescriptor {
                id: "torrentdownloads".into(),
                name: "TorrentDownloads".into(),
                site_url: base,
                categories: vec!["all".into()],
                resolves_http: true,
                scope: Default::default(),
                language: None,
                alternate_of: None,
            },
            std::path::Path::new("."),
        )
        .unwrap();
        let outcome = source
            .search(
                &SearchRequest {
                    query: "nirvana in fire".into(),
                    category: "all".into(),
                    sources: vec![],
                },
                &SearchLimits {
                    results_per_source: limit,
                    ..SearchLimits::default()
                },
                CancellationToken::new(),
            )
            .await;
        server.abort();
        assert_eq!(outcome.results.len(), 2);
        assert_eq!(outcome.results[1].title, "Result 3");
        assert!(
            outcome
                .results
                .iter()
                .all(|r| r.reference.uri().ends_with("&dn=A%26B%C2%8D"))
        );
        // Retain the per-source error so skipped results cannot become a false
        // success, while returning valid results from both sides of the bad row.
        assert_eq!(outcome.report.status, SourceStatus::Failed);
        assert!(
            outcome
                .report
                .error
                .unwrap()
                .contains("Skipped TorrentDownloads result")
        );
    }

    #[tokio::test]
    async fn torrentdownloads_continues_after_a_bad_detail_until_end_of_search() {
        search_with_one_bad_detail(3).await;
    }

    #[tokio::test]
    async fn torrentdownloads_keeps_row_errors_when_the_result_limit_is_reached() {
        search_with_one_bad_detail(2).await;
    }
}
