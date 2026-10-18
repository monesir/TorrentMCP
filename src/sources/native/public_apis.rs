use super::{NativeSource, append, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use chrono::TimeZone;
use serde_json::Value;

pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "nekobt"
            | "tsukihime"
            | "yts"
            | "yggtracker"
            | "goggames"
            | "snowfl"
            | "snowfl_community_052"
            | "pantsu"
            | "sukebei"
            | "dodi_repacks"
            | "onlinefix"
    )
}
fn quote(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, html::QUERY_SAFE).to_string()
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn formatted_size(value: &Value) -> Option<u64> {
    let mut size = value.as_f64().or_else(|| value.as_str()?.parse().ok())?;
    if size < 0. || !size.is_finite() {
        return None;
    }
    let mut unit = 0;
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    while size >= 1024. && unit < 5 {
        size /= 1024.;
        unit += 1;
    }
    super::super::limetorrents::parse_size(&format!("{size:.2} {}", units[unit]))
}
fn local_iso(value: &str) -> Option<u64> {
    let value = value.split('.').next()?.replace('T', " ");
    let date = chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S").ok()?;
    chrono::Local
        .from_local_datetime(&date)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}

impl NativeSource {
    pub(super) async fn collect_public_api(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let base = self.descriptor.site_url.trim_end_matches('/');
        let query = quote(&request.query);
        match self.descriptor.id.as_str() {
            "dodi_repacks" | "onlinefix" => {
                let dodi = self.descriptor.id == "dodi_repacks";
                let endpoint =
                    if base == "https://dodi-repacks.site" || base == "https://online-fix.me" {
                        "https://hydralinks.cloud"
                    } else {
                        base
                    };
                let response = self
                    .fetch(
                        &format!(
                            "{endpoint}/sources/{}.json",
                            if dodi { "dodi" } else { "onlinefix" }
                        ),
                        None,
                    )
                    .await?;
                let data: Value = serde_json::from_str(&response)?;
                let terms: Vec<_> = request
                    .query
                    .split_whitespace()
                    .map(str::to_lowercase)
                    .collect();
                for row in data["downloads"]
                    .as_array()
                    .ok_or_else(|| runtime("Hydra downloads array missing"))?
                {
                    let title = text(row, "title");
                    if !terms.iter().any(|term| title.to_lowercase().contains(term)) {
                        continue;
                    }
                    let uri = row["uris"][0]
                        .as_str()
                        .ok_or_else(|| runtime("Hydra download URI missing"))?;
                    let date = if dodi {
                        Some(
                            chrono::NaiveDateTime::parse_from_str(
                                text(row, "uploadDate"),
                                "%Y-%m-%dT%H:%M:%S%.fZ",
                            )
                            .map_err(|_| runtime("Invalid Hydra upload date"))?
                            .and_utc()
                            .timestamp()
                            .try_into()
                            .map_err(|_| runtime("Invalid Hydra upload date"))?,
                        )
                    } else {
                        None
                    };
                    results.push(candidate(
                        &self.descriptor,
                        title.into(),
                        uri.into(),
                        None,
                        super::super::limetorrents::parse_size(text(row, "fileSize")),
                        None,
                        None,
                        date,
                        now(),
                    )?);
                    if results.len() >= limit {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "pantsu" | "sukebei" => {
                let category = match (self.descriptor.id.as_str(), request.category.as_str()) {
                    (_, "all") => "_",
                    ("pantsu", "anime") => "3_",
                    ("pantsu", "music") => "2_",
                    ("pantsu", "pictures") => "6_",
                    ("pantsu", "software") => "1_",
                    ("pantsu", "games") => "1_2",
                    ("sukebei", "anime") => "1_1",
                    ("sukebei", "pictures") => "1_",
                    ("sukebei", "games") => "1_3",
                    (_, "books") => "4_",
                    _ => return Err(runtime("Unsupported Pantsu API category")),
                };
                let endpoint = format!("{base}/api/search/?c={category}&q={query}");
                let response = self
                    .fetch(&format!("{endpoint}&limit=1&page=1"), None)
                    .await?;
                let data: Value = serde_json::from_str(&response)?;
                let count = json::number(&data["totalRecordCount"])
                    .ok_or_else(|| runtime("Pantsu API total count missing"))?;
                // Integer ceiling also fixes the original Python float/range
                // bug when totalRecordCount is an exact multiple of 100.
                for page in 1..=count.div_ceil(100) {
                    let response = self
                        .fetch(&format!("{endpoint}&limit=100&page={page}"), None)
                        .await?;
                    let data: Value = serde_json::from_str(&response)?;
                    for row in data["torrents"]
                        .as_array()
                        .ok_or_else(|| runtime("Unexpected Pantsu API JSON"))?
                    {
                        results.push(candidate(
                            &self.descriptor,
                            text(row, "name").into(),
                            text(row, "magnet").into(),
                            Some(format!("{base}/view/{}", scalar(&row["id"]))),
                            json::number(&row["filesize"]),
                            json::number(&row["seeders"]),
                            json::number(&row["leechers"]),
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
            "nekobt" => {
                let response = self
                    .fetch(
                        &format!(
                            "{base}/api/v1/torrents/search?query={}",
                            json::form_value(&request.query)
                        ),
                        None,
                    )
                    .await?;
                let value: Value = serde_json::from_str(&response)?;
                let rows = value
                    .as_array()
                    .or_else(|| value["data"].as_array())
                    .or_else(|| value["data"]["results"].as_array())
                    .or_else(|| value["data"]["torrents"].as_array())
                    .ok_or_else(|| runtime("Unexpected NekoBT JSON"))?;
                for row in rows {
                    let name = row["title"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| row["auto_title"].as_str())
                        .unwrap_or("Desconocido");
                    let id = scalar(&row["id"]);
                    let uri = row["magnet"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| {
                            row["infohash"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(|hash| {
                                    format!("magnet:?xt=urn:btih:{hash}&dn={}", quote(name))
                                })
                        })
                        .unwrap_or_else(|| format!("{base}/api/v1/torrents/download/{id}"));
                    let uploaded = row
                        .get("uploaded_at")
                        .or_else(|| row.get("created_at"))
                        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                        .filter(|n| *n >= 0. && n.is_finite())
                        .map(|n| (n / 1000.) as u64);
                    results.push(candidate(
                        &self.descriptor,
                        name.into(),
                        uri,
                        Some(format!("{base}/torrents/{id}")),
                        formatted_size(row.get("filesize").unwrap_or(&Value::from(0))),
                        json::number(row.get("seeders").unwrap_or(&Value::from(0))),
                        json::number(row.get("leechers").unwrap_or(&Value::from(0))),
                        uploaded,
                        now(),
                    )?);
                    if results.len() >= limit {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "tsukihime" => {
                let api = if base == "https://tsukihime.org" {
                    "https://api.tsukihime.org"
                } else {
                    base
                };
                let response = self
                    .fetch(&format!("{api}/v1/search/torrents?q={query}"), None)
                    .await?;
                let value: Value = serde_json::from_str(&response)?;
                for row in value["results"]
                    .as_array()
                    .ok_or_else(|| runtime("Unexpected TsukiHime JSON"))?
                {
                    let id = scalar(&row["id"]);
                    let name = text(row, "name");
                    // The search row already names the torrent; the detail only adds
                    // trackers and swarm counts, so a missing or failed detail keeps
                    // the result with unknown counts instead of failing the source.
                    let info: Value =
                        match self.fetch(&format!("{api}/v1/torrents/{id}"), None).await {
                            Ok(response) => serde_json::from_str(&response).unwrap_or(Value::Null),
                            Err(_) => Value::Null,
                        };
                    let trackers = info["trackers"].as_array();
                    let mut seeds = 0u64;
                    let mut leeches = 0u64;
                    let mut uri = format!(
                        "magnet:?xt=urn:btih:{}&dn={}",
                        text(row, "btih"),
                        quote(name).replace('/', "%2F")
                    );
                    for tracker in trackers.into_iter().flatten() {
                        seeds =
                            seeds.saturating_add(json::number(&tracker["seeders"]).unwrap_or(0));
                        leeches =
                            leeches.saturating_add(json::number(&tracker["leechers"]).unwrap_or(0));
                        uri.push_str(&format!(
                            "&tr={}",
                            quote(text(tracker, "url")).replace('/', "%2F")
                        ));
                    }
                    results.push(candidate(
                        &self.descriptor,
                        name.into(),
                        uri,
                        Some(format!("https://tsukihime.org/view/{id}")),
                        json::number(&row["totalsize"]),
                        trackers.map(|_| seeds),
                        trackers.map(|_| leeches),
                        json::number(&row["source_date"]),
                        now(),
                    )?);
                    if results.len() >= limit {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "yggtracker" => {
                let registry = if base == "https://github.com/YGGverse/YGGtracker" {
                    "https://raw.githubusercontent.com/YGGverse/qbittorrent-yggtracker-search-plugin/main/nodes.json".into()
                } else {
                    format!("{base}/nodes.json")
                };
                let response = self.fetch(&registry, None).await?;
                let value: Value = serde_json::from_str(&response)?;
                for node in value
                    .as_array()
                    .ok_or_else(|| runtime("Unexpected YGGtracker node registry"))?
                {
                    let mut query =
                        format!("query={}&filter=true", json::form_value(&request.query));
                    let categories = node["categories"][&request.category]
                        .as_array()
                        .ok_or_else(|| runtime("Node lacks requested category mapping"))?;
                    if !categories.is_empty() {
                        query.push_str(&format!(
                            "&categories={}",
                            json::form_value(
                                &categories.iter().map(scalar).collect::<Vec<_>>().join("|")
                            )
                        ));
                    }
                    let url = text(node, "url").replacen("%s", &query, 1);
                    if !url.starts_with("http://") && !url.starts_with("https://") {
                        return Err(runtime("YGGtracker node URL must be HTTP(S)"));
                    }
                    let response = self.fetch(&url, None).await?;
                    let data: Value = serde_json::from_str(&response)?;
                    let mut parsed = vec![];
                    for item in data["torrents"]
                        .as_array()
                        .ok_or_else(|| runtime("Unexpected YGGtracker JSON"))?
                    {
                        let torrent = &item["torrent"];
                        let file = &torrent["file"];
                        let scrape = &torrent["scrape"];
                        parsed.push(candidate(
                            &self.descriptor,
                            text(file, "name").into(),
                            text(file, "url").into(),
                            Some(text(torrent, "url").into()),
                            json::number(&file["size"]),
                            json::number(&scrape["seeders"]),
                            json::number(&scrape["leechers"]),
                            None,
                            now(),
                        )?);
                    }
                    if append(results, parsed, limit) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "goggames" => {
                let query = request.query.trim().to_lowercase();
                let recent = [".", "*", "!"].contains(&query.as_str());
                let endpoint = if recent {
                    format!("{base}/api/web/recent-torrents")
                } else {
                    format!(
                        "{base}/search?page=1&search={}&sort_by=lastUpdateDescending",
                        quote(&query)
                    )
                };
                let response = self.fetch(&endpoint, None).await?;
                let data: Value = serde_json::from_str(&response)?;
                let rows = data
                    .as_array()
                    .or_else(|| data["data"].as_array())
                    .ok_or_else(|| runtime("Unexpected GOG-Games JSON"))?;
                // The original emits a fake no-results torrent. An empty array
                // is a successful empty search under the engine contract.
                for row in rows {
                    let mut title = row["title"]
                        .as_str()
                        .unwrap_or("Juego Desconocido")
                        .chars()
                        .filter(|c| c.is_ascii())
                        .collect::<String>()
                        .replace('|', "-");
                    if recent {
                        title = format!("[{query}] {title}");
                    }
                    let uri = if let Some(hash) = row["infohash"].as_str().filter(|s| !s.is_empty())
                    {
                        format!("magnet:?xt=urn:btih:{hash}&dn={}", quote(&title))
                    } else {
                        title.push_str(" - No torrent");
                        base.into()
                    };
                    let published = row["torrent_date"]
                        .as_str()
                        .or_else(|| row["last_update"].as_str())
                        .and_then(local_iso);
                    results.push(candidate(
                        &self.descriptor,
                        title,
                        uri,
                        Some(format!("{base}/game/{}", text(row, "slug"))),
                        None,
                        None,
                        None,
                        published,
                        now(),
                    )?);
                    if results.len() >= limit {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "snowfl" | "snowfl_community_052" => self.collect_snowfl(request, limit, results).await,
            "yts" => self.collect_yts(request, limit, results).await,
            _ => Err(runtime("No public API adapter")),
        }
    }

    async fn collect_snowfl(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let base = self.descriptor.site_url.trim_end_matches('/');
        let modern = self.descriptor.id.ends_with("052");
        let home = self
            .fetch(
                &format!("{base}/{}", if modern { "" } else { "index.html" }),
                None,
            )
            .await?;
        let js = regex::Regex::new(r#"(b\.min\.js[^"\s<]*)"#)
            .unwrap()
            .captures(&home)
            .map(|m| m[1].to_owned())
            .ok_or_else(|| runtime("Snowfl script link not found"))?;
        let script = self.fetch(&format!("{base}/{js}"), None).await?;
        let pattern = if modern {
            r#"findNextItem.*?"([^"]+)""#
        } else {
            r#""([a-zA-Z0-9]+)";\$\(\(function\(\)\{var e,t,n,r,o,a,i="#
        };
        let token = regex::Regex::new(pattern)
            .unwrap()
            .captures(&script)
            .map(|m| m[1].to_owned())
            .ok_or_else(|| runtime("Snowfl session token not found"))?;
        let query = quote(&request.query);
        if query.len() <= 2 {
            return Err(runtime(
                "Snowfl query must contain at least three characters",
            ));
        }
        let url = if modern {
            format!("{base}/{token}/{query}/DH5kKsJw/0/SEED/NONE/0")
        } else {
            let milliseconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            // Only a cache-buster; no cryptographic or persistent identity.
            format!(
                "{base}//{token}/{query}/{:08x}/0/SEED/NONE/1?_={milliseconds}",
                milliseconds as u32
            )
        };
        let response = self.fetch(&url, None).await?;
        let data: Value = serde_json::from_str(&response)?;
        let observed = now();
        for row in data
            .as_array()
            .ok_or_else(|| runtime("Unexpected Snowfl JSON"))?
        {
            let mut uri = text(row, "magnet").to_owned();
            if modern && uri.is_empty() {
                let resolution = format!(
                    "{base}/{token}/{}/{}",
                    text(row, "site"),
                    quote(text(row, "url"))
                );
                if let Ok(response) = self.fetch(&resolution, None).await
                    && let Ok(response) = serde_json::from_str::<Value>(&response)
                {
                    uri = text(&response, "url").into();
                }
            }
            if uri.is_empty() {
                uri = text(row, "url").into();
            }
            let mut title = text(row, "name").to_owned();
            if modern && !text(row, "site").is_empty() {
                title.push_str(&format!(" [{}]", text(row, "site")));
            }
            let published = if modern {
                relative_age(text(row, "age"), observed)
            } else {
                None
            };
            results.push(candidate(
                &self.descriptor,
                title,
                uri,
                Some(text(row, "url").into()),
                row["size"]
                    .as_str()
                    .and_then(super::super::limetorrents::parse_size),
                json::number(&row["seeder"]),
                json::number(&row["leecher"]),
                published,
                observed,
            )?);
            if results.len() >= limit {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn collect_yts(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let base = self.descriptor.site_url.trim_end_matches('/');
        let api = if base == "https://yts.bz" {
            "https://movies-api.accel.li/api/v2/list_movies.json".into()
        } else {
            format!("{base}/api/v2/list_movies.json")
        };
        let query = yts_query(&request.query);
        let url = format!("{api}?{query}");
        let mut pages = 0usize;
        for page in 0..usize::MAX {
            let url = if page == 0 {
                url.clone()
            } else {
                format!("{url}&page={page}")
            };
            let response = self.fetch(&url, None).await?;
            let data: Value = serde_json::from_str(&response)?;
            if data["status"] != "ok" {
                return Err(runtime("YTS API reported a failed status"));
            }
            let data = &data["data"];
            if page == 0 {
                let per_page = json::number(&data["limit"])
                    .filter(|&n| n > 0)
                    .ok_or_else(|| runtime("Invalid YTS page size"))?;
                pages = (json::number(&data["movie_count"]).unwrap_or(0) / per_page) as usize;
            }
            if let Some(movies) = data["movies"].as_array() {
                for movie in movies {
                    if let Some(torrents) = movie["torrents"].as_array() {
                        for torrent in torrents {
                            let title = movie["title_long"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| text(movie, "title"));
                            // Preserve the original optional-field spacing and labels.
                            let field = |key: &str| {
                                if let Some(value) = torrent[key].as_str() {
                                    if value.is_empty() {
                                        String::new()
                                    } else {
                                        format!("[{value}]")
                                    }
                                } else {
                                    String::from("None")
                                }
                            };
                            let title = format!(
                                "{title} {} {} {} {} [YTS]",
                                field("quality"),
                                field("video_codec"),
                                field("type"),
                                field("audio_channels")
                            );
                            results.push(candidate(
                                &self.descriptor,
                                title,
                                text(torrent, "url").into(),
                                Some(text(movie, "url").into()),
                                torrent["size"]
                                    .as_str()
                                    .and_then(super::super::limetorrents::parse_size),
                                json::number(&torrent["seeds"]),
                                json::number(&torrent["peers"]),
                                json::number(&torrent["date_uploaded_unix"]),
                                now(),
                            )?);
                            if results.len() >= limit {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            if page >= pages {
                break;
            }
        }
        Ok(false)
    }
}

fn relative_age(value: &str, now: u64) -> Option<u64> {
    let regex =
        regex::Regex::new(r"(?i)(\d+)\s*(second|minute|hour|day|week|month|year)s?").unwrap();
    let captures = regex.captures(value)?;
    let amount: u64 = captures[1].parse().ok()?;
    let unit = match captures[2].to_lowercase().as_str() {
        "second" => 1,
        "minute" => 60,
        "hour" => 3600,
        "day" => 86400,
        "week" => 604800,
        "month" => 2592000,
        _ => 31536000,
    };
    now.checked_sub(amount.checked_mul(unit)?)
}
fn yts_query(query: &str) -> String {
    let mut query = query.to_owned();
    let mut params = Vec::new();
    let quality = regex::Regex::new(r"(?:quality=)?((?:2160|1440|1080|720|480|240)p|3D)").unwrap();
    if let Some(capture) = quality.captures(&query) {
        params.push(("quality", capture[1].to_owned()));
        query = quality.replace_all(&query, "").trim().into();
    }
    let codec = regex::Regex::new(r"(?:x|h)(264|265)").unwrap();
    if let Some(capture) = codec.captures(&query) {
        if let Some((_, quality)) = params.iter_mut().find(|(key, _)| *key == "quality") {
            quality.push_str(&format!(".x{}", &capture[1]));
        }
        query = codec.replace_all(&query, "").trim().into();
    }
    for (key, pattern) in [
        ("minimum_rating", r"(?:min(?:imum)?_)?rating=(\d)"),
        ("genre", r"genre=(\w+)"),
    ] {
        let regex = regex::Regex::new(pattern).unwrap();
        if let Some(capture) = regex.captures(&query) {
            params.push((key, capture[1].to_owned()));
            query = regex.replace_all(&query, "").trim().into();
        }
    }
    query = regex::Regex::new(r"&page=\d+")
        .unwrap()
        .replace_all(&query, "")
        .trim()
        .into();
    if !query.is_empty() {
        params.push(("query_term", query));
    }
    params
        .into_iter()
        .map(|(key, value)| format!("{key}={}", json::form_value(&value)))
        .collect::<Vec<_>>()
        .join("&")
}
