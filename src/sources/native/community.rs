//! Reviewed community ports; each variant keeps its original source identity.
use super::{NativeSource, append, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult, SourceDescriptor};
use scraper::{Html, Selector};
use serde_json::Value;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "nyaasi"
            | "sukebeisi"
            | "cloudtorrents"
            | "animetosho"
            | "animetosho_community_006"
            | "mikan"
            | "subsplease"
            | "acgrip"
            | "thepiratebay"
            | "torrentclaw"
    )
}

impl NativeSource {
    pub(super) async fn collect_community(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let query =
            percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        match id {
            "thepiratebay" => {
                let api = if base == "https://thepiratebay.org" {
                    "https://apibay.org"
                } else {
                    base
                };
                let response = self
                    .fetch(&format!("{api}/q.php?q={query}&cat=0"), None)
                    .await?;
                let value: Value = serde_json::from_str(&response)?;
                let rows = value
                    .as_array()
                    .ok_or_else(|| runtime("Unexpected Pirate Bay JSON"))?;
                if rows
                    .first()
                    .is_some_and(|row| row["name"] == "No results returned")
                {
                    return Ok(false);
                }
                let trackers = "&tr=udp%3A%2F%2Ftracker.coppersurfer.tk%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.openbittorrent.com%3A6969%2Fannounce&tr=udp%3A%2F%2F9.rarbg.to%3A2710%2Fannounce&tr=udp%3A%2F%2F9.rarbg.me%3A2780%2Fannounce&tr=udp%3A%2F%2F9.rarbg.to%3A2730%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=http%3A%2F%2Fp4p.arenabg.com%3A1337%2Fannounce&tr=udp%3A%2F%2Ftracker.torrent.eu.org%3A451%2Fannounce&tr=udp%3A%2F%2Ftracker.tiny-vps.com%3A6969%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce";
                let mut parsed = vec![];
                for row in rows {
                    let title = row["name"]
                        .as_str()
                        .ok_or_else(|| runtime("Pirate Bay result has no title"))?;
                    let hash = row["info_hash"]
                        .as_str()
                        .ok_or_else(|| runtime("Pirate Bay result has no hash"))?;
                    let download = format!(
                        "magnet:?xt=urn:btih:{hash}&dn={}{trackers}",
                        percent_encoding::utf8_percent_encode(title, html::QUERY_SAFE)
                    );
                    let id = row["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| row["id"].to_string());
                    parsed.push(candidate(
                        &self.descriptor,
                        title.into(),
                        download,
                        Some(format!("https://thepiratebay.org/description.php?id={id}")),
                        json::number(&row["size"]),
                        json::number(&row["seeders"]),
                        json::number(&row["leechers"]),
                        None,
                        now(),
                    )?);
                }
                Ok(append(results, parsed, limit))
            }
            "torrentclaw" => {
                let mut url = format!(
                    "{base}/api/v1/search?q={}&limit=50",
                    json::form_value(&request.query)
                );
                if request.category != "all" {
                    url.push_str(match request.category.as_str() {
                        "movies" => "&type=movie",
                        "tv" => "&type=show",
                        _ => return Err(runtime("Unsupported TorrentClaw category")),
                    });
                }
                let response = self.fetch(&url, None).await?;
                let value: Value = serde_json::from_str(&response)?;
                let rows = value["results"]
                    .as_array()
                    .ok_or_else(|| runtime("Unexpected TorrentClaw JSON"))?;
                let trackers = [
                    "udp://tracker.opentrackr.org:1337/announce",
                    "udp://open.tracker.cl:1337/announce",
                    "udp://tracker.openbittorrent.com:6969/announce",
                    "udp://tracker.torrent.eu.org:451/announce",
                    "udp://open.stealth.si:80/announce",
                    "udp://exodus.desync.com:6969/announce",
                    "udp://open.demonii.com:1337/announce",
                    "udp://tracker.qu.ax:6969/announce",
                    "udp://tracker.dler.org:6969/announce",
                    "udp://tracker.filemail.com:6969/announce",
                ];
                let mut parsed = vec![];
                for content in rows {
                    let Some(torrents) = content["torrents"].as_array() else {
                        continue;
                    };
                    for torrent in torrents {
                        let Some(hash) = torrent["infoHash"].as_str().filter(|s| !s.is_empty())
                        else {
                            continue;
                        };
                        let title = torrent["rawTitle"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .or_else(|| content["title"].as_str())
                            .unwrap_or("");
                        let mut decorated = title.to_owned();
                        if let Some(score) = torrent["qualityScore"].as_i64() {
                            decorated.push_str(&format!(" [TC-{score}]"));
                        }
                        if torrent["scanStatus"] == "success" {
                            decorated.push_str(" [TrueSpec]");
                        }
                        let uri = torrent["magnetUrl"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                let mut uri = format!(
                                    "magnet:?xt=urn:btih:{hash}&dn={}",
                                    percent_encoding::utf8_percent_encode(
                                        &title.replace(' ', "."),
                                        html::QUERY_SAFE
                                    )
                                );
                                for tracker in trackers {
                                    uri.push_str(&format!("&tr={}", json::form_value(tracker)));
                                }
                                uri
                            });
                        let size = json::number(&torrent["sizeBytes"])
                            .filter(|&s| s > 0)
                            .and_then(|size| {
                                let mut value = size as f64;
                                let mut unit = 0;
                                let units = ["B", "KB", "MB", "GB", "TB"];
                                while value >= 1024. && unit < 4 {
                                    value /= 1024.;
                                    unit += 1;
                                }
                                super::super::limetorrents::parse_size(&format!(
                                    "{value:.2} {}",
                                    units[unit]
                                ))
                            });
                        let date = torrent["uploadedAt"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .and_then(|d| d.timestamp().try_into().ok());
                        let description = content["contentUrl"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(|s| format!("{base}{s}"))
                            .unwrap_or_else(|| base.into());
                        parsed.push(candidate(
                            &self.descriptor,
                            decorated,
                            uri,
                            Some(description),
                            size,
                            torrent["seeders"].as_u64(),
                            torrent["leechers"].as_u64(),
                            date,
                            now(),
                        )?);
                    }
                }
                Ok(append(results, parsed, limit))
            }
            "nyaasi" | "sukebeisi" => {
                let category = nyaa_category(id, &request.category)?;
                let mut page = 1;
                loop {
                    let url =
                        format!("{base}/?f=0&s=seeders&o=desc&c={category}&q={query}&p={page}");
                    let response = self.fetch(&url, None).await?;
                    let parsed = parse_nyaa(&response, &self.descriptor, now())?;
                    let count = parsed.len();
                    if append(results, parsed, limit) {
                        return Ok(true);
                    }
                    if count < 75 {
                        return Ok(false);
                    }
                    page += 1;
                }
            }
            "animetosho" | "animetosho_community_006" => {
                let modern = id.ends_with("006");
                let feed = match base {
                    "https://animetosho.org" => "https://feed.animetosho.org",
                    "https://animetosho.xyz" => "https://feed.animetosho.xyz",
                    _ => base,
                };
                let query = if modern {
                    json::form_value(&request.query)
                } else {
                    query
                };
                let response = self.fetch(&format!("{feed}/json?q={query}"), None).await?;
                let value: Value = serde_json::from_str(&response)?;
                let rows = value
                    .as_array()
                    .ok_or_else(|| runtime("Unexpected Anime Tosho JSON"))?;
                let mut parsed = vec![];
                for row in rows {
                    let uri = row["magnet_uri"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            if modern {
                                row["torrent_url"].as_str()
                            } else {
                                None
                            }
                        });
                    let Some(uri) = uri else {
                        if modern {
                            continue;
                        } else {
                            return Err(runtime("Anime Tosho result has no magnet"));
                        }
                    };
                    let title = row["title"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            if modern {
                                row["torrent_name"].as_str()
                            } else {
                                None
                            }
                        })
                        .unwrap_or("Unknown");
                    parsed.push(candidate(
                        &self.descriptor,
                        title.into(),
                        uri.into(),
                        row["link"].as_str().map(str::to_owned),
                        json::number(&row["total_size"]),
                        json::number(&row["seeders"]),
                        json::number(&row["leechers"]),
                        if modern {
                            json::number(&row["timestamp"])
                        } else {
                            None
                        },
                        now(),
                    )?);
                }
                Ok(append(results, parsed, limit))
            }
            "mikan" => {
                let response = self
                    .fetch(&format!("{base}/RSS/Search?searchstr={query}"), None)
                    .await?;
                let document = roxmltree::Document::parse(&response)
                    .map_err(|_| runtime("Invalid Mikan RSS"))?;
                let mut parsed = vec![];
                for item in document.descendants().filter(|n| n.has_tag_name("item")) {
                    let text = |tag: &str| {
                        item.children()
                            .find(|n| n.has_tag_name(tag))
                            .and_then(|n| n.text())
                    };
                    let enclosure = item
                        .children()
                        .find(|n| n.has_tag_name("enclosure"))
                        .ok_or_else(|| runtime("Mikan item has no enclosure"))?;
                    parsed.push(candidate(
                        &self.descriptor,
                        text("title")
                            .ok_or_else(|| runtime("Mikan item has no title"))?
                            .into(),
                        enclosure
                            .attribute("url")
                            .ok_or_else(|| runtime("Mikan item has no download URL"))?
                            .into(),
                        text("link").map(str::to_owned),
                        enclosure.attribute("length").and_then(|s| s.parse().ok()),
                        None,
                        None,
                        None,
                        now(),
                    )?);
                }
                Ok(append(results, parsed, limit))
            }
            "subsplease" => {
                for page in 0..6 {
                    let response = self
                        .fetch(
                            &format!("{base}/api/?f=search&tz=$&s={query}&p={page}"),
                            None,
                        )
                        .await?;
                    let value: Value = serde_json::from_str(&response)?;
                    if value.as_object().is_some_and(|rows| rows.is_empty())
                        || value.as_array().is_some_and(|rows| rows.is_empty())
                    {
                        break;
                    }
                    let rows = value
                        .as_object()
                        .ok_or_else(|| runtime("Unexpected SubsPlease JSON"))?;
                    let mut parsed = vec![];
                    for (name, row) in rows {
                        let date = row["release_date"]
                            .as_str()
                            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
                            .and_then(|d| d.timestamp().try_into().ok());
                        for download in row["downloads"]
                            .as_array()
                            .ok_or_else(|| runtime("SubsPlease result has no downloads"))?
                        {
                            let uri = download["magnet"]
                                .as_str()
                                .ok_or_else(|| runtime("SubsPlease result has no magnet"))?;
                            let size = url::Url::parse(uri).ok().and_then(|u| {
                                u.query_pairs()
                                    .find(|(k, _)| k == "xl")
                                    .and_then(|(_, v)| v.parse().ok())
                            });
                            let resolution = download["res"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| download["res"].to_string());
                            parsed.push(candidate(
                                &self.descriptor,
                                format!("[SubsPlease] {name} ({resolution}p)"),
                                uri.into(),
                                None,
                                size,
                                None,
                                None,
                                date,
                                now(),
                            )?);
                        }
                    }
                    if append(results, parsed, limit) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "cloudtorrents" => self.collect_cloud(request, limit, results).await,
            "acgrip" => {
                let response = self.fetch(&format!("{base}/?term={query}"), None).await?;
                Ok(append(
                    results,
                    parse_acgrip(&response, &self.descriptor, now())?,
                    limit,
                ))
            }
            _ => Err(runtime("No community adapter for source")),
        }
    }

    async fn collect_cloud(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let base = self.descriptor.site_url.trim_end_matches('/');
        let api = if base == "https://cloudtorrents.com" {
            "https://api.cloudtorrents.com"
        } else {
            base
        };
        let query =
            percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        let category = match request.category.as_str() {
            "all" => None,
            "anime" => Some("1"),
            "software" => Some("2"),
            "books" => Some("3"),
            "games" => Some("4"),
            "movies" => Some("5"),
            "music" => Some("6"),
            "tv" => Some("8"),
            _ => return Err(runtime("Unsupported CloudTorrents category")),
        };
        let mut offset = 0;
        let mut collected = vec![];
        loop {
            let mut url = format!("{api}/search/?offset={offset}&limit=50&query={query}");
            if let Some(category) = category {
                url.push_str(&format!("&torrent_type={category}"));
            }
            let response = self.fetch(&url, None).await?;
            let value: Value = serde_json::from_str(&response)?;
            for row in value["results"]
                .as_array()
                .ok_or_else(|| runtime("Unexpected CloudTorrents JSON"))?
            {
                let torrent = &row["torrent"];
                let title = torrent["name"]
                    .as_str()
                    .ok_or_else(|| runtime("CloudTorrents result has no title"))?;
                let uri = torrent["torrentMagnet"]
                    .as_str()
                    .ok_or_else(|| runtime("CloudTorrents result has no magnet"))?;
                let kind = torrent["torrentType"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase();
                let id = row["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| row["id"].to_string());
                let date = torrent["uploadedAt"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .and_then(|d| d.timestamp().try_into().ok());
                let size = json::number(&torrent["size"]).or_else(|| {
                    torrent["size"]
                        .as_str()
                        .and_then(super::super::limetorrents::parse_size)
                });
                collected.push(candidate(
                    &self.descriptor,
                    title.into(),
                    uri.into(),
                    Some(format!("{base}/{kind}/{id}")),
                    size,
                    json::number(&torrent["seeders"]),
                    json::number(&torrent["leechers"]),
                    date,
                    now(),
                )?);
            }
            if value.get("next").is_none() {
                return Err(runtime("CloudTorrents response lacks pagination state"));
            }
            if value["next"].is_null() {
                break;
            }
            if collected.len() > 10000 {
                return Err(runtime("CloudTorrents exceeds bounded result buffer"));
            }
            offset += 50;
        }
        collected.sort_by_key(|result| std::cmp::Reverse(result.observations[0].seeders));
        Ok(append(results, collected, limit))
    }
}

fn nyaa_category(id: &str, category: &str) -> Result<&'static str, SearchError> {
    if id == "sukebeisi" {
        return Ok(match category {
            "all" => "0_0",
            "anime" => "1_1",
            "books" => "1_2",
            "games" => "1_3",
            "pictures" => "2_1",
            "movies" => "2_2",
            _ => return Err(runtime("Unsupported Sukebei category")),
        });
    }
    Ok(match category {
        "all" => "0_0",
        "anime" => "1_0",
        "books" => "3_0",
        "music" => "2_0",
        "pictures" => "5_0",
        "software" => "6_0",
        "tv" | "movies" => "4_0",
        _ => return Err(runtime("Unsupported Nyaa category")),
    })
}

fn parse_nyaa(
    response: &str,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let decoded = html_escape::decode_html_entities(response);
    let document = Html::parse_document(&decoded);
    let mut results = vec![];
    for row in document.select(&Selector::parse("table tr").unwrap()) {
        let cells: Vec<_> = row.select(&Selector::parse("td").unwrap()).collect();
        if cells.len() < 7 {
            continue;
        }
        let Some(title) = row
            .select(&Selector::parse("a[href^='/view/'][title]:not([class])").unwrap())
            .next()
        else {
            continue;
        };
        let Some(magnet) = row
            .select(&Selector::parse("a[href^='magnet:']").unwrap())
            .next()
        else {
            continue;
        };
        let text = |i: usize| cells[i].text().collect::<String>().trim().to_owned();
        let published = if source.id == "nyaasi" {
            cells[4]
                .value()
                .attr("data-timestamp")
                .and_then(|s| s.parse().ok())
        } else {
            None
        };
        results.push(candidate(
            source,
            title.value().attr("title").unwrap().into(),
            magnet.value().attr("href").unwrap().into(),
            Some(format!(
                "{}{}",
                source.site_url.trim_end_matches('/'),
                title.value().attr("href").unwrap()
            )),
            super::super::limetorrents::parse_size(&text(3)),
            text(5).parse().ok(),
            text(6).parse().ok(),
            published,
            now,
        )?);
    }
    Ok(results)
}

fn parse_acgrip(
    response: &str,
    source: &SourceDescriptor,
    now: u64,
) -> Result<Vec<SearchResult>, SearchError> {
    let document = Html::parse_document(response);
    let mut results = vec![];
    for row in document.select(&Selector::parse("table tr").unwrap()) {
        let cells: Vec<_> = row.select(&Selector::parse("td").unwrap()).collect();
        if cells.len() < 4 {
            continue;
        }
        let Some(title) = cells[1]
            .select(&Selector::parse("a[href^='/t/']").unwrap())
            .next()
        else {
            continue;
        };
        let Some(download) = cells[2]
            .select(&Selector::parse("a[href$='.torrent']").unwrap())
            .next()
        else {
            continue;
        };
        let name = title
            .text()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let date = row
            .select(&Selector::parse("time[datetime]").unwrap())
            .next()
            .and_then(|t| t.value().attr("datetime"))
            .and_then(|s| s.parse().ok())
            .or(Some(now));
        let size = super::super::limetorrents::parse_size(&cells[3].text().collect::<String>());
        results.push(candidate(
            source,
            name,
            format!(
                "{}{}",
                source.site_url,
                download.value().attr("href").unwrap()
            ),
            Some(format!(
                "{}{}",
                source.site_url,
                title.value().attr("href").unwrap()
            )),
            size,
            None,
            None,
            date,
            now,
        )?);
    }
    Ok(results)
}
