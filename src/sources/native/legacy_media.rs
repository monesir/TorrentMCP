//! Legacy public French and Spanish plugins; variants keep their own identity.
use super::{NativeSource, candidate, html, json, now, runtime};
use crate::{SearchError, SearchRequest, SearchResult};
use chrono::TimeZone;
use scraper::{ElementRef, Html, Selector};
use std::collections::{HashSet, VecDeque};
pub(super) fn supports(id: &str) -> bool {
    matches!(
        id,
        "cpasbien"
            | "torrent9"
            | "dontorrent_community_020"
            | "elitetorrent"
            | "maxitorrent"
            | "mejortorrent"
            | "pediatorrent_community_043"
    )
}
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("Static selector")
}
fn raw(n: ElementRef<'_>) -> String {
    n.text().collect::<String>()
}
fn field(n: ElementRef<'_>, s: &str) -> String {
    n.select(&sel(s)).next().map(raw).unwrap_or_default()
}
fn href(n: ElementRef<'_>) -> String {
    n.value().attr("href").unwrap_or("").into()
}
fn cap(p: &str, s: &str) -> Option<String> {
    regex::Regex::new(p)
        .ok()?
        .captures(s)?
        .get(1)
        .map(|s| s.as_str().into())
}
fn num(s: &str) -> Option<u64> {
    s.trim().replace(',', "").parse().ok()
}
fn size(s: &str) -> Option<u64> {
    super::super::limetorrents::parse_size(s.trim())
}
fn date(s: &str, fmt: &str) -> Option<u64> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), fmt)
        .ok()?
        .and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&d)
        .earliest()?
        .timestamp()
        .try_into()
        .ok()
}
fn join(base: &str, h: &str) -> Result<String, SearchError> {
    url::Url::parse(base)
        .and_then(|u| u.join(h))
        .map(String::from)
        .map_err(|e| runtime(e.to_string()))
}
fn links(response: &str, selector: &str) -> Vec<String> {
    let doc = Html::parse_document(response);
    doc.select(&sel(selector)).map(href).collect()
}
fn deobfuscate(s: &str) -> Option<String> {
    let mut value = s.as_bytes().to_vec();
    for _ in 0..10 {
        value = data_encoding::BASE64.decode(&value).ok()?;
        let decoded = String::from_utf8(value.clone())
            .ok()?
            .chars()
            .map(|c| match c {
                'a'..='m' | 'A'..='M' => char::from_u32(c as u32 + 13).unwrap(),
                'n'..='z' | 'N'..='Z' => char::from_u32(c as u32 - 13).unwrap(),
                _ => c,
            })
            .collect::<String>();
        if decoded.contains("magnet") {
            return Some(decoded);
        }
    }
    None
}
struct Row {
    title: String,
    uri: String,
    desc: Option<String>,
    size: Option<u64>,
    seeds: Option<u64>,
    leech: Option<u64>,
    date: Option<u64>,
}
impl Row {
    fn new(title: String, uri: String, desc: Option<String>) -> Self {
        Self {
            title,
            uri,
            desc,
            size: None,
            seeds: None,
            leech: None,
            date: None,
        }
    }
}
impl NativeSource {
    pub(super) async fn legacy_site(&self) -> Result<String, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let actual = if (id == "cpasbien" && base == "http://www.cpasbien.fr")
            || (id == "torrent9" && base == "http://torent9.fr")
        {
            let registry = if id == "cpasbien" {
                "https://raw.githubusercontent.com/MarcBresson/cpasbien/master/cpasbien.url"
            } else {
                "https://raw.githubusercontent.com/menegop/qbfrench/master/urls.json"
            };
            match self.fetch(registry, None).await {
                Ok(s) if id == "cpasbien" => s.trim().into(),
                Ok(s) => serde_json::from_str::<serde_json::Value>(&s)?["torrent9"][0]
                    .as_str()
                    .ok_or_else(|| runtime("Torrent9 domain missing"))?
                    .into(),
                Err(_) => {
                    if id == "cpasbien" {
                        "http://www.cpasbien.biz".into()
                    } else {
                        "https://www.torrent9.fm".into()
                    }
                }
            }
        } else {
            base.to_owned()
        };
        Ok(actual)
    }

    pub(super) async fn collect_legacy_media(
        &self,
        request: &SearchRequest,
        limit: usize,
        results: &mut Vec<SearchResult>,
    ) -> Result<bool, SearchError> {
        let id = self.descriptor.id.as_str();
        let base = self.descriptor.site_url.trim_end_matches('/');
        let q = percent_encoding::utf8_percent_encode(&request.query, html::QUERY_SAFE).to_string();
        let mut rows = Vec::new();
        match id {
            "cpasbien" | "torrent9" => {
                let actual = self.legacy_site().await?;
                let mut seen = HashSet::new();
                for page in 0..10 {
                    let route = if id == "cpasbien" {
                        format!("{actual}/recherche/{q}/{}", page * 50 + 1)
                    } else {
                        format!("{actual}/search_torrent/{q}/page-{}", page + 1)
                    };
                    let response = self.fetch(&route, None).await?;
                    let old = rows.len();
                    let doc = Html::parse_document(&response);
                    for tr in doc.select(&sel(if id == "cpasbien" {
                        "table.table-corps tr"
                    } else {
                        "tbody tr"
                    })) {
                        let Some(a) = tr
                            .select(&sel(if id == "cpasbien" {
                                "a.titre[href]"
                            } else {
                                "a[href]"
                            }))
                            .next()
                        else {
                            continue;
                        };
                        let desc = format!("{actual}{}", href(a));
                        if !seen.insert(desc.clone()) {
                            continue;
                        }
                        let mut r = Row::new(
                            if id == "cpasbien" {
                                raw(a)
                            } else {
                                field(tr, "h3")
                            },
                            desc.clone(),
                            Some(desc),
                        );
                        let (bytes, seeds, leech) = if id == "cpasbien" {
                            (field(tr, ".poid"), field(tr, ".up"), field(tr, ".down"))
                        } else {
                            let values: Vec<_> = tr
                                .descendants()
                                .filter_map(|n| n.value().as_text().map(|t| (n, t)))
                                .filter(|(n, _)| {
                                    !n.ancestors()
                                        .filter_map(ElementRef::wrap)
                                        .any(|e| e.value().name() == "h3")
                                })
                                .map(|(_, t)| t.to_string())
                                .collect();
                            (
                                values.get(3).cloned().unwrap_or_default(),
                                values.get(5).cloned().unwrap_or_default(),
                                values.get(7).cloned().unwrap_or_default(),
                            )
                        };
                        let bytes = super::static_regex(r"(?i)([KMGTP])o")
                            .replace_all(&bytes, "${1}B");
                        r.size = size(&bytes);
                        r.seeds = num(&seeds);
                        r.leech = num(&leech);
                        rows.push(r);
                    }
                    if rows.len() == old {
                        break;
                    }
                }
                rows.sort_by_key(|r| {
                    std::cmp::Reverse((r.seeds.unwrap_or(0), r.leech.unwrap_or(0)))
                });
            }
            "dontorrent_community_020" => {
                let route = format!("{base}/buscar/{}", q.replace('+', "%20"));
                let response = self.fetch(&route, None).await?;
                let pages = {
                    let doc = Html::parse_document(&response);
                    let count = doc
                        .select(&sel("p.lead"))
                        .nth(1)
                        .map(|n| field(n, "b"))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
                    if count > 0 {
                        doc.select(&sel("a.page-link")).count().saturating_sub(2)
                    } else {
                        0
                    }
                };
                let mut found = Vec::new();
                for page in 2..=pages {
                    found.extend(links(
                        &self.fetch(&format!("{route}/page/{page}"), None).await?,
                        "a.text-decoration-none[href]",
                    ));
                }
                found.extend(links(
                    &self.fetch(&route, None).await?,
                    "a.text-decoration-none[href]",
                ));
                for link in found {
                    let response = self.fetch(&format!("{base}{link}"), None).await?;
                    let doc = Html::parse_document(&response);
                    let name = link.rsplit('/').next().unwrap_or("").replace('-', " ");
                    if link.split('/').nth(1) == Some("serie") {
                        for tr in doc.select(&sel("tr")).skip(1) {
                            let td: Vec<_> = tr.select(&sel("td")).collect();
                            if td.len() < 2 {
                                continue;
                            }
                            let Some(a) = td[1].select(&sel("a[href]")).next() else {
                                continue;
                            };
                            rows.push(Row::new(
                                format!("{name} {}", raw(td[0])),
                                format!("https:{}", href(a)),
                                Some(link.clone()),
                            ));
                        }
                    } else {
                        let Some(a) = doc
                            .select(&sel("a.text-white.bg-primary.rounded-pill[href]"))
                            .next()
                        else {
                            continue;
                        };
                        let mut r =
                            Row::new(name, format!("https:{}", href(a)), Some(link.clone()));
                        let info: Vec<_> = doc.select(&sel("p.mb-0")).collect();
                        if let Some(n) = info.get(if info.len() == 2 { 1 } else { 0 }) {
                            let texts: Vec<_> = n.text().collect();
                            r.size = texts.last().and_then(|s| size(&s.replace(',', ".")));
                        }
                        rows.push(r);
                    }
                }
            }
            "pediatorrent_community_043" => {
                let response = self.fetch(&format!("{base}/buscar?q={q}"), None).await?;
                let count = {
                    let doc = Html::parse_document(&response);
                    let value = doc
                        .select(&sel("p.text-2xl.text-lime-500.text-center"))
                        .next()
                        .map(raw)
                        .unwrap_or_default();
                    super::static_regex(r"\d+")
                        .find_iter(&value)
                        .nth(2)
                        .and_then(|m| m.as_str().parse::<usize>().ok())
                        .unwrap_or(0)
                };
                let mut found = Vec::new();
                for page in 1..=count / 17 + 1 {
                    found.extend(links(
                        &self
                            .fetch(&format!("{base}/buscar/page/{page}?q={q}"), None)
                            .await?,
                        "a[href]",
                    ));
                }
                for link in found {
                    let last = link.rsplit('/').next().unwrap_or("");
                    if ["dmca", "ayuda", "documentales", "peliculas"].contains(&last)
                        || last.starts_with("buscar?q=")
                        || last.starts_with("1?q=")
                    {
                        continue;
                    }
                    let response = self.fetch(&link, None).await?;
                    let doc = Html::parse_document(&response);
                    let name = last.split('-').skip(1).collect::<Vec<_>>().join(" ");
                    if link.split('/').nth(3) == Some("series") {
                        for td in doc.select(&sel(
                            "td.px-6.py-4.whitespace-nowrap.text-right.text-sm.font-medium.ml-auto",
                        )) {
                            if let Some(a) = td.select(&sel("a[href]")).next() {
                                let h = href(a);
                                rows.push(Row::new(
                                    format!("{name}{h}"),
                                    format!("{base}/{h}", h = h.trim_start_matches('/')),
                                    Some(link.clone()),
                                ));
                            }
                        }
                    } else if let Some(a) = doc.select(&sel("a[href]")).nth(11) {
                        rows.push(Row::new(
                            name,
                            join(&format!("{base}/"), &href(a))?,
                            Some(link.clone()),
                        ));
                    }
                }
            }
            "mejortorrent" => {
                let mut last = 1;
                let mut found = Vec::new();
                for page in 1..=10 {
                    let route = if page == 1 {
                        format!("{base}/busqueda?q={q}")
                    } else {
                        format!("{base}/busqueda/page/{page}?q={q}")
                    };
                    let response = self.fetch(&route, None).await?;
                    if page == 1 {
                        last = super::static_regex(r#""go to page (\d+)""#)
                            .captures_iter(&response)
                            .filter_map(|c| c[1].parse::<usize>().ok())
                            .max()
                            .unwrap_or(1);
                    }
                    let kind = match request.category.as_str() {
                        "movies" => "pelicula",
                        "tv" => "serie",
                        _ => "(?:pelicula|serie)",
                    };
                    let pattern = format!(r#"{}/{kind}/[0-9]+/[^\"]+"#, regex::escape(base));
                    found.extend(
                        regex::Regex::new(&pattern)
                            .unwrap()
                            .find_iter(&response)
                            .map(|m| m.as_str().to_owned()),
                    );
                    if page >= last {
                        break;
                    }
                }
                for desc in found {
                    let response = self.fetch(&desc, None).await?;
                    let doc = Html::parse_document(&response);
                    let title = doc
                        .select(&sel(".text-blue-900"))
                        .next()
                        .map(raw)
                        .unwrap_or_default();
                    if desc.contains("/pelicula/") {
                        let quality = cap(r#"/quality/([^\"]+)"#, &response).unwrap_or_default();
                        let path = cap(r"(/torrents/.+\.torrent)", &response)
                            .ok_or_else(|| runtime("MejorTorrent download missing"))?;
                        let mut r = Row::new(
                            format!("{title} ({quality})"),
                            format!("{base}{path}"),
                            Some(desc),
                        );
                        r.date = cap(r"(\d{2}/\d{2}/\d{4})", &response)
                            .as_deref()
                            .and_then(|s| date(s, "%d/%m/%Y"));
                        rows.push(r);
                    } else {
                        for tr in doc.select(&sel("tbody tr")) {
                            let td: Vec<_> = tr.select(&sel("td")).collect();
                            if td.len() < 4 {
                                continue;
                            }
                            let Some(a) = td[3].select(&sel("a[href]")).next() else {
                                continue;
                            };
                            let key = field(td[3], "p");
                            let mut r = Row::new(
                                format!(
                                    "{title} ({}){}",
                                    raw(td[1]).trim(),
                                    if key.trim() == "Sin clave" {
                                        String::new()
                                    } else {
                                        format!(", password: {}", key.trim())
                                    }
                                ),
                                format!("{base}{}", href(a)),
                                Some(desc.clone()),
                            );
                            r.date = date(&raw(td[2]), "%Y-%m-%d");
                            rows.push(r);
                        }
                    }
                }
            }
            "elitetorrent" => {
                let plus = q.replace("%20", "+");
                let response = self.fetch(&format!("{base}/?s={plus}"), None).await?;
                let pages = if response.contains("paginacion") {
                    links(&response, "a.pagina[href]")
                        .last()
                        .and_then(|h| cap(r"page/(\d+)/", h))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                } else if response.contains("Resultado de buscar") {
                    1
                } else {
                    0
                };
                let mut seen = HashSet::new();
                let mut found = Vec::new();
                for page in 1..=pages.min(2) {
                    let response = self
                        .fetch(&format!("{base}/page/{page}/?s={plus}"), None)
                        .await?;
                    for h in links(&response, "a[href]") {
                        let matching = match request.category.as_str() {
                            "movies" => h.contains("/peliculas/"),
                            "tv" => h.contains("/series/"),
                            _ => h.contains("/peliculas/") || h.contains("/series/"),
                        };
                        if matching && h.starts_with(base) && seen.insert(h.clone()) {
                            found.push(h);
                        }
                    }
                }
                for desc in found {
                    let response = self.fetch(&desc, None).await?.replace('\n', "");
                    let Some(title) = cap(r"(<h1>Descargar .+ por torrent</h1>)", &response) else {
                        continue;
                    };
                    let encoded: Vec<_> = super::static_regex(r#"i=([-A-Za-z0-9+/]+={0,3})""#)
                        .captures_iter(&response)
                        .map(|c| c[1].to_owned())
                        .collect();
                    let Some(uri) = encoded.get(1).and_then(|s| deobfuscate(s)) else {
                        continue;
                    };
                    let title = title
                        .trim_start_matches(|c| "<h1>Descargar".contains(c))
                        .trim_end_matches(|c| "por torrent</h1>".contains(c))
                        .trim();
                    let language = cap(r"Idioma:</b>([a-zA-Zñ\ ]+)", &response);
                    let quality = cap(r"Calidad:</b> ([0-9.a-z\-]+)", &response);
                    let d = cap(r"Fecha:</b>([ 0-9\-]+)", &response).map(|s| s.replace(' ', ""));
                    let mut r = Row::new(
                        format!(
                            "{title}{}{}({})",
                            language
                                .as_ref()
                                .map(|s| format!(" [{}]", s.trim()))
                                .unwrap_or_default(),
                            quality
                                .as_ref()
                                .map(|s| format!(" {} ", s.trim()))
                                .unwrap_or_default(),
                            d.as_deref().unwrap_or("-1")
                        ),
                        uri,
                        Some(desc),
                    );
                    r.size = cap(r"Tama.?o:</b> ([0-9.]+[ GM]+B)", &response)
                        .as_deref()
                        .and_then(size)
                        .or(Some(0));
                    r.seeds = cap(r"<b>Semillas</b>:([ 0-9]*)", &response)
                        .as_deref()
                        .and_then(num);
                    r.leech = cap(r"<b>Clientes</b>:([ 0-9]*)", &response)
                        .as_deref()
                        .and_then(num);
                    r.date = d.as_deref().and_then(|s| {
                        date(
                            s,
                            if s.split('-').next()?.parse::<u32>().ok()? > 1000 {
                                "%Y-%m-%d"
                            } else {
                                "%d-%m-%Y"
                            },
                        )
                    });
                    rows.push(r);
                }
            }
            "maxitorrent" => {
                let mut seen = HashSet::new();
                for page in (1..).step_by(2) {
                    let response = self
                        .fetch(
                            &format!("{base}/get/result/"),
                            Some(&format!("s={}&pg={page}", json::form_value(&q))),
                        )
                        .await?;
                    let data: serde_json::Value = serde_json::from_str(&response)?;
                    let groups = data["data"]["torrents"]
                        .as_object()
                        .ok_or_else(|| runtime("MaxiTorrent response missing torrents"))?;
                    let mut stop = false;
                    let mut entries = Vec::new();
                    for value in groups.values() {
                        if value.is_null() {
                            stop = true;
                            break;
                        }
                        if let Some(items) = value.as_object() {
                            for item in items.values() {
                                if let Some(guid) = item["guid"].as_str() {
                                    entries.push((
                                        format!("{base}/{guid}"),
                                        item["torrentSize"]
                                            .as_str()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| item["torrentSize"].to_string()),
                                    ));
                                }
                            }
                        }
                    }
                    for (start, bytes) in entries {
                        let mut queue = VecDeque::from([start]);
                        while let Some(link) = queue.pop_front() {
                            if !seen.insert(link.clone()) {
                                continue;
                            }
                            if seen.len() > 1000 {
                                return Err(runtime("MaxiTorrent detail traversal limit exceeded"));
                            }
                            let response = self.fetch(&link, None).await?;
                            if let Some(path) =
                                cap(r#"window.location.href = "([^\"]+)""#, &response)
                            {
                                let path = if path.starts_with("//") {
                                    path.trim_start_matches('/')
                                        .split_once('/')
                                        .map(|(_, s)| format!("/{s}"))
                                        .unwrap_or_default()
                                } else {
                                    path
                                };
                                let uri = join(&format!("{base}/"), &path)?;
                                // This plugin emits the script's destination as
                                // an HTTP reference even when it has no extension.
                                {
                                    let name = uri
                                        .rsplit('/')
                                        .next()
                                        .unwrap_or("")
                                        .split('.')
                                        .next()
                                        .unwrap_or("")
                                        .to_owned();
                                    let mut r = Row::new(name, uri.clone(), Some(uri));
                                    r.size = size(&bytes);
                                    rows.push(r);
                                }
                            } else {
                                for h in links(
                                    &response,
                                    "div[style='float:left;width:100%;height:auto;text-align:center;'] a[href], ul.buscar-list a[href], span.color a[href], span.color + a[href]",
                                ) {
                                    queue.push_back(join(&format!("{base}/"), &h)?);
                                }
                            }
                        }
                    }
                    if stop {
                        break;
                    }
                }
            }
            _ => return Err(runtime("Unknown legacy media source")),
        }
        for r in rows {
            results.push(candidate(
                &self.descriptor,
                r.title,
                r.uri,
                r.desc,
                r.size,
                r.seeds,
                r.leech,
                r.date,
                now(),
            )?);
            if results.len() >= limit {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
