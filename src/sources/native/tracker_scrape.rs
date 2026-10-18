//! Bounded BEP 15 UDP and BEP 48 HTTP scrapes used by the FitGirl source.
use super::{NativeSource, runtime};
use crate::SearchError;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
const WAIT: Duration = Duration::from_millis(1200);
enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    List,
    Dict(BTreeMap<Vec<u8>, Value>),
}
fn bdecode(data: &[u8], position: &mut usize, depth: u8) -> Option<Value> {
    if depth > 16 {
        return None;
    }
    let first = *data.get(*position)?;
    if first == b'i' {
        *position += 1;
        let end = data[*position..].iter().position(|b| *b == b'e')? + *position;
        let value = std::str::from_utf8(&data[*position..end])
            .ok()?
            .parse()
            .ok()?;
        *position = end + 1;
        return Some(Value::Int(value));
    }
    if first == b'd' || first == b'l' {
        *position += 1;
        let mut values = BTreeMap::new();
        while *data.get(*position)? != b'e' {
            let key = bdecode(data, position, depth + 1)?;
            if first == b'd' {
                let Value::Bytes(key) = key else { return None };
                values.insert(key, bdecode(data, position, depth + 1)?);
            }
        }
        *position += 1;
        return Some(if first == b'd' {
            Value::Dict(values)
        } else {
            Value::List
        });
    }
    let end = data[*position..].iter().position(|b| *b == b':')? + *position;
    let length = std::str::from_utf8(&data[*position..end])
        .ok()?
        .parse::<usize>()
        .ok()?;
    *position = end + 1;
    let last = position.checked_add(length)?;
    let value = data.get(*position..last)?.to_vec();
    *position = last;
    Some(Value::Bytes(value))
}
fn transaction() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        ^ std::process::id().rotate_left(13)
}
fn int(map: &BTreeMap<Vec<u8>, Value>, keys: &[&[u8]]) -> Option<u64> {
    keys.iter().find_map(|k| match map.get(*k) {
        Some(Value::Int(n)) => (*n).try_into().ok(),
        _ => None,
    })
}

impl NativeSource {
    pub(super) async fn scrape_trackers(
        &self,
        uris: &[String],
        hashes: &[String],
    ) -> BTreeMap<String, (u64, u64)> {
        let mut frequency: Vec<(String, usize)> = Vec::new();
        for uri in uris {
            if let Ok(uri) = url::Url::parse(uri) {
                for (k, v) in uri
                    .query_pairs()
                    .filter(|(k, _)| k.eq_ignore_ascii_case("tr"))
                {
                    let _ = k;
                    if let Some((_, count)) = frequency.iter_mut().find(|(t, _)| t == &v) {
                        *count += 1
                    } else {
                        frequency.push((v.into_owned(), 1));
                    }
                }
            }
        }
        frequency.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let mut ordered = frequency.into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        ordered.extend(super::fitgirl::TRACKERS.iter().map(|s| s.to_string()));
        let mut seen = std::collections::HashSet::new();
        let mut tasks = tokio::task::JoinSet::new();
        for tracker in ordered
            .into_iter()
            .filter(|s| seen.insert(s.to_lowercase()))
            .filter(|s| {
                url::Url::parse(s).is_ok_and(|u| matches!(u.scheme(), "udp" | "https" | "http"))
            })
            .take(4)
        {
            let client = self.client.clone();
            let hashes = hashes.to_vec();
            tasks.spawn(async move {
                tokio::time::timeout(
                    WAIT * 2 + Duration::from_millis(500),
                    scrape(client, &tracker, &hashes),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
            });
        }
        let mut best: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        while let Some(Ok(stats)) = tasks.join_next().await {
            for (hash, value) in stats {
                best.entry(hash)
                    .and_modify(|current| *current = (*current).max(value))
                    .or_insert(value);
            }
        }
        best
    }
}
async fn scrape(
    client: reqwest::Client,
    tracker: &str,
    hashes: &[String],
) -> Result<BTreeMap<String, (u64, u64)>, SearchError> {
    let mut url = url::Url::parse(tracker).map_err(|_| runtime("Invalid tracker URL"))?;
    let mut stats = BTreeMap::new();
    let decoded: Vec<_> = hashes
        .iter()
        .filter_map(|s| data_encoding::HEXLOWER.decode(s.as_bytes()).ok())
        .collect();
    if decoded.len() != hashes.len() {
        return Err(runtime("Invalid scrape infohash"));
    }
    if url.scheme() == "udp" {
        let addresses = tokio::net::lookup_host((
            url.host_str()
                .ok_or_else(|| runtime("Tracker host missing"))?,
            url.port().unwrap_or(6969),
        ))
        .await?;
        let address = addresses
            .min_by_key(|a| !a.is_ipv4())
            .ok_or_else(|| runtime("No tracker address"))?;
        let socket = tokio::net::UdpSocket::bind(if address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await?;
        socket.connect(address).await?;
        let transaction = transaction();
        let mut request = 0x41727101980u64.to_be_bytes().to_vec();
        request.extend(0u32.to_be_bytes());
        request.extend(transaction.to_be_bytes());
        socket.send(&request).await?;
        let mut buffer = [0u8; 2048];
        let length = tokio::time::timeout(WAIT, socket.recv(&mut buffer))
            .await
            .map_err(|_| runtime("Tracker connect timeout"))??;
        if length < 16
            || buffer[..4] != 0u32.to_be_bytes()
            || buffer[4..8] != transaction.to_be_bytes()
        {
            return Err(runtime("Invalid tracker connect response"));
        }
        let connection = buffer[8..16].to_vec();
        for (chunk_index, chunk) in decoded.chunks(60).enumerate() {
            let transaction = transaction.wrapping_add(chunk_index as u32 + 1);
            let mut request = connection.clone();
            request.extend(2u32.to_be_bytes());
            request.extend(transaction.to_be_bytes());
            for hash in chunk {
                request.extend(hash);
            }
            socket.send(&request).await?;
            let length = tokio::time::timeout(WAIT, socket.recv(&mut buffer))
                .await
                .map_err(|_| runtime("Tracker scrape timeout"))??;
            if length < 8
                || buffer[..4] != 2u32.to_be_bytes()
                || buffer[4..8] != transaction.to_be_bytes()
            {
                return Err(runtime("Invalid tracker scrape response"));
            }
            for (i, entry) in buffer[8..length]
                .chunks_exact(12)
                .take(chunk.len())
                .enumerate()
            {
                let seeds = u32::from_be_bytes(entry[..4].try_into().unwrap());
                let leech = u32::from_be_bytes(entry[8..12].try_into().unwrap());
                stats.insert(
                    hashes[chunk_index * 60 + i].clone(),
                    (seeds.into(), leech.into()),
                );
            }
        }
    } else {
        let path = url.path();
        let Some(index) = path.rfind("announce") else {
            return Ok(stats);
        };
        url.set_path(&format!("{}scrape{}", &path[..index], &path[index + 8..]));
        let mut query = url
            .query_pairs()
            .filter(|(k, _)| k != "info_hash")
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    super::json::form_value(&k),
                    super::json::form_value(&v)
                )
            })
            .collect::<Vec<_>>();
        for hash in &decoded {
            query.push(format!(
                "info_hash={}",
                hash.iter().map(|b| format!("%{b:02X}")).collect::<String>()
            ));
        }
        url.set_query(Some(&query.join("&")));
        let mut response = client
            .get(url)
            .timeout(WAIT)
            .send()
            .await
            .map_err(super::http_error)?
            .error_for_status()
            .map_err(super::http_error)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(super::http_error)? {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err(runtime("Tracker response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let Some(Value::Dict(root)) = bdecode(&bytes, &mut 0, 0) else {
            return Ok(stats);
        };
        let Some(Value::Dict(files)) = root.get(b"files".as_slice()) else {
            return Ok(stats);
        };
        for (hash, key) in hashes.iter().zip(decoded) {
            if let Some(Value::Dict(value)) = files.get(&key)
                && let (Some(seeds), Some(leech)) = (
                    int(value, &[b"complete", b"seeds", b"seeders"]),
                    int(value, &[b"incomplete", b"peers", b"leechers"]),
                ) {
                    stats.insert(hash.clone(), (seeds, leech));
                }
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_and_deep_tracker_data() {
        assert!(bdecode(b"d5:files", &mut 0, 0).is_none());
        assert!(bdecode(&[b'l'; 20], &mut 0, 0).is_none());
        assert!(matches!(
            bdecode(b"d8:completei12e10:incompletei3ee", &mut 0, 0),
            Some(Value::Dict(_))
        ));
    }

    #[tokio::test]
    async fn validates_udp_transactions_and_reads_batched_scrape_counts() {
        let server = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let responder = tokio::spawn(async move {
            let mut buffer = [0u8; 2048];
            let (length, peer) = server.recv_from(&mut buffer).await.unwrap();
            assert_eq!(length, 16);
            assert_eq!(&buffer[..8], &0x41727101980u64.to_be_bytes());
            let mut answer = 0u32.to_be_bytes().to_vec();
            answer.extend(&buffer[12..16]);
            answer.extend(42u64.to_be_bytes());
            server.send_to(&answer, peer).await.unwrap();
            let (length, peer) = server.recv_from(&mut buffer).await.unwrap();
            assert_eq!(length, 36);
            assert_eq!(&buffer[8..12], &2u32.to_be_bytes());
            let mut answer = 2u32.to_be_bytes().to_vec();
            answer.extend(&buffer[12..16]);
            answer.extend(12u32.to_be_bytes());
            answer.extend(99u32.to_be_bytes());
            answer.extend(3u32.to_be_bytes());
            server.send_to(&answer, peer).await.unwrap();
        });
        let hash = "1234567890abcdef1234567890abcdef12345678".to_owned();
        let stats = scrape(
            reqwest::Client::new(),
            &format!("udp://{address}/announce"),
            std::slice::from_ref(&hash),
        )
        .await
        .unwrap();
        assert_eq!(stats.get(&hash), Some(&(12, 3)));
        responder.await.unwrap();
    }

    #[tokio::test]
    async fn http_scrape_preserves_binary_hash_and_tracker_query() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let hash = "1234567890abcdef1234567890abcdef12345678".to_owned();
        let key = data_encoding::HEXLOWER.decode(hash.as_bytes()).unwrap();
        let responder = tokio::spawn(async move {
            let (mut stream, _) = server.accept().await.unwrap();
            let mut buffer = [0u8; 4096];
            let length = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..length]);
            assert!(
                request.starts_with("GET /scrape?token=fixture&info_hash=%12%34%56%78%90%AB%CD%EF"),
                "{request}"
            );
            let mut body = b"d5:filesd20:".to_vec();
            body.extend(key);
            body.extend(b"d8:completei12e10:incompletei3eeee");
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(&body).await.unwrap();
        });
        let stats = scrape(
            reqwest::Client::new(),
            &format!("http://{address}/announce?token=fixture"),
            std::slice::from_ref(&hash),
        )
        .await
        .unwrap();
        assert_eq!(stats.get(&hash), Some(&(12, 3)));
        responder.await.unwrap();
    }
}
