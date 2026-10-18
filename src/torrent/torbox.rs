//! TorBox as a faster data path for inspection. A torrent TorBox already holds
//! is read over HTTPS from its servers instead of from peers; anything else
//! falls back to the wrapped service (rqbit), with the reason in the report.
//!
//! Metadata always comes from the wrapped service, so file ids and catalogs
//! are the same whichever path serves the bytes. TorBox files are matched to
//! catalog files by path and size, never by TorBox's own numbering.
use super::*;
use reqwest::Client;
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex, time::Duration};

const API: &str = "https://api.torbox.app/v1/api";
const CALL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a cached torrent may take to show its files after being added.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// A TorBox API key. Never printed: Debug shows only that one is set.
#[derive(Clone, PartialEq, Eq)]
pub struct TorboxKey(String);

impl TorboxKey {
    pub fn new(key: impl Into<String>) -> Option<Self> {
        let key = key.into().trim().to_owned();
        (!key.is_empty()).then_some(Self(key))
    }
}

impl std::fmt::Debug for TorboxKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TorboxKey(..)")
    }
}

pub struct TorboxClient {
    http: Client,
    base: String,
    key: TorboxKey,
    /// Answers already received in this operation, by infohash.
    cached: Mutex<HashMap<String, bool>>,
}

/// One file as TorBox lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub id: u64,
    pub name: String,
    pub size: u64,
}

fn failure(message: impl Into<String>) -> TorrentFailure {
    TorrentFailure::new("torbox_unavailable", message)
}

impl TorboxClient {
    pub fn new(key: TorboxKey) -> Result<Self, TorrentFailure> {
        Self::with_base(key, API)
    }

    /// For tests against a local fake of the API.
    pub fn with_base(key: TorboxKey, base: &str) -> Result<Self, TorrentFailure> {
        let http = Client::builder()
            .timeout(CALL_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| failure(e.to_string()))?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            key,
            cached: Mutex::new(HashMap::new()),
        })
    }

    /// The `data` of a successful API answer.
    async fn data(&self, request: reqwest::RequestBuilder) -> Result<Value, TorrentFailure> {
        let response = request
            .bearer_auth(&self.key.0)
            .send()
            .await
            // The error text can carry the request URL, which may hold the key.
            .map_err(|e| failure(format!("TorBox request failed: {}", e.without_url())))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|_| failure(format!("TorBox answered {status} without JSON")))?;
        if !status.is_success() || body["success"] != Value::Bool(true) {
            let detail = body["detail"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or("no detail");
            let code = if status == 401 || status == 403 {
                "torbox_unauthorized"
            } else {
                "torbox_unavailable"
            };
            return Err(TorrentFailure::new(
                code,
                format!("TorBox answered {status}: {detail}"),
            ));
        }
        Ok(body["data"].clone())
    }

    /// Whether TorBox can serve this torrent at once.
    pub async fn cached(&self, hash: &str) -> Result<bool, TorrentFailure> {
        if let Some(known) = self.cached.lock().unwrap().get(hash) {
            return Ok(*known);
        }
        let data = self
            .data(
                self.http
                    .get(format!("{}/torrents/checkcached", self.base))
                    .query(&[
                        ("hash", hash),
                        ("format", "object"),
                        ("list_files", "false"),
                    ]),
            )
            .await?;
        let cached = data
            .as_object()
            .is_some_and(|entries| entries.keys().any(|key| key.eq_ignore_ascii_case(hash)));
        self.cached.lock().unwrap().insert(hash.to_owned(), cached);
        Ok(cached)
    }

    /// Add a cached torrent to the account; Ok(None) if it is already there.
    async fn add(&self, hash: &str) -> Result<Option<u64>, TorrentFailure> {
        let form = reqwest::multipart::Form::new()
            .text("magnet", format!("magnet:?xt=urn:btih:{hash}"))
            .text("seed", "3")
            .text("allow_zip", "false")
            .text("add_only_if_cached", "true");
        match self
            .data(
                self.http
                    .post(format!("{}/torrents/createtorrent", self.base))
                    .multipart(form),
            )
            .await
        {
            Ok(data) => Ok(data["torrent_id"].as_u64()),
            Err(error) if error.code == "torbox_unauthorized" => Err(error),
            // Already in the account is reported as an error; look it up instead.
            Err(_) => Ok(None),
        }
    }

    /// The account's torrent with this infohash.
    async fn find(&self, hash: &str) -> Result<Option<Value>, TorrentFailure> {
        let data = self
            .data(
                self.http
                    .get(format!("{}/torrents/mylist", self.base))
                    .query(&[("bypass_cache", "true")]),
            )
            .await?;
        Ok(data.as_array().and_then(|torrents| {
            torrents
                .iter()
                .find(|t| {
                    t["hash"]
                        .as_str()
                        .is_some_and(|h| h.eq_ignore_ascii_case(hash))
                })
                .cloned()
        }))
    }

    async fn torrent(&self, id: u64) -> Result<Value, TorrentFailure> {
        self.data(
            self.http
                .get(format!("{}/torrents/mylist", self.base))
                .query(&[("id", id.to_string().as_str()), ("bypass_cache", "true")]),
        )
        .await
    }

    /// Make a cached torrent available in the account and list its files.
    /// Returns its id and whether this call added it.
    pub async fn prepare(
        &self,
        hash: &str,
    ) -> Result<(u64, bool, Vec<RemoteFile>), TorrentFailure> {
        let (id, added) = match self.add(hash).await? {
            Some(id) => (id, true),
            None => {
                let existing = self.find(hash).await?.ok_or_else(|| {
                    failure("TorBox neither added the torrent nor lists it in the account")
                })?;
                let id = existing["id"]
                    .as_u64()
                    .ok_or_else(|| failure("TorBox torrent without id"))?;
                (id, false)
            }
        };
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            let torrent = self.torrent(id).await?;
            let ready = torrent["download_present"] == Value::Bool(true)
                || torrent["download_finished"] == Value::Bool(true);
            if ready {
                return Ok((id, added, remote_files(&torrent)));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(failure(
                    "TorBox did not make the cached torrent ready in 30 s",
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// A direct HTTPS link to one file.
    pub async fn link(&self, torrent_id: u64, file_id: u64) -> Result<String, TorrentFailure> {
        let data = self
            .data(
                self.http
                    .get(format!("{}/torrents/requestdl", self.base))
                    .query(&[
                        ("token", self.key.0.as_str()),
                        ("torrent_id", torrent_id.to_string().as_str()),
                        ("file_id", file_id.to_string().as_str()),
                        ("zip_link", "false"),
                    ]),
            )
            .await?;
        data.as_str()
            .map(str::to_owned)
            .ok_or_else(|| failure("TorBox returned no download link"))
    }

    /// Whether the link serves exact byte ranges of a file this size.
    pub async fn serves_ranges(&self, url: &str, size: u64) -> bool {
        let Ok(response) = self.http.get(url).header("Range", "bytes=0-0").send().await else {
            return false;
        };
        response.status() == 206
            && response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                == Some(format!("bytes 0-0/{size}").as_str())
    }

    pub async fn delete(&self, torrent_id: u64) {
        let _ = self
            .data(
                self.http
                    .post(format!("{}/torrents/controltorrent", self.base))
                    .json(&serde_json::json!({"torrent_id": torrent_id, "operation": "delete"})),
            )
            .await;
    }
}

fn remote_files(torrent: &Value) -> Vec<RemoteFile> {
    torrent["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(RemoteFile {
                id: file["id"].as_u64()?,
                name: file["name"].as_str()?.replace('\\', "/"),
                size: file["size"].as_u64()?,
            })
        })
        .collect()
}

/// The TorBox file holding a catalog file: same size and the same path, with
/// or without the torrent's top folder; else the only file of that size.
pub fn match_file<'a>(files: &'a [RemoteFile], file: &TorrentFile) -> Option<&'a RemoteFile> {
    let sized: Vec<_> = files.iter().filter(|f| f.size == file.size_bytes).collect();
    sized
        .iter()
        .find(|f| f.name == file.path || f.name.ends_with(&format!("/{}", file.path)))
        .copied()
        .or_else(|| (sized.len() == 1).then(|| sized[0]))
}

/// Serves each opened file from TorBox when it can, else from `inner`.
pub struct TorboxFirst<'a> {
    inner: &'a dyn TorrentService,
    client: TorboxClient,
    /// Open TorBox-served files, by infohash and file id, with their count.
    remote: Mutex<HashMap<(String, usize), usize>>,
    /// Torrents this run added to the account, removed when their last file closes.
    added: Mutex<HashMap<String, u64>>,
    notes: Mutex<Vec<String>>,
}

impl<'a> TorboxFirst<'a> {
    pub fn new(inner: &'a dyn TorrentService, client: TorboxClient) -> Self {
        Self {
            inner,
            client,
            remote: Mutex::new(HashMap::new()),
            added: Mutex::new(HashMap::new()),
            notes: Mutex::new(Vec::new()),
        }
    }

    /// Remove the torrents this run added to the TorBox account.
    pub async fn release_added(&self) {
        let added: Vec<u64> = self
            .added
            .lock()
            .unwrap()
            .drain()
            .map(|(_, id)| id)
            .collect();
        for id in added {
            self.client.delete(id).await;
        }
    }

    fn note(&self, text: String) {
        let mut notes = self.notes.lock().unwrap();
        if notes.len() < 32 && !notes.contains(&text) {
            notes.push(text);
        }
    }

    async fn open_remote(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<Option<MediaInput>, TorrentFailure> {
        let hash = &torrent.info_hash_v1;
        if !self.client.cached(hash).await? {
            self.note(format!("{hash}: not cached on TorBox; read from peers"));
            return Ok(None);
        }
        let (id, added, files) = self.client.prepare(hash).await?;
        if added {
            self.added.lock().unwrap().insert(hash.clone(), id);
        }
        let Some(remote) = match_file(&files, file) else {
            self.note(format!(
                "{hash}: TorBox lists no file matching {}; read from peers",
                file.path
            ));
            return Ok(None);
        };
        let url = self.client.link(id, remote.id).await?;
        if !self.client.serves_ranges(&url, file.size_bytes).await {
            self.note(format!(
                "{hash}: TorBox link does not serve byte ranges; read from peers"
            ));
            return Ok(None);
        }
        Ok(Some(MediaInput::RemoteFile {
            url,
            size_bytes: file.size_bytes,
        }))
    }
}

#[async_trait]
impl TorrentService for TorboxFirst<'_> {
    fn description(&self) -> TorrentServiceRun {
        let mut run = self.inner.description();
        run.backend = format!("torbox+{}", run.backend);
        run.notes.extend(self.notes.lock().unwrap().iter().cloned());
        run
    }

    async fn metadata(
        &self,
        reference: &TorrentReference,
    ) -> Result<TorrentMetadata, TorrentFailure> {
        self.inner.metadata(reference).await
    }

    async fn open_file(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure> {
        match self.open_remote(torrent, file).await {
            Ok(Some(input)) => {
                *self
                    .remote
                    .lock()
                    .unwrap()
                    .entry((torrent.info_hash_v1.clone(), file.id))
                    .or_default() += 1;
                Ok(input)
            }
            Ok(None) => self.inner.open_file(torrent, file).await,
            Err(error) => {
                self.note(format!(
                    "{}: {} ({}); read from peers",
                    torrent.info_hash_v1, error.message, error.code
                ));
                self.inner.open_file(torrent, file).await
            }
        }
    }

    async fn close_file(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<(), TorrentFailure> {
        let key = (torrent.info_hash_v1.clone(), file.id);
        let last_remote = {
            let mut remote = self.remote.lock().unwrap();
            match remote.get_mut(&key) {
                None => None,
                Some(count) => {
                    *count -= 1;
                    if *count == 0 {
                        remote.remove(&key);
                    }
                    Some(!remote.keys().any(|(hash, _)| hash == &key.0))
                }
            }
        };
        match last_remote {
            None => self.inner.close_file(torrent, file).await,
            Some(false) => Ok(()),
            Some(true) => {
                let added = self.added.lock().unwrap().remove(&key.0);
                if let Some(id) = added {
                    self.client.delete(id).await;
                }
                Ok(())
            }
        }
    }

    async fn shutdown(&self) -> Result<(), TorrentFailure> {
        self.release_added().await;
        self.inner.shutdown().await
    }
}

/// Wrap `inner` with TorBox when the runtime has a key.
pub fn torbox_first<'a>(
    runtime: &TorrentRuntime,
    inner: &'a dyn TorrentService,
) -> Option<TorboxFirst<'a>> {
    let key = runtime.torbox.clone()?;
    TorboxClient::new(key)
        .ok()
        .map(|client| TorboxFirst::new(inner, client))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64) -> TorrentFile {
        TorrentFile {
            id: 0,
            path: path.into(),
            size_bytes: size,
            media_kind: "video".into(),
        }
    }

    #[test]
    fn files_match_by_path_and_size_not_torbox_numbering() {
        let files = vec![
            RemoteFile {
                id: 7,
                name: "Show S01/E01.mkv".into(),
                size: 100,
            },
            RemoteFile {
                id: 3,
                name: "Show S01/E02.mkv".into(),
                size: 100,
            },
            RemoteFile {
                id: 9,
                name: "Show S01/extra.nfo".into(),
                size: 5,
            },
        ];
        assert_eq!(match_file(&files, &file("E02.mkv", 100)).unwrap().id, 3);
        assert_eq!(
            match_file(&files, &file("Show S01/E01.mkv", 100))
                .unwrap()
                .id,
            7
        );
        assert!(
            match_file(&files, &file("E03.mkv", 100)).is_none(),
            "same size, other name, and not unique"
        );
        assert_eq!(match_file(&files, &file("renamed.nfo", 5)).unwrap().id, 9);
        assert!(match_file(&files, &file("E01.mkv", 101)).is_none());
    }

    #[test]
    fn the_key_never_appears_in_debug_output() {
        let key = TorboxKey::new(" secret-key ").unwrap();
        assert_eq!(format!("{key:?}"), "TorboxKey(..)");
        assert!(TorboxKey::new("  ").is_none());
    }
}
