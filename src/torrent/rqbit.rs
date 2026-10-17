use super::*;
use reqwest::{Client, Response};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::process::{Child, Command};

fn http_error(error: reqwest::Error) -> TorrentFailure {
    if error.is_timeout() {
        super::timeout()
    } else {
        TorrentFailure::new("torrent_service_failed", error.without_url().to_string())
    }
}

/// An owned rqbit process with a private output directory and loopback API.
/// Dropping it kills the process even when a caller drops the workflow future.
pub struct RqbitService {
    child: Child,
    client: Client,
    base: String,
    directory: PathBuf,
    payload: Option<(TorrentReference, bool, Vec<u8>)>,
    stopped: bool,
}

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
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = cache.join(format!("request-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("rqbit.log"))?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(limits.timeout_seconds))
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
        command
            .args([
                "-v",
                "error",
                "--http-api-listen-addr",
                &address.to_string(),
                "--disable-dht-persistence",
                "--disable-upnp-port-forward",
                "--disable-lsd",
                "--disable-tcp-listen",
                "--ratelimit-download",
                "4194304",
                "--ratelimit-upload",
                "1024",
                "--peer-limit",
                "30",
                "server",
                "start",
                "--disable-persistence",
            ])
            .arg(&directory);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let child = command
            .spawn()
            .map_err(|e| TorrentFailure::new("torrent_service_unavailable", e.to_string()))?;
        let mut service = Self {
            child,
            client,
            base: format!("http://{address}"),
            directory,
            payload: None,
            stopped: false,
        };
        let ready = bounded(limits.timeout_seconds.min(10), cancel, async {
            loop {
                if let Some(status) = service.child.try_wait()? {
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
        is_url: bool,
        body: Vec<u8>,
    ) -> Result<AddResponse, TorrentFailure> {
        let response = self
            .client
            .post(format!("{}/torrents", self.base))
            .query(query)
            .query(&[("is_url", is_url)])
            .header(
                "Content-Type",
                if is_url {
                    "text/plain"
                } else {
                    "application/x-bittorrent"
                },
            )
            .body(body)
            .send()
            .await
            .map_err(http_error)?;
        serde_json::from_slice(&Self::response_bytes(response, 8 * 1024 * 1024).await?)
            .map_err(|e| TorrentFailure::new("torrent_metadata_invalid", e.to_string()))
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

    fn create_placeholders(&self, torrent: &TorrentCatalog) -> Result<PathBuf, TorrentFailure> {
        let output = self.directory.join("files");
        std::fs::create_dir(&output)?;
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
}

#[async_trait]
impl TorrentService for RqbitService {
    fn description(&self) -> TorrentServiceRun {
        TorrentServiceRun {
            backend: "rqbit".into(),
            working_directory: self.directory.clone(),
            started: true,
            stopped: self.stopped,
            shutdown_error: None,
        }
    }

    async fn metadata(
        &mut self,
        reference: &TorrentReference,
    ) -> Result<TorrentMetadata, TorrentFailure> {
        validate_reference(reference)?;
        let (is_url, body) = match reference {
            TorrentReference::Magnet { uri, .. } => (true, uri.as_bytes().to_vec()),
            TorrentReference::Http { uri, post_form, .. } => {
                // Fetch POST references ourselves: rqbit's URL API does not carry form fields.
                let client = Client::builder()
                    .redirect(reqwest::redirect::Policy::limited(5))
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| TorrentFailure::new("torrent_service_failed", e.to_string()))?;
                let request = match post_form {
                    Some(form) => client.post(uri).form(form),
                    None => client.get(uri),
                };
                let response = request.send().await.map_err(http_error)?;
                (
                    false,
                    Self::response_bytes(response, 10 * 1024 * 1024).await?,
                )
            }
        };
        let added = self
            .add(
                &[
                    ("list_only", "true".into()),
                    ("timeout_ms", "120000".into()),
                ],
                is_url,
                body.clone(),
            )
            .await?;
        if added.id.is_some() {
            return Err(TorrentFailure::new(
                "torrent_service_failed",
                "Metadata-only request unexpectedly created a download",
            ));
        }
        let metadata = Self::metadata_from(added.details)?;
        self.payload = Some((reference.clone(), is_url, body));
        Ok(metadata)
    }

    async fn open_file(
        &mut self,
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
        let (reference, is_url, body) = self.payload.as_ref().ok_or_else(|| {
            TorrentFailure::new(
                "torrent_service_failed",
                "Read metadata before opening a file",
            )
        })?;
        if reference != &torrent.resolved_reference {
            return Err(TorrentFailure::new(
                "torrent_identity_mismatch",
                "Reference differs from inspected metadata",
            ));
        }
        let output = self.create_placeholders(torrent)?;
        let added = self
            .add(
                &[
                    ("only_files", file.id.to_string()),
                    ("output_folder", output.to_string_lossy().into_owned()),
                    ("overwrite", "true".into()),
                    ("timeout_ms", "120000".into()),
                ],
                *is_url,
                body.clone(),
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
            return Err(TorrentFailure::new(
                "torrent_catalog_changed",
                "Torrent identity, files or output directory changed while opening the file",
            ));
        }
        // Keep only range-driven reads after initialization; stop the ordinary file downloader.
        loop {
            let response = self
                .client
                .post(format!("{}/torrents/{id}/update_only_files", self.base))
                .header("Content-Type", "application/json")
                .body("{\"only_files\":[]}")
                .send()
                .await
                .map_err(http_error)?;
            if response.status().is_success() {
                Self::response_bytes(response, 4096).await?;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(MediaInput::DownloadService {
            url: format!("{}/torrents/{id}/stream/{}", self.base, file.id),
            size_bytes: file.size_bytes,
        })
    }

    async fn shutdown(&mut self) -> Result<(), TorrentFailure> {
        if !self.stopped {
            if self.child.try_wait()?.is_none() {
                self.child.kill().await?;
            }
            self.child.wait().await?;
            self.stopped = true;
        }
        Ok(())
    }
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
