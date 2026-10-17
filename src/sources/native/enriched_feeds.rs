//! Public feeds with local query matching and detail-page metadata enrichment.
use super::{NativeSource, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult, TorrentReference};
use chrono::TimeZone;
use scraper::{Html, Selector};
use serde_json::Value;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "academictorrents" | "audiobookbay" | "darklibria" | "eztvx" | "thepiratebay_community_059"
    )
}
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("Static selector")
}
fn cap(p: &str, s: &str) -> Option<String> {
    regex::Regex::new(p)
        .ok()?
        .captures(s)?
        .get(1)
        .map(|s| s.as_str().into())
}
fn text(n: scraper::ElementRef<'_>) -> String {
    n.text().collect::<String>().trim().into()
}
fn size(s: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(s)
}
fn number(v: &Value) -> Option<u64> {
    json::number(v)
}
fn quote(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, html::QUERY_SAFE).to_string()
}
fn local(s: &str, fmt: &str) -> Option<u64> {
    let d = chrono::NaiveDateTime::parse_from_str(s, fmt).ok()?;
    chrono::Local
        .from_local_datetime(&d)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}
fn iso(s: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .and_then(|d| d.timestamp().try_into().ok())
        .or_else(|| local(s, "%Y-%m-%dT%H:%M:%S"))
}
fn episodes(s: &str) -> (Option<u32>, Option<u32>) {
    let re=regex::Regex::new(r"(?i)\b(?:s(\d{1,2})e(\d{1,2})|s(\d{1,2})|e(\d{1,2})|(\d{1,2})x(\d{1,2})|season\s*(\d{1,2})\s*episode\s*(\d{1,2})|season\s*(\d{1,2}))\b").unwrap();
    let Some(c) = re.captures(s) else {
        return (None, None);
    };
    let n = |i| c.get(i).and_then(|v| v.as_str().parse().ok());
    if c.get(1).is_some() {
        (n(1), n(2))
    } else if c.get(3).is_some() {
        (n(3), None)
    } else if c.get(4).is_some() {
        (None, n(4))
    } else if c.get(5).is_some() {
        (n(5), n(6))
    } else if c.get(7).is_some() {
        (n(7), n(8))
    } else {
        (n(9), None)
    }
}

pub(super) fn cache_file(name: &str) -> std::path::PathBuf {
    let root = std::env::var_os("TORRENT_SEARCH_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(|s| std::path::PathBuf::from(s).join("torrent-search/cache"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("torrent-search-cache"));
    root.join(name)
}

impl NativeSource {
    pub(super) async fn resolve_piratebay_category(
        &self,
        url: &url::Url,
    ) -> Result<TorrentReference, SearchError> {
        let id = url
            .query_pairs()
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned())
            .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| runtime("PirateBay result identifier missing"))?;
        let base = self.descriptor.site_url.trim_end_matches('/');
        let api = if base == "https://thepiratebay.org" {
            "https://apibay.org"
        } else {
            base
        };
        let data: Value =
            serde_json::from_str(&self.fetch(&format!("{api}/t.php?id={id}"), None).await?)?;
        let hash = data["info_hash"]
            .as_str()
            .ok_or_else(|| runtime("PirateBay hash missing"))?;
        let title = data["name"].as_str().unwrap_or("");
        let mut uri = format!(
            "magnet:?xt=urn:btih:{hash}&dn={}",
            quote(title).replace('/', "%2F")
        );
        for tracker in [
            "udp://tracker.coppersurfer.tk:6969/announce",
            "udp://tracker.openbittorrent.com:6969/announce",
            "udp://9.rarbg.to:2710/announce",
            "udp://9.rarbg.me:2780/announce",
            "udp://9.rarbg.to:2730/announce",
            "udp://tracker.opentrackr.org:1337",
            "http://p4p.arenabg.com:1337/announce",
            "udp://tracker.torrent.eu.org:451/announce",
            "udp://tracker.tiny-vps.com:6969/announce",
            "udp://open.stealth.si:80/announce",
        ] {
            uri.push_str(&format!("&tr={}", json::form_value(tracker)));
        }
        crate::reference::parse_reference(&uri, &self.descriptor.id)
    }
    pub(super) async fn collect_enriched_feed(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let q = quote(&request.query);
        match id {
            "academictorrents" => {
                let cache = cache_file("academic-cache.xml");
                let today = chrono::Local::now().date_naive().to_string();
                let cached = tokio::fs::read_to_string(&cache)
                    .await
                    .ok()
                    .filter(|s| s.starts_with(&format!("{today}\n")));
                let response = if let Some(s) = cached {
                    s.split_once('\n').unwrap().1.to_owned()
                } else {
                    let data = self
                        .fetch_bounded(&format!("{base}/database.xml"), None, 64 * 1024 * 1024)
                        .await?;
                    // Validate before replacing a reusable cache with a failed response.
                    roxmltree::Document::parse(&data).map_err(|e| runtime(e.to_string()))?;
                    if let Some(parent) = cache.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    let temporary = cache.with_extension(format!("{}.tmp", std::process::id()));
                    tokio::fs::write(&temporary, format!("{today}\n{data}")).await?;
                    tokio::fs::rename(&temporary, &cache).await?;
                    data
                };
                let rows: Vec<(String, String, String, String)> = {
                    let doc = roxmltree::Document::parse(&response)
                        .map_err(|e| runtime(e.to_string()))?;
                    let terms: Vec<_> = q
                        .replace("%20", " ")
                        .split_whitespace()
                        .map(str::to_lowercase)
                        .collect();
                    doc.descendants()
                        .filter(|n| n.has_tag_name("item"))
                        .filter_map(|n| {
                            let f = |key| {
                                n.children()
                                    .find(|c| c.has_tag_name(key))
                                    .and_then(|c| c.text())
                                    .unwrap_or("")
                                    .to_owned()
                            };
                            let title = f("title");
                            let description = f("description");
                            terms
                                .iter()
                                .any(|t| {
                                    title.to_lowercase().contains(t)
                                        || description.to_lowercase().contains(t)
                                })
                                .then(|| (title, f("infohash"), f("size"), f("link")))
                        })
                        .collect()
                };
                for (title, hash, bytes, desc) in rows {
                    let details = self.fetch(&format!("{desc}/tech"), None).await?;
                    let peers = regex::Regex::new(
                        r"<tr><td>Mirrors</td><td>(\d+)\s*complete,\s*(\d+)\s*downloading",
                    )
                    .unwrap();
                    let peers = peers.captures(&details);
                    let date = cap(r"<tr><td>Added</td><td>([^<]+)</td></tr>", &details)
                        .as_deref()
                        .and_then(iso);
                    results.push(candidate(
                        &self.descriptor,
                        title,
                        format!("{base}/download/{hash}.torrent"),
                        Some(desc),
                        bytes.parse().ok(),
                        peers.as_ref().and_then(|c| c[1].parse().ok()),
                        peers.as_ref().and_then(|c| c[2].parse().ok()),
                        date,
                        now(),
                    )?);
                    if results.len() >= limit {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "audiobookbay" => {
                let mut actual = None;
                let mirrors = if base == "http://theaudiobookbay.se" {
                    vec![base, "http://audiobookbay.fi", "http://audiobookbay.is"]
                } else {
                    vec![base]
                };
                for mirror in mirrors {
                    if self.fetch(&format!("{mirror}/"), None).await.is_ok() {
                        actual = Some(mirror);
                        break;
                    }
                }
                let actual = actual.ok_or_else(|| runtime("No healthy AudioBookBay mirror"))?;
                let trackers = "&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Ftracker.open-internet.nl%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A69691337%2Fannounce&tr=udp%3A%2F%2Ftracker.vanitycore.co%3A6969%2Fannounce&tr=http%3A%2F%2Ftracker.baravik.org%3A6970%2Fannounce&tr=http%3A%2F%2Fretracker.telecom.by%3A80%2Fannounce&tr=http%3A%2F%2Ftracker.vanitycore.co%3A6969%2Fannounce";
                let mut last = 1;
                for page in 1.. {
                    let response = self
                        .fetch(&format!("{actual}//page/{page}/?s={q}&cat=all"), None)
                        .await?;
                    let (rows, pages): (Vec<_>, _) = {
                        let doc = Html::parse_document(&response);
                        let pages = doc
                            .select(&sel("a[title][href]"))
                            .filter(|a| a.value().attr("title").unwrap().contains("»»"))
                            .filter_map(|a| {
                                a.value()
                                    .attr("href")?
                                    .split('/')
                                    .nth(2)?
                                    .parse::<usize>()
                                    .ok()
                            })
                            .max()
                            .unwrap_or(0);
                        let rows = doc
                            .select(&sel(".post .postTitle a[href]"))
                            .map(|a| {
                                (
                                    text(a),
                                    format!("{actual}/{}", a.value().attr("href").unwrap()),
                                )
                            })
                            .collect();
                        (rows, pages)
                    };
                    if page == 1 {
                        last = pages.max(1);
                    }
                    for (title, desc) in rows {
                        let details = self.fetch(&desc, None).await?;
                        let doc = Html::parse_document(&details);
                        let texts: Vec<_> = doc
                            .root_element()
                            .text()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .collect();
                        let hash = texts
                            .windows(2)
                            .find(|p| p[0] == "Info Hash:")
                            .map(|p| p[1])
                            .ok_or_else(|| runtime("AudioBookBay info hash missing"))?;
                        let bytes = texts
                            .iter()
                            .position(|s| *s == "Combined File Size:")
                            .and_then(|i| {
                                let v = texts.get(i + 1)?;
                                let u = texts.get(i + 2).copied().unwrap_or("");
                                size(&format!("{v}{}", u.replace('s', "")))
                            })
                            .or(Some(100 * 1024 * 1024));
                        results.push(candidate(
                            &self.descriptor,
                            title.clone(),
                            format!("magnet:?xt=urn:btih:{hash}&dn={}{trackers}", quote(&title)),
                            Some(desc),
                            bytes,
                            Some(1),
                            Some(1),
                            None,
                            now(),
                        )?);
                        if results.len() >= limit {
                            return Ok(true);
                        }
                    }
                    if page >= last {
                        break;
                    }
                }
                Ok(false)
            }
            "darklibria" => {
                let mut pages = 1;
                for page in 1.. {
                    let response = self
                        .fetch(&format!("{base}/search?page={page}&find={q}"), None)
                        .await?;
                    let links: Vec<_> = {
                        let doc = Html::parse_document(&response);
                        if page == 1 {
                            let value = doc
                                .select(&sel("span.text.text-light.mt-0"))
                                .next()
                                .map(text)
                                .unwrap_or_default();
                            let parts: Vec<_> = value.split_whitespace().collect();
                            pages = parts
                                .get(4)
                                .and_then(|s| s.parse::<usize>().ok())
                                .zip(
                                    parts
                                        .get(2)
                                        .and_then(|s| s.split('-').nth(1)?.parse::<usize>().ok()),
                                )
                                .filter(|(_, n)| *n > 0)
                                .map(|(total, n)| total.div_ceil(n))
                                .unwrap_or(0);
                        }
                        doc.select(&sel("tbody[style='vertical-align: center'] a[href]"))
                            .map(|a| a.value().attr("href").unwrap().to_owned())
                            .collect()
                    };
                    for link in links {
                        let desc = url::Url::parse(&format!("{base}/"))
                            .and_then(|u| u.join(&link))
                            .map_err(|e| runtime(e.to_string()))?
                            .to_string();
                        let details = self.fetch(&desc, None).await?;
                        let rows = {
                            let doc = Html::parse_document(&details);
                            let name = doc
                                .select(&sel("#russian_name"))
                                .next()
                                .map(text)
                                .ok_or_else(|| runtime("DarkLibria series title missing"))?;
                            let mut rows = Vec::new();
                            for tr in doc.select(&sel("tr.torrent")) {
                                let td: Vec<_> = tr.select(&sel("td")).collect();
                                if td.len() < 7 {
                                    continue;
                                }
                                let date =
                                    cap(r"(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})", &text(td[3]))
                                        .ok_or_else(|| runtime("DarkLibria date missing"))?;
                                let date = chrono::NaiveDateTime::parse_from_str(
                                    &date,
                                    "%Y-%m-%d %H:%M:%S",
                                )
                                .map_err(|_| runtime("Invalid DarkLibria date"))?
                                .and_utc()
                                .with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M:%S");
                                let uri = td[4]
                                    .select(&sel(
                                        "a[title='Magnet-ссылка'], a[title='Скачать торрент']",
                                    ))
                                    .filter_map(|a| a.value().attr("href"))
                                    .find(|s| !s.is_empty())
                                    .ok_or_else(|| runtime("DarkLibria download missing"))?
                                    .to_owned();
                                let mut bytes = text(td[2]);
                                for (from, to) in [
                                    ("Тб", "TB"),
                                    ("Гб", "GB"),
                                    ("Мб", "MB"),
                                    ("Кб", "KB"),
                                    ("б", "B"),
                                ] {
                                    bytes = bytes.replace(from, to);
                                }
                                rows.push((
                                    format!("[{date}] {name} [{}] {}", text(td[0]), text(td[1])),
                                    uri,
                                    size(&bytes),
                                    text(td[5]).parse().ok(),
                                    text(td[6]).parse().ok(),
                                ));
                            }
                            rows
                        };
                        for (title, uri, bytes, seeds, leech) in rows {
                            results.push(candidate(
                                &self.descriptor,
                                title,
                                uri,
                                Some(desc.clone()),
                                bytes,
                                seeds,
                                leech,
                                None,
                                now(),
                            )?);
                            if results.len() >= limit {
                                return Ok(true);
                            }
                        }
                    }
                    if page >= pages {
                        break;
                    }
                }
                Ok(false)
            }
            "eztvx" => {
                // Preserve the original's encoded query handling and title cleanup.
                let keywords = q.replace("%20", " ").replace(['.', '-'], " ");
                let wanted = episodes(&keywords);
                let ep=regex::Regex::new(r"(?i)\b(?:s\d{1,2}e\d{1,2}|s\d{1,2}|e\d{1,2}|\d{1,2}x\d{1,2}|season\s*\d{1,2}\s*episode\s*\d{1,2}|season\s*\d{1,2})\b").unwrap();
                let junk=regex::Regex::new(r"(?i)\b(1080p|720p|480p|2160p|4k|x264|x265|hevc|avc|bluray|webrip|web-dl|hdtv|dvdrip|proper|repack|extended|theatrical|directors\.cut|remux)\b").unwrap();
                let cleaned = junk.replace_all(&keywords, "");
                let cleaned = ep
                    .replace_all(&cleaned, "")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let key = std::env::var("EZTVX_OMDB_API_KEY")
                    .unwrap_or_else(|_| "YOUR_OMDB_API_KEY".into());
                let omdb = if base == "https://eztvx.to" {
                    "http://www.omdbapi.com"
                } else {
                    base
                };
                let imdb = if cleaned.is_empty() {
                    None
                } else {
                    self.fetch(
                        &format!(
                            "{omdb}/?apikey={key}&t={}&type=series",
                            cleaned.replace(' ', "+")
                        ),
                        None,
                    )
                    .await
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .filter(|v| v["Response"] == "True")
                    .and_then(|v| v["imdbID"].as_str().map(|s| s.replace("tt", "")))
                };
                for page in 1.. {
                    let filter = imdb
                        .as_ref()
                        .map(|s| format!("imdb_id={s}"))
                        .unwrap_or_else(|| format!("Keywords={}", cleaned.replace(' ', "+")));
                    let response = self
                        .fetch(
                            &format!("{base}/api/get-torrents?limit=100&page={page}&{filter}"),
                            None,
                        )
                        .await?;
                    let data: Value = serde_json::from_str(&response)?;
                    let rows = data["torrents"]
                        .as_array()
                        .ok_or_else(|| runtime("EZTVX torrents array missing"))?;
                    for row in rows {
                        let title = row["title"].as_str().unwrap_or("Unknown");
                        let actual = episodes(title);
                        if wanted.0.is_some_and(|n| actual.0 != Some(n))
                            || wanted.1.is_some_and(|n| actual.1 != Some(n))
                            || imdb.is_none()
                                && !cleaned
                                    .split_whitespace()
                                    .all(|t| title.to_lowercase().contains(&t.to_lowercase()))
                        {
                            continue;
                        }
                        let uri = row["magnet_url"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .or_else(|| row["torrent_url"].as_str())
                            .unwrap_or("");
                        if uri.is_empty() {
                            continue;
                        }
                        let bytes = number(&row["size_bytes"]).and_then(|n| {
                            let n = n as f64;
                            size(&if n < 1048576. {
                                format!("{:.1} KB", n / 1024.)
                            } else if n < 1073741824. {
                                format!("{:.1} MB", n / 1048576.)
                            } else {
                                format!("{:.2} GB", n / 1073741824.)
                            })
                        });
                        results.push(candidate(
                            &self.descriptor,
                            title.into(),
                            uri.into(),
                            Some(row["episode_url"].as_str().unwrap_or(base).into()),
                            bytes,
                            Some(number(&row["seeds"]).unwrap_or(0)),
                            Some(number(&row["peers"]).unwrap_or(0)),
                            None,
                            now(),
                        )?);
                        if results.len() >= limit {
                            return Ok(true);
                        }
                    }
                    if rows.len() < 100
                        || page * 100 >= number(&data["torrents_count"]).unwrap_or(0)
                    {
                        break;
                    }
                }
                Ok(false)
            }
            "thepiratebay_community_059" => {
                let api = if base == "https://thepiratebay.org" {
                    "https://apibay.org"
                } else {
                    base
                };
                let categories: &[u32] = match request.category.as_str() {
                    "all" => &[0],
                    "anime" => &[
                        207, 208, 201, 202, 205, 206, 209, 299, 501, 502, 505, 599, 699,
                    ],
                    "books" => &[601, 602],
                    "games" => &[400, 504],
                    "movies" => &[207],
                    "music" => &[101, 104],
                    "pictures" => &[603, 604, 503],
                    "software" => &[300],
                    "tv" => &[208],
                    _ => return Err(runtime("Unknown PirateBay category")),
                };
                for category in categories {
                    let response = self
                        .fetch(&format!("{api}/q.php?q={q}&cat={category}"), None)
                        .await?;
                    let data: Value = serde_json::from_str(&response)?;
                    for row in data
                        .as_array()
                        .ok_or_else(|| runtime("Unexpected PirateBay JSON"))?
                    {
                        let desc = format!(
                            "{base}/description.php?id={}",
                            row["id"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| row["id"].to_string())
                        );
                        results.push(candidate(
                            &self.descriptor,
                            row["name"].as_str().unwrap_or("").into(),
                            desc.clone(),
                            Some(desc),
                            number(&row["size"]),
                            number(&row["seeders"]),
                            number(&row["leechers"]),
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
            _ => Err(runtime("Unknown enriched feed")),
        }
    }
}
