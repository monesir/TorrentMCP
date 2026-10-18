use super::*;
use reqwest::{Client, Response};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashMap},
    fs::{File, OpenOptions},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::process::{Child, Command};

fn http_error(error: reqwest::Error) -> TorrentFailure {
    if error.is_timeout() {
        super::timeout()
    } else {
        TorrentFailure::new("torrent_service_failed", error.without_url().to_string())
    }
}

/// One rqbit process shared by every operation of a session.
///
/// - Metadata is resolved once per infohash and kept as .torrent bytes on disk,
///   so later sessions add the torrent instantly instead of asking the swarm again.
/// - A torrent is added once and stays managed while any of its files is open.
///   The last close deletes it and its partial data, unless parking is enabled
///   (`with_parking`): then it is paused with its pieces and resumed by the next
///   open, until a cap or the parking time evicts it.
/// - The session directory is removed on shutdown; a crashed session's directory
///   is swept by the next one (its lock file is no longer held).
pub struct RqbitService {
    child: tokio::sync::Mutex<Option<Child>>,
    stopped: AtomicBool,
    client: Client,
    base: String,
    directory: PathBuf,
    metadata_directory: PathBuf,
    lock: Mutex<Option<File>>,
    known: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    managed: tokio::sync::Mutex<HashMap<String, Managed>>,
    timeout: Duration,
    parking: Option<ParkingLimits>,
}

/// How many closed torrents a long-lived session keeps paused, and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParkingLimits {
    pub max_torrents: usize,
    /// Bytes rqbit reports as downloaded, summed over parked torrents.
    pub max_bytes: u64,
    pub ttl: Duration,
}

/// A torrent added to rqbit, with the files currently being read.
struct Managed {
    id: u64,
    open: HashMap<usize, usize>,
    /// Background reads that fetch a file's head and tail while it is open.
    warmers: HashMap<usize, Vec<tokio::task::AbortHandle>>,
    /// Set while paused with no open file.
    parked: Option<Parked>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Parked {
    since: Instant,
    bytes: u64,
}

/// Parked torrents to delete, oldest first: those past `ttl`, then the oldest
/// until both caps hold.
fn evictions(parked: &[(String, Parked)], limits: &ParkingLimits, now: Instant) -> Vec<String> {
    let mut ordered: Vec<_> = parked.iter().collect();
    ordered.sort_by_key(|(_, entry)| entry.since);
    let mut count = ordered.len();
    let mut bytes: u64 = ordered.iter().map(|(_, entry)| entry.bytes).sum();
    let mut evicted = Vec::new();
    for (hash, entry) in ordered {
        let expired = now.saturating_duration_since(entry.since) >= limits.ttl;
        if expired || count > limits.max_torrents || bytes > limits.max_bytes {
            evicted.push(hash.clone());
            count -= 1;
            bytes -= entry.bytes;
        }
    }
    evicted
}

/// Trackers given to every torrent in addition to its own. A torrent resolved
/// from a magnet carries only that magnet's trackers, often one or two.
const SESSION_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://explodie.org:6969/announce",
    "udp://tracker.dler.org:6969/announce",
    "udp://tracker.qu.ax:6969/announce",
    "udp://tracker.theoks.net:6969/announce",
    "udp://opentracker.io:6969/announce",
    "udp://tracker.tiny-vps.com:6969/announce",
    "udp://retracker.lanta-net.ru:2710/announce",
    "http://tracker.opentrackr.org:1337/announce",
];

/// Bytes read ahead from each end of an opened file. Containers keep their
/// headers at the start and often their index at the end (MP4 moov, Matroska
/// cues and statistics tags); fetching both at once spares ffprobe one
/// sequential wait for a cold piece per seek.
const WARM_HEAD_BYTES: u64 = 1024 * 1024;
const WARM_TAIL_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
struct AddResponse {
    id: Option<u64>,
    details: Details,
    output_folder: String,
}
#[derive(Deserialize)]
struct Details {
    info_hash: String,
    name: Option<String>,
    files: Option<Vec<ServiceFile>>,
}
#[derive(Deserialize)]
struct ServiceFile {
    components: Vec<String>,
    length: u64,
    #[serde(default)]
    attributes: serde_json::Value,
}
#[derive(Deserialize)]
struct Stats {
    state: String,
    error: Option<String>,
    #[serde(default)]
    progress_bytes: u64,
}

const METADATA_CACHE_ENTRIES: usize = 2000;
const MAX_TORRENT_BYTES: usize = 16 * 1024 * 1024;

impl RqbitService {
    pub async fn start(
        runtime: &TorrentRuntime,
        limits: &TorrentServiceLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, TorrentFailure> {
        limits.validate()?;
        let executable = std::fs::canonicalize(&runtime.rqbit).map_err(|e| {
            TorrentFailure::new(
                "torrent_service_unavailable",
                format!("Cannot open rqbit executable: {e}"),
            )
        })?;
        std::fs::create_dir_all(&runtime.cache_directory)?;
        let cache = std::fs::canonicalize(&runtime.cache_directory)?;
        sweep_cache(&cache);
        let metadata_directory = cache.join("metadata");
        std::fs::create_dir_all(&metadata_directory)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = cache
            .join("sessions")
            .join(format!("{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(directory.join("torrents"))?;
        let lock = hold_lock(&directory.join("session.lock"))?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("rqbit.log"))?;
        let timeout = Duration::from_secs(limits.timeout_seconds);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .map_err(|e| TorrentFailure::new("torrent_service_failed", e.to_string()))?;
        let mut command = Command::new(executable);
        command
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        // User configuration must not turn this private inspection process into a LAN service.
        for (key, _) in
            std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("RQBIT_"))
        {
            command.env_remove(key);
        }
        command.args([
            "-v",
            "error",
            "--http-api-listen-addr",
            &address.to_string(),
            "--disable-dht-persistence",
            "--disable-upnp-port-forward",
            "--disable-lsd",
            "--disable-tcp-listen",
            "--ratelimit-upload",
            "1024",
            "--peer-limit",
            "60",
            "--trackers-filename",
        ]);
        let trackers = directory.join("trackers.txt");
        std::fs::write(&trackers, SESSION_TRACKERS.join("\n"))?;
        command.arg(&trackers);
        if let Some(limit) = runtime.download_limit_bytes_per_second {
            command.args(["--ratelimit-download", &limit.to_string()]);
        }
        command
            .args(["server", "start", "--disable-persistence"])
            .arg(directory.join("torrents"));
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .spawn()
            .map_err(|e| TorrentFailure::new("torrent_service_unavailable", e.to_string()))?;
        let service = Self {
            child: tokio::sync::Mutex::new(Some(child)),
            stopped: AtomicBool::new(false),
            client,
            base: format!("http://{address}"),
            directory,
            metadata_directory,
            lock: Mutex::new(Some(lock)),
            known: Mutex::new(HashMap::new()),
            managed: tokio::sync::Mutex::new(HashMap::new()),
            timeout,
            parking: None,
        };
        let ready = bounded(limits.timeout_seconds.min(10), cancel, async {
            loop {
                if let Some(child) = service.child.lock().await.as_mut()
                    && let Some(status) = child.try_wait()?
                {
                    return Err(TorrentFailure::new(
                        "torrent_service_failed",
                        format!("rqbit exited during startup: {status}; see rqbit.log"),
                    ));
                }
                if service
                    .client
                    .get(&service.base)
                    .timeout(Duration::from_millis(300))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        if let Err(error) = ready {
            let _ = service.shutdown().await;
            return Err(error);
        }
        Ok(service)
    }

    async fn response_bytes(response: Response, limit: usize) -> Result<Vec<u8>, TorrentFailure> {
        if !response.status().is_success() {
            return Err(TorrentFailure::new(
                "torrent_service_failed",
                format!(
                    "HTTP {} from torrent service or metadata source",
                    response.status().as_u16()
                ),
            ));
        }
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(TorrentFailure::new(
                "torrent_metadata_too_large",
                "Torrent metadata response exceeds limit",
            ));
        }
        let mut response = response;
        let mut data = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(http_error)? {
            if data.len() + chunk.len() > limit {
                return Err(TorrentFailure::new(
                    "torrent_metadata_too_large",
                    "Torrent metadata response exceeds limit",
                ));
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }

    async fn add(
        &self,
        query: &[(&str, String)],
        torrent: &[u8],
    ) -> Result<AddResponse, TorrentFailure> {
        let response = self
            .client
            .post(format!("{}/torrents", self.base))
            .query(query)
            .header("Content-Type", "application/x-bittorrent")
            .body(torrent.to_vec())
            .send()
            .await
            .map_err(http_error)?;
        serde_json::from_slice(&Self::response_bytes(response, 8 * 1024 * 1024).await?)
            .map_err(|e| TorrentFailure::new("torrent_metadata_invalid", e.to_string()))
    }

    async fn post_json(&self, path: &str, body: String) -> Result<(), TorrentFailure> {
        let response = self
            .client
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(http_error)?;
        Self::response_bytes(response, 64 * 1024).await.map(|_| ())
    }

    fn metadata_from(details: Details) -> Result<TorrentMetadata, TorrentFailure> {
        let files = details.files.ok_or_else(|| {
            TorrentFailure::new(
                "torrent_metadata_invalid",
                "Torrent service returned no file list",
            )
        })?;
        if files.len() > 10000 {
            return Err(TorrentFailure::new(
                "torrent_metadata_invalid",
                "Too many torrent files",
            ));
        }
        let mut result = Vec::with_capacity(files.len());
        for (id, file) in files.into_iter().enumerate() {
            let path = file.components.join("/");
            if file.components.iter().any(|p| p.contains('/'))
                || !safe_components(&path)
                || file
                    .attributes
                    .get("symlink")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                return Err(TorrentFailure::new(
                    "torrent_metadata_invalid",
                    "Unsafe torrent file path or symlink",
                ));
            }
            result.push(TorrentFile {
                id,
                size_bytes: file.length,
                media_kind: media_kind(&path).into(),
                path,
            });
        }
        Ok(TorrentMetadata {
            info_hash_v1: details.info_hash.to_ascii_lowercase(),
            name: details.name,
            files: result,
        })
    }

    fn cached_torrent(&self, hash: &str) -> Option<Arc<Vec<u8>>> {
        if let Some(bytes) = self.known.lock().unwrap().get(hash) {
            return Some(bytes.clone());
        }
        let bytes = std::fs::read(self.metadata_directory.join(format!("{hash}.torrent"))).ok()?;
        (bytes.len() <= MAX_TORRENT_BYTES).then(|| Arc::new(bytes))
    }

    fn remember_torrent(&self, hash: &str, bytes: Arc<Vec<u8>>) {
        let path = self.metadata_directory.join(format!("{hash}.torrent"));
        if !path.exists() {
            // Write then rename, so a concurrent reader never sees a partial file.
            let partial = self
                .metadata_directory
                .join(format!("{hash}.{}.partial", std::process::id()));
            if std::fs::write(&partial, bytes.as_slice()).is_ok()
                && std::fs::rename(&partial, &path).is_err()
            {
                let _ = std::fs::remove_file(&partial);
            }
        }
        self.known.lock().unwrap().insert(hash.into(), bytes);
    }

    async fn torrent_bytes(
        &self,
        reference: &TorrentReference,
    ) -> Result<Arc<Vec<u8>>, TorrentFailure> {
        match reference {
            TorrentReference::Magnet {
                uri,
                info_hash_v1: Some(hash),
            } => {
                if let Some(bytes) = self.cached_torrent(hash) {
                    return Ok(bytes);
                }
                let response = self
                    .client
                    .post(format!("{}/torrents/resolve_magnet", self.base))
                    .header("Content-Type", "text/plain")
                    .body(uri.clone())
                    .send()
                    .await
                    .map_err(http_error)?;
                Ok(Arc::new(
                    Self::response_bytes(response, MAX_TORRENT_BYTES).await?,
                ))
            }
            TorrentReference::Magnet { .. } => Err(TorrentFailure::new(
                "torrent_service_failed",
                "rqbit requires a v1 infohash",
            )),
            TorrentReference::Http { uri, post_form, .. } => {
                // Fetch .torrent files ourselves: rqbit's URL API does not carry form fields.
                let client = Client::builder()
                    .redirect(reqwest::redirect::Policy::limited(5))
                    .timeout(self.timeout.min(Duration::from_secs(30)))
                    .build()
                    .map_err(|e| TorrentFailure::new("torrent_service_failed", e.to_string()))?;
                let request = match post_form {
                    Some(form) => client.post(uri).form(form),
                    None => client.get(uri),
                };
                let response = request.send().await.map_err(http_error)?;
                Ok(Arc::new(
                    Self::response_bytes(response, MAX_TORRENT_BYTES).await?,
                ))
            }
        }
    }

    /// Bytes of a torrent whose metadata this session or an earlier one has read.
    fn known_torrent(&self, torrent: &TorrentCatalog) -> Result<Arc<Vec<u8>>, TorrentFailure> {
        self.cached_torrent(&torrent.info_hash_v1).ok_or_else(|| {
            TorrentFailure::new(
                "torrent_service_failed",
                "Read metadata before opening a file",
            )
        })
    }

    fn torrent_directory(&self, hash: &str) -> PathBuf {
        self.directory.join("torrents").join(hash)
    }

    fn create_placeholders(&self, torrent: &TorrentCatalog) -> Result<PathBuf, TorrentFailure> {
        let output = self.torrent_directory(&torrent.info_hash_v1);
        if output.exists() {
            std::fs::remove_dir_all(&output)?;
        }
        std::fs::create_dir_all(&output)?;
        // Sparse files: a read far into a large file must not make the
        // filesystem zero-fill everything before it.
        for file in &torrent.files {
            let path = file
                .path
                .split('/')
                .fold(output.clone(), |p, component| p.join(component));
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let placeholder = OpenOptions::new().write(true).create_new(true).open(path)?;
            set_sparse(&placeholder)?;
        }
        Ok(output)
    }

    /// Add the torrent with one selected file and wait until rqbit serves reads.
    async fn manage(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<u64, TorrentFailure> {
        let bytes = self.known_torrent(torrent)?;
        // An add interrupted by a dropped caller may have left the torrent in rqbit.
        let _ = self
            .post_json(
                &format!("/torrents/{}/delete", torrent.info_hash_v1),
                String::new(),
            )
            .await;
        let output = self.create_placeholders(torrent)?;
        let added = self
            .add(
                &[
                    ("only_files", file.id.to_string()),
                    ("output_folder", output.to_string_lossy().into_owned()),
                    ("overwrite", "true".into()),
                ],
                &bytes,
            )
            .await?;
        let id = added.id.ok_or_else(|| {
            TorrentFailure::new(
                "torrent_service_failed",
                "Torrent service did not create a file reader",
            )
        })?;
        let actual = Self::metadata_from(added.details)?;
        if actual.info_hash_v1 != torrent.info_hash_v1
            || actual.files != torrent.files
            || Path::new(&added.output_folder) != output
        {
            let _ = self.forget(id, &torrent.info_hash_v1).await;
            return Err(TorrentFailure::new(
                "torrent_catalog_changed",
                "Torrent identity, files or output directory changed while opening the file",
            ));
        }
        if let Err(error) = self.wait_live(id).await {
            let _ = self.forget(id, &torrent.info_hash_v1).await;
            return Err(error);
        }
        Ok(id)
    }

    async fn stats(&self, id: u64) -> Result<Stats, TorrentFailure> {
        let response = self
            .client
            .get(format!("{}/torrents/{id}/stats/v1", self.base))
            .send()
            .await
            .map_err(http_error)?;
        serde_json::from_slice(&Self::response_bytes(response, 1 << 20).await?)
            .map_err(|e| TorrentFailure::new("torrent_service_failed", e.to_string()))
    }

    async fn wait_live(&self, id: u64) -> Result<(), TorrentFailure> {
        loop {
            let stats = self.stats(id).await?;
            match stats.state.as_str() {
                "live" => return Ok(()),
                "error" => {
                    return Err(TorrentFailure::new(
                        "torrent_service_failed",
                        stats.error.unwrap_or_else(|| "rqbit torrent error".into()),
                    ));
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    /// Keep closed torrents paused, with their pieces, for repeat reads.
    /// Meant for a session that outlives requests; a per-request session
    /// deletes everything at shutdown anyway.
    pub fn with_parking(mut self, limits: ParkingLimits) -> Self {
        self.parking = Some(limits);
        self
    }

    /// Pause a torrent with no open file; None if rqbit refused, and the
    /// caller then deletes it.
    async fn park(&self, id: u64) -> Option<Parked> {
        self.post_json(&format!("/torrents/{id}/pause"), String::new())
            .await
            .ok()?;
        let bytes = self.stats(id).await.map_or(0, |stats| stats.progress_bytes);
        Some(Parked {
            since: Instant::now(),
            bytes,
        })
    }

    /// Select `open` on a parked torrent and start it again.
    async fn resume(&self, id: u64, open: &HashMap<usize, usize>) -> Result<(), TorrentFailure> {
        self.select(id, open).await?;
        self.post_json(&format!("/torrents/{id}/start"), String::new())
            .await?;
        self.wait_live(id).await
    }

    /// Delete parked torrents beyond the caps or the parking time.
    async fn evict(&self, managed: &mut HashMap<String, Managed>) {
        let Some(limits) = &self.parking else {
            return;
        };
        let parked: Vec<_> = managed
            .iter()
            .filter_map(|(hash, entry)| entry.parked.map(|parked| (hash.clone(), parked)))
            .collect();
        for hash in evictions(&parked, limits, Instant::now()) {
            if let Some(entry) = managed.remove(&hash) {
                let _ = self.forget(entry.id, &hash).await;
            }
        }
    }

    /// Delete parked torrents whose parking time is over.
    pub async fn expire_parked(&self) {
        let mut managed = self.managed.lock().await;
        self.evict(&mut managed).await;
    }

    /// How many torrents are parked now.
    pub async fn parked(&self) -> usize {
        let managed = self.managed.lock().await;
        managed
            .values()
            .filter(|entry| entry.parked.is_some())
            .count()
    }

    /// Delete the torrent from rqbit together with its partial data.
    async fn forget(&self, id: u64, hash: &str) -> Result<(), TorrentFailure> {
        let result = self
            .post_json(&format!("/torrents/{id}/delete"), String::new())
            .await;
        let _ = std::fs::remove_dir_all(self.torrent_directory(hash));
        result
    }

    async fn select(&self, id: u64, open: &HashMap<usize, usize>) -> Result<(), TorrentFailure> {
        let files: BTreeSet<_> = open.keys().collect();
        self.post_json(
            &format!("/torrents/{id}/update_only_files"),
            serde_json::json!({ "only_files": files }).to_string(),
        )
        .await
    }
}

#[async_trait]
impl TorrentService for RqbitService {
    fn description(&self) -> TorrentServiceRun {
        TorrentServiceRun {
            backend: "rqbit".into(),
            working_directory: self.directory.clone(),
            started: true,
            stopped: self.stopped.load(Ordering::SeqCst),
            shutdown_error: None,
            notes: vec![],
        }
    }

    async fn metadata(
        &self,
        reference: &TorrentReference,
    ) -> Result<TorrentMetadata, TorrentFailure> {
        validate_reference(reference)?;
        let bytes = self.torrent_bytes(reference).await?;
        let added = self.add(&[("list_only", "true".into())], &bytes).await?;
        if added.id.is_some() {
            return Err(TorrentFailure::new(
                "torrent_service_failed",
                "Metadata-only request unexpectedly created a download",
            ));
        }
        let metadata = Self::metadata_from(added.details)?;
        if let TorrentReference::Magnet {
            info_hash_v1: Some(hash),
            ..
        } = reference
            && hash != &metadata.info_hash_v1
        {
            return Err(TorrentFailure::new(
                "torrent_identity_mismatch",
                "Torrent service returned metadata for another infohash",
            ));
        }
        self.remember_torrent(&metadata.info_hash_v1, bytes);
        Ok(metadata)
    }

    async fn open_file(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure> {
        torrent.validate()?;
        if torrent.selected_file(file.id)? != file {
            return Err(TorrentFailure::new(
                "torrent_catalog_changed",
                "Selected file differs from catalog",
            ));
        }
        // Keep the file selected while it is read: a torrent with nothing left
        // to download marks every peer "not needed", and reads of pieces not
        // yet on disk then wait forever.
        let mut managed = self.managed.lock().await;
        if let Some(entry) = managed.get_mut(&torrent.info_hash_v1)
            && entry.parked.is_some()
        {
            let open = HashMap::from([(file.id, 1)]);
            match self.resume(entry.id, &open).await {
                Ok(()) => {
                    entry.parked = None;
                    entry.open = open;
                    let id = entry.id;
                    return Ok(self.reader(&mut managed, torrent, file, id));
                }
                // Start over with a fresh add.
                Err(_) => {
                    let id = entry.id;
                    managed.remove(&torrent.info_hash_v1);
                    let _ = self.forget(id, &torrent.info_hash_v1).await;
                }
            }
        }
        let id = match managed.get_mut(&torrent.info_hash_v1) {
            Some(entry) => {
                // Count the file only once rqbit accepted it, so an open
                // dropped midway (timeout, cancellation) holds nothing.
                let mut open = entry.open.clone();
                *open.entry(file.id).or_default() += 1;
                self.select(entry.id, &open).await?;
                entry.open = open;
                entry.id
            }
            None => {
                let id = self.manage(torrent, file).await?;
                managed.insert(
                    torrent.info_hash_v1.clone(),
                    Managed {
                        id,
                        open: HashMap::from([(file.id, 1)]),
                        warmers: HashMap::new(),
                        parked: None,
                    },
                );
                id
            }
        };
        Ok(self.reader(&mut managed, torrent, file, id))
    }

    async fn close_file(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<(), TorrentFailure> {
        let mut managed = self.managed.lock().await;
        let Some(entry) = managed.get_mut(&torrent.info_hash_v1) else {
            return Ok(());
        };
        Self::release(entry, file.id);
        if entry.open.is_empty() {
            let id = entry.id;
            if self.parking.is_some()
                && let Some(parked) = self.park(id).await
            {
                entry.parked = Some(parked);
                self.evict(&mut managed).await;
                return Ok(());
            }
            managed.remove(&torrent.info_hash_v1);
            self.forget(id, &torrent.info_hash_v1).await
        } else {
            self.select(entry.id, &entry.open).await
        }
    }

    async fn shutdown(&self) -> Result<(), TorrentFailure> {
        if let Some(mut child) = self.child.lock().await.take() {
            if child.try_wait()?.is_none() {
                child.kill().await?;
            }
            child.wait().await?;
            self.stopped.store(true, Ordering::SeqCst);
        }
        self.managed.lock().await.clear();
        drop(self.lock.lock().unwrap().take());
        // rqbit's file handles close with the process; Windows may release them a moment later.
        let mut last = Ok(());
        for _ in 0..20 {
            last = match std::fs::remove_dir_all(&self.directory) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => return Ok(()),
            };
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        last.map_err(|e| {
            TorrentFailure::new(
                "torrent_cleanup_failed",
                format!("Cannot remove session directory: {e}"),
            )
        })
    }
}

impl RqbitService {
    /// Whether the rqbit process is still running.
    pub async fn alive(&self) -> bool {
        match self.child.lock().await.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// The stream URL of an opened file, with its background warm-up started.
    fn reader(
        &self,
        managed: &mut HashMap<String, Managed>,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
        id: u64,
    ) -> MediaInput {
        let url = format!("{}/torrents/{id}/stream/{}", self.base, file.id);
        let entry = managed
            .get_mut(&torrent.info_hash_v1)
            .expect("managed while open");
        entry
            .warmers
            .entry(file.id)
            .or_insert_with(|| self.warm(&url, file.size_bytes));
        MediaInput::DownloadService {
            url,
            size_bytes: file.size_bytes,
        }
    }

    fn release(entry: &mut Managed, file_id: usize) {
        if let Some(count) = entry.open.get_mut(&file_id) {
            *count -= 1;
            if *count == 0 {
                entry.open.remove(&file_id);
                for warmer in entry.warmers.remove(&file_id).unwrap_or_default() {
                    warmer.abort();
                }
            }
        }
    }

    /// Start reading both ends of a file in the background; the bytes are
    /// dropped, only the pieces rqbit verifies on the way matter.
    fn warm(&self, url: &str, size: u64) -> Vec<tokio::task::AbortHandle> {
        let mut ranges = vec![(0, WARM_HEAD_BYTES.min(size) - 1)];
        if size > WARM_HEAD_BYTES {
            ranges.push((
                size.saturating_sub(WARM_TAIL_BYTES).max(WARM_HEAD_BYTES),
                size - 1,
            ));
        }
        ranges
            .into_iter()
            .map(|(start, end)| {
                let request = self
                    .client
                    .get(url)
                    .header("Range", format!("bytes={start}-{end}"));
                tokio::spawn(async move {
                    if let Ok(mut response) = request.send().await {
                        while let Ok(Some(_)) = response.chunk().await {}
                    }
                })
                .abort_handle()
            })
            .collect()
    }
}

/// Remove what earlier sessions left behind and bound the metadata cache.
fn sweep_cache(cache: &Path) {
    // Directories of the previous layout, one per request, never removed.
    if let Ok(entries) = std::fs::read_dir(cache) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("request-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(cache.join("sessions")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !lock_is_held(&path.join("session.lock")) {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(cache.join("metadata")) {
        let mut files: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((modified, entry.path()))
            })
            .collect();
        for (_, path) in &files {
            if path.extension().is_some_and(|e| e == "partial") {
                let _ = std::fs::remove_file(path);
            }
        }
        if files.len() > METADATA_CACHE_ENTRIES {
            files.sort();
            for (_, path) in &files[..files.len() - METADATA_CACHE_ENTRIES] {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// A live session holds its lock file open without delete sharing.
#[cfg(windows)]
fn hold_lock(path: &Path) -> Result<File, std::io::Error> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0)
        .open(path)
}
#[cfg(windows)]
fn lock_is_held(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => false,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}
#[cfg(not(windows))]
fn hold_lock(path: &Path) -> Result<File, std::io::Error> {
    let file = File::create(path)?;
    std::fs::write(path, std::process::id().to_string())?;
    Ok(file)
}
/// Without a portable advisory lock, treat a session as live while its process exists.
#[cfg(not(windows))]
fn lock_is_held(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists())
}

#[cfg(windows)]
fn set_sparse(file: &File) -> Result<(), std::io::Error> {
    use std::{ffi::c_void, os::windows::io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            handle: *mut c_void,
            code: u32,
            input: *mut c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
    let mut returned = 0;
    // FSCTL_SET_SPARSE on a newly created, still empty, owned placeholder.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x900c4,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(windows))]
fn set_sparse(_file: &File) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_torrents_are_evicted_oldest_first_by_age_count_and_bytes() {
        let now = Instant::now();
        let limits = ParkingLimits {
            max_torrents: 2,
            max_bytes: 100,
            ttl: Duration::from_secs(60),
        };
        let parked = |name: &str, age: u64, bytes: u64| {
            (
                name.to_owned(),
                Parked {
                    since: now - Duration::from_secs(age),
                    bytes,
                },
            )
        };
        assert!(evictions(&[parked("a", 5, 10), parked("b", 1, 10)], &limits, now).is_empty());
        assert_eq!(
            evictions(
                &[parked("young", 1, 10), parked("old", 61, 10)],
                &limits,
                now
            ),
            ["old"]
        );
        assert_eq!(
            evictions(
                &[parked("c", 1, 1), parked("a", 3, 1), parked("b", 2, 1)],
                &limits,
                now
            ),
            ["a"],
            "count cap removes the oldest"
        );
        assert_eq!(
            evictions(&[parked("new", 1, 60), parked("old", 2, 60)], &limits, now),
            ["old"],
            "byte cap removes the oldest until the rest fit"
        );
        assert_eq!(
            evictions(&[parked("huge", 1, 500)], &limits, now),
            ["huge"],
            "one torrent over the byte cap is not kept"
        );
    }

    #[test]
    fn sweep_removes_dead_sessions_and_old_layout_but_keeps_live_ones() {
        let cache = tempfile::tempdir().unwrap();
        let root = cache.path();
        let legacy = root.join("request-1-2/files");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("movie.mkv"), b"partial").unwrap();
        let dead = root.join("sessions/1-1");
        std::fs::create_dir_all(&dead).unwrap();
        std::fs::write(dead.join("session.lock"), b"").unwrap();
        let live = root.join("sessions/2-2");
        std::fs::create_dir_all(&live).unwrap();
        let _held = hold_lock(&live.join("session.lock")).unwrap();
        std::fs::create_dir_all(root.join("metadata")).unwrap();
        std::fs::write(root.join("metadata/abc.1.partial"), b"x").unwrap();
        sweep_cache(root);
        assert!(!root.join("request-1-2").exists());
        assert!(!dead.exists());
        #[cfg(windows)]
        assert!(live.join("session.lock").exists());
        assert!(!root.join("metadata/abc.1.partial").exists());
    }
}
