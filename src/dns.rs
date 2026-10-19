//! Name resolution for source sites: the system resolver first, then DNS over
//! HTTPS when the system resolver fails.
//!
//! Some networks answer "no such host" for torrent sites that exist. When the
//! system lookup fails, the name is asked of public DNS-over-HTTPS resolvers,
//! addressed by IP so they need no lookup of their own. Only names the system
//! could not resolve leave the machine this way. A system answer, even a
//! wrong one, is used as given. `TORRENTX_ENCRYPTED_DNS=off` disables the
//! fallback.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Deserialize;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Queried in order; `{name}` is replaced by the host name.
const PROVIDERS: &[&str] = &[
    "https://1.1.1.1/dns-query?name={name}&type=A",
    "https://8.8.8.8/resolve?name={name}&type=A",
];

type Failure = Box<dyn std::error::Error + Send + Sync>;

/// Cheap to clone; clones share one cache.
#[derive(Clone)]
pub struct FallbackResolver(Arc<Inner>);

struct Inner {
    providers: Vec<String>,
    client: reqwest::Client,
    enabled: bool,
    cache: Mutex<HashMap<String, (Vec<IpAddr>, Instant)>>,
}

/// The resolver every source client shares, so answers are cached once.
pub fn resolver() -> Arc<FallbackResolver> {
    static SHARED: OnceLock<FallbackResolver> = OnceLock::new();
    Arc::new(
        SHARED
            .get_or_init(|| {
                let enabled = !std::env::var("TORRENTX_ENCRYPTED_DNS")
                    .is_ok_and(|v| v.eq_ignore_ascii_case("off"));
                FallbackResolver::new(PROVIDERS.iter().map(|p| p.to_string()).collect(), enabled)
            })
            .clone(),
    )
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "Answer", default)]
    answer: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: u16,
    #[serde(rename = "TTL", default)]
    ttl: u64,
    data: String,
}

/// A records from a DNS JSON answer, and how long to keep them.
fn addresses(answer: &Answer) -> (Vec<IpAddr>, Duration) {
    let records: Vec<&Record> = answer.answer.iter().filter(|r| r.kind == 1).collect();
    let addresses = records.iter().filter_map(|r| r.data.parse().ok()).collect();
    let ttl = records
        .iter()
        .map(|r| r.ttl)
        .min()
        .unwrap_or(0)
        .clamp(60, 3600);
    (addresses, Duration::from_secs(ttl))
}

impl FallbackResolver {
    pub fn new(providers: Vec<String>, enabled: bool) -> Self {
        Self(Arc::new(Inner {
            providers,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(6))
                .build()
                .unwrap_or_default(),
            enabled,
            cache: Mutex::new(HashMap::new()),
        }))
    }
}

impl Inner {
    async fn encrypted(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        if let Some((addresses, expires)) = self.cache.lock().unwrap().get(host)
            && *expires > Instant::now()
        {
            return Ok(addresses.clone());
        }
        let mut errors = Vec::new();
        for provider in &self.providers {
            let url = provider.replace("{name}", host);
            let reply = self
                .client
                .get(&url)
                .header("accept", "application/dns-json")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            let answer = match reply {
                Ok(reply) => reply
                    .bytes()
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|body| {
                        serde_json::from_slice::<Answer>(&body).map_err(|e| e.to_string())
                    }),
                Err(error) => Err(error.to_string()),
            };
            match answer {
                Ok(answer) => {
                    let (addresses, ttl) = addresses(&answer);
                    if addresses.is_empty() {
                        errors.push("no address".to_owned());
                        continue;
                    }
                    self.cache
                        .lock()
                        .unwrap()
                        .insert(host.to_owned(), (addresses.clone(), Instant::now() + ttl));
                    return Ok(addresses);
                }
                Err(error) => errors.push(error),
            }
        }
        Err(errors.join("; "))
    }
}

impl Resolve for FallbackResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let system = match tokio::net::lookup_host((host.as_str(), 0)).await {
                Ok(found) => {
                    let found: Vec<SocketAddr> = found.collect();
                    if !found.is_empty() {
                        return Ok(Box::new(found.into_iter()) as Addrs);
                    }
                    "no address".to_owned()
                }
                Err(error) => error.to_string(),
            };
            if !resolver.enabled {
                return Err(Failure::from(system));
            }
            match resolver.encrypted(&host).await {
                Ok(found) => Ok(Box::new(
                    found
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip, 0))
                        .collect::<Vec<_>>()
                        .into_iter(),
                ) as Addrs),
                Err(encrypted) => Err(Failure::from(format!(
                    "{system}; encrypted DNS also failed: {encrypted}"
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Answer every HTTP request on a local port with `body`; returns the port.
    async fn serve(body: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut request = [0; 2048];
                    let _ = stream.read(&mut request).await;
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        port
    }

    #[test]
    fn reads_a_records_and_bounds_the_ttl() {
        let answer: Answer = serde_json::from_str(
            r#"{"Status":0,"Answer":[{"name":"x.","type":5,"TTL":30,"data":"y."},
                {"name":"y.","type":1,"TTL":20,"data":"192.0.2.7"}]}"#,
        )
        .unwrap();
        let (found, ttl) = addresses(&answer);
        assert_eq!(found, vec!["192.0.2.7".parse::<IpAddr>().unwrap()]);
        assert_eq!(ttl, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn a_name_the_system_cannot_resolve_is_reached_through_the_fallback() {
        let site = serve("site reached").await;
        let dns = serve(r#"{"Answer":[{"type":1,"TTL":300,"data":"127.0.0.1"}]}"#).await;
        let broken = "http://127.0.0.1:1/dns-query?name={name}".to_string();
        let working = format!("http://127.0.0.1:{dns}/dns-query?name={{name}}");
        let resolver = Arc::new(FallbackResolver::new(vec![broken, working], true));
        let client = reqwest::Client::builder()
            .dns_resolver(resolver.clone())
            .build()
            .unwrap();
        // .invalid never resolves (RFC 6761).
        let body = client
            .get(format!("http://torrentx-test.invalid:{site}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "site reached");

        let off = Arc::new(FallbackResolver::new(
            vec![format!("http://127.0.0.1:{dns}/dns-query?name={{name}}")],
            false,
        ));
        let client = reqwest::Client::builder()
            .dns_resolver(off)
            .build()
            .unwrap();
        let error = client
            .get(format!("http://torrentx-test.invalid:{site}/"))
            .send()
            .await
            .unwrap_err();
        assert!(error.is_connect(), "{error}");
    }
}
