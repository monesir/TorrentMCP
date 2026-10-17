//! Multi-hop public searches and source-specific POST download references.
use super::{NativeSource, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult, TorrentReference};
use chrono::TimeZone;
use scraper::{ElementRef, Html, Selector};
use std::{collections::BTreeMap, time::Duration};
pub(super) fn supports(id: &str) -> bool {
    matches!(id, "ali213" | "mypornclub" | "rutor" | "zooqle")
}
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("Static selector")
}
fn raw(n: ElementRef<'_>) -> String {
    n.text().collect::<String>()
}
fn cap(p: &str, s: &str) -> Option<String> {
    regex::Regex::new(p)
        .ok()?
        .captures(s)?
        .get(1)
        .map(|m| m.as_str().into())
}
fn size(s: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(s.trim())
}
fn num(s: &str) -> Option<u64> {
    s.trim().parse().ok()
}
fn sumdigits(s: &str) -> i64 {
    s.chars()
        .filter_map(|c| c.to_digit(10))
        .map(i64::from)
        .sum()
}
fn rutor_date(s: &str) -> Option<u64> {
    let mut value = html_escape::decode_html_entities(s).into_owned();
    for (i, m) in [
        "Янв", "Фев", "Мар", "Апр", "Май", "Июн", "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек",
    ]
    .iter()
    .enumerate()
    {
        value = value.replace(m, &format!("{:02}", i + 1));
    }
    let date = chrono::NaiveDate::parse_from_str(value.trim(), "%d %m %y")
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&date)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}
impl NativeSource {
    pub(super) async fn collect_advanced_source(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let q = percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        match id {
            "ali213" => {
                let response = self
                    .fetch(&format!("{base}/search?kw={q}&submit="), None)
                    .await?;
                let re=regex::Regex::new(&format!(r#"<p class="downAddress"><a href="{}/pcgame/(.*?)" target="_blank">.*?<em>(.{{2,7}})</em>"#,regex::escape(base))).unwrap();
                let games: Vec<_> = re
                    .captures_iter(&response)
                    .take(5)
                    .map(|c| (c[1].to_owned(), c[2].to_owned()))
                    .collect();
                let first = if base == "http://down.ali213.net" {
                    "http://www.soft50.com"
                } else {
                    base
                };
                let final_site = if base == "http://down.ali213.net" {
                    "http://btfile.soft5566.com"
                } else {
                    base
                };
                let mut tasks = tokio::task::JoinSet::new();
                for (path, bytes) in games {
                    let client = self.client.clone();
                    let source = NativeSource {
                        descriptor: self.descriptor.clone(),
                        client,
                        assets: self.assets.clone(),
                    };
                    let base = base.to_owned();
                    let first = first.to_owned();
                    let final_site = final_site.to_owned();
                    tasks.spawn(async move {
                        let response = source.fetch(&format!("{base}/pcgame/{path}"), None).await?;
                        let Some(key) = cap(r#"var downUrl ="/(.*?)""#, &response) else {
                            return Ok::<_, SearchError>(None);
                        };
                        let mut response = String::new();
                        for _ in 0..20 {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            response = source.fetch(&format!("{first}/{key}"), None).await?;
                            if !response.is_empty() {
                                break;
                            }
                        }
                        let Some(desc) = cap(
                            r#"class="result_js" href="(.*?)" target="_blank">"#,
                            &response,
                        ) else {
                            return Ok(None);
                        };
                        let response = source.fetch(&desc, None).await?;
                        let Some(key) = cap(
                            &format!(
                                r#"id="btbtn" href="{}/y/(.*?)" target="_blank""#,
                                regex::escape(&final_site)
                            ),
                            &response,
                        ) else {
                            return Ok(None);
                        };
                        Ok(Some(candidate(
                            &source.descriptor,
                            key.clone(),
                            format!("{final_site}/y/{key}"),
                            Some(desc),
                            size(&bytes),
                            None,
                            None,
                            None,
                            now(),
                        )?))
                    });
                }
                while let Some(row) = tasks.join_next().await {
                    if let Some(row) = row.map_err(|e| runtime(e.to_string()))?? {
                        results.push(row);
                        if results.len() >= limit {
                            tasks.abort_all();
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            "rutor" => {
                let c = match request.category.as_str() {
                    "movies" => 1,
                    "tv" => 6,
                    "music" => 2,
                    "games" => 8,
                    "anime" => 10,
                    "software" => 9,
                    "pictures" => 3,
                    "books" => 11,
                    _ => 0,
                };
                let config: serde_json::Value =
                    std::fs::read_to_string(self.assets.join("qbit/engines/rutor.json"))
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default();
                let magnet = config["magnet"].as_bool().unwrap_or(false);
                let download = if base == "https://rutor.info" {
                    "https://d.rutor.info/download".to_owned()
                } else {
                    format!("{base}/download")
                };
                let mut last = 1;
                let re=regex::Regex::new(r#"(?s)(?:gai|tum)"><td>(.+?)</td.+?href="(magnet:.+?)".+?href="/(torrent/(\d+).+?)">(.+?)</a.+?right">([.\d]+?&nbsp;\w+?)</td.+?<span.+?(\d+?)</span>.+?<span.+?(\d+?)</span>"#).unwrap();
                for page in 0.. {
                    let response = self
                        .fetch(&format!("{base}/search/{page}/{c}/000/0/{q}"), None)
                        .await?;
                    if page == 0 {
                        let total = cap(r"</b>\sРезультатов\sпоиска\s(\d{1,4})\s", &response)
                            .and_then(|s| s.parse::<usize>().ok())
                            .ok_or_else(|| runtime("Unexpected Rutor response"))?;
                        last = total.div_ceil(100);
                        if total == 0 {
                            break;
                        }
                    }
                    for row in re.captures_iter(&response) {
                        results.push(candidate(
                            &self.descriptor,
                            html_escape::decode_html_entities(&row[5]).into_owned(),
                            if magnet {
                                html_escape::decode_html_entities(&row[2]).into_owned()
                            } else {
                                format!("{download}/{}", &row[4])
                            },
                            Some(format!("{base}/{}", &row[3])),
                            size(&row[6].replace("&nbsp;", " ")),
                            num(&row[7]),
                            num(&row[8]),
                            rutor_date(&row[1]),
                            now(),
                        )?);
                        if results.len() >= limit {
                            return Ok(true);
                        }
                    }
                    if page + 1 >= last {
                        break;
                    }
                }
                Ok(false)
            }
            "zooqle" => {
                let c = match request.category.as_str() {
                    "all" => "0",
                    "software" => "apps",
                    "books" => "None",
                    s => s,
                };
                for page in 1.. {
                    let referer = format!("{base}/category/{c}/");
                    let response = self
                        .fetch_referer(
                            &format!("{base}/query/{q}/page/{page}"),
                            Some(&format!("q={}&page={page}", json::form_value(&q))),
                            if c == "0" {
                                None
                            } else {
                                Some(referer.as_str())
                            },
                        )
                        .await?;
                    let ids: Vec<_> = {
                        let doc = Html::parse_document(&response);
                        doc.select(&sel("tr input[type=hidden][name=id][value]"))
                            .map(|n| n.value().attr("value").unwrap().to_owned())
                            .collect()
                    };
                    let mut count = 0;
                    for identity in ids {
                        let details = self
                            .fetch(
                                &format!("{base}/torrent-page/"),
                                Some(&format!("id={}", json::form_value(&identity))),
                            )
                            .await?;
                        let doc = Html::parse_document(&details);
                        let get = |label: &str| {
                            cap(&format!(r"(?s)<strong>{label}</strong>.*?</li>\s*<li>:</li>\s*<li[^>]*>(.*?)</li>"),&details).map(|s|html_escape::decode_html_entities(&regex::Regex::new("<[^>]*>").unwrap().replace_all(&s,"")).trim().to_owned())
                        };
                        if c != "0"
                            && get("Category").as_deref().map(str::to_lowercase).as_deref()
                                != Some(c)
                        {
                            continue;
                        }
                        let title = cap(r"(?s)<title>(.*?)\s+Torrent - Zooqle</title>", &details)
                            .unwrap_or_else(|| "N/A".into());
                        let hid = doc
                            .select(&sel("input[name=hid][value]"))
                            .next()
                            .and_then(|n| n.value().attr("value"))
                            .map(str::to_owned);
                        let magnet = doc
                            .select(&sel("a[href^='magnet:']"))
                            .next()
                            .and_then(|n| n.value().attr("href"))
                            .map(str::to_owned);
                        let uri = if let Some(hid) = &hid {
                            format!("{base}/torfile/#hid={}", json::form_value(hid))
                        } else {
                            magnet.ok_or_else(|| runtime("Zooqle download reference missing"))?
                        };
                        let mut row = candidate(
                            &self.descriptor,
                            title,
                            uri,
                            Some(base.into()),
                            get("Size").as_deref().and_then(size),
                            get("Seed").as_deref().and_then(num),
                            get("Leech").as_deref().and_then(num),
                            None,
                            now(),
                        )?;
                        if let (Some(hid), TorrentReference::Http { post_form, .. }) =
                            (hid, &mut row.reference)
                        {
                            *post_form = Some(BTreeMap::from([("hid".into(), hid)]));
                        }
                        results.push(row);
                        count += 1;
                        if results.len() >= limit {
                            return Ok(true);
                        }
                    }
                    if count < 40 {
                        break;
                    }
                }
                Ok(false)
            }
            "mypornclub" => {
                let query = q.replace("%20", "-");
                let mut last = 1;
                for page in 1.. {
                    let response = self
                        .fetch(&format!("{base}/s/{query}/seeders/{page}"), None)
                        .await?;
                    if page == 1 {
                        last = cap(r"<div>Page\s\d\sof\s(\d+)</div>", &response)
                            .and_then(|s| s.parse().ok())
                            .ok_or_else(|| runtime("MyPornClub pagination missing"))?;
                    }
                    let rows = {
                        let doc = Html::parse_document(&response);
                        doc.select(&sel(".torrents_list .torrent_element"))
                            .filter(|row| {
                                row.value().classes().any(|c| c == "torrent_element")
                                    && row
                                        .select(&sel(".torrent_element_text_div"))
                                        .next()
                                        .is_some()
                            })
                            .filter_map(|row| {
                                let a = row
                                    .select(&sel(
                                        ".torrent_element_text_div a:not(.uploader_tel)[href]",
                                    ))
                                    .next()?;
                                let title =
                                    row.select(&sel(".torrent_element_text_span")).next()?;
                                let mut name = String::new();
                                for n in title.children() {
                                    if let Some(t) = n.value().as_text() {
                                        name.push_str(t.trim());
                                    } else if let Some(e) = ElementRef::wrap(n) {
                                        if e.value().name() == "b" {
                                            name.push_str(&format!("[{}]", raw(e)));
                                        }
                                        if e.value().name() == "i" {
                                            break;
                                        } else if e.value().name() != "b" {
                                            name.push_str(raw(e).trim());
                                        }
                                    }
                                }
                                let info = row.select(&sel(".torrent_element_info")).next()?;
                                let value = raw(info);
                                let date = info
                                    .select(&sel("a.linkadd"))
                                    .next()
                                    .map(raw)
                                    .unwrap_or_default();
                                let amount = date
                                    .split_whitespace()
                                    .next()
                                    .and_then(|s| s.parse::<u64>().ok());
                                let unit = date.split_whitespace().nth(1).and_then(|u| match u {
                                    "minutes" => Some(60),
                                    "hours" => Some(3600),
                                    "days" => Some(86400),
                                    "months" => Some(30 * 86400),
                                    "years" => Some(365 * 86400),
                                    _ => None,
                                });
                                Some((
                                    name,
                                    format!("{base}{}", a.value().attr("href")?),
                                    cap(r"\[size\]:\s*([0-9.,]+\s*\w+)", &value)
                                        .as_deref()
                                        .and_then(|s| size(&s.replace(',', "."))),
                                    cap(r"\[seeders\]:\s*(\d+)", &value)
                                        .as_deref()
                                        .and_then(num),
                                    cap(r"\[leechers\]:\s*(\d+)", &value)
                                        .as_deref()
                                        .and_then(num),
                                    amount.zip(unit).map(|(a, u)| now().saturating_sub(a * u)),
                                ))
                            })
                            .collect::<Vec<_>>()
                    };
                    for (mut title, desc, bytes, seeds, leech, date) in rows {
                        let details = self.fetch(&desc, None).await?;
                        let mut uri = html::resolve_magnet(&details, id)?.uri().to_owned();
                        if let Some(web) =
                            cap(r"(sxyprn\.com[^\w]*?post[^\w]*?[\w]*?\.html)", &details)
                        {
                            let web = format!("https://{}", web.replace('\\', ""));
                            let details = self.fetch(&web, None).await?;
                            if let Some(encoded) = cap(r#"data-vnfo=["'](\{.+?\})["']"#, &details) {
                                let data: serde_json::Value = serde_json::from_str(&encoded)?;
                                let key = web
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or("")
                                    .split('.')
                                    .next()
                                    .unwrap_or("");
                                if let Some(path) = data[key].as_str() {
                                    let mut parts =
                                        path.split('/').map(str::to_owned).collect::<Vec<_>>();
                                    if parts.len() > 7 {
                                        let token = data_encoding::BASE64
                                            .encode(
                                                format!(
                                                    "{}-sxyprn.com-{}",
                                                    sumdigits(&parts[6]),
                                                    sumdigits(&parts[7])
                                                )
                                                .as_bytes(),
                                            )
                                            .replace('+', "-")
                                            .replace('/', "_")
                                            .replace('=', ".");
                                        parts[1] = format!("{}8/{token}", parts[1]);
                                        let value = parts[5]
                                            .parse::<i64>()
                                            .map_err(|_| runtime("Invalid web seed metadata"))?
                                            - sumdigits(&parts[6])
                                            - sumdigits(&parts[7]);
                                        parts[5] = value.to_string();
                                        let webseed =
                                            format!("https://sxyprn.com{}", parts.join("/"));
                                        // Resolve headers only; do not download the video web seed.
                                        let final_url = self
                                            .client
                                            .get(&webseed)
                                            .send()
                                            .await
                                            .map_err(super::http_error)?
                                            .error_for_status()
                                            .map_err(super::http_error)?
                                            .url()
                                            .to_string();
                                        uri.push_str(&format!("&ws={webseed}&ws={final_url}"));
                                        title = format!("💥 {title}");
                                    }
                                }
                            }
                        }
                        results.push(candidate(
                            &self.descriptor,
                            title,
                            uri,
                            Some(desc),
                            bytes,
                            seeds,
                            leech,
                            date,
                            now(),
                        )?);
                        if results.len() >= limit {
                            return Ok(true);
                        }
                    }
                    if page >= last {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Ok(false)
            }
            _ => Err(runtime("Unknown advanced source")),
        }
    }
}
