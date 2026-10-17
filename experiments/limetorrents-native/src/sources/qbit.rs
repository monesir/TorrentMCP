use super::{SearchSource, SourceOutcome};
use crate::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SourceDescriptor, SourceObservation,
    SourceReport, SourceStatus, TorrentReference, reference::parse_reference,
};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
    time::{Instant as Deadline, timeout_at},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Deserialize)]
pub struct QbitPlugin {
    #[serde(flatten)]
    pub descriptor: SourceDescriptor,
    pub sha256: String,
}

#[derive(Deserialize)]
struct FilePin {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    common_files: Vec<FilePin>,
    sources: Vec<QbitPlugin>,
}

pub struct QbitSource {
    assets: PathBuf,
    python: PathBuf,
    plugin: QbitPlugin,
}

pub fn load_sources(
    assets: &Path,
    python: &Path,
) -> Result<Vec<Arc<dyn SearchSource>>, SearchError> {
    let assets = assets.canonicalize()?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(assets.join("sources.json"))?)?;
    if manifest.schema_version != 1 || manifest.sources.is_empty() {
        return Err(SearchError::Runtime(
            "Unsupported or empty source manifest".into(),
        ));
    }
    for file in &manifest.common_files {
        verify_file(&assets, &file.path, &file.sha256)?;
    }
    for required in ["qbit/helpers.py", "qbit/novaprinter.py", "qbit/socks.py"] {
        if !manifest.common_files.iter().any(|pin| pin.path == required) {
            return Err(SearchError::Runtime(format!(
                "Missing common file pin: {required}"
            )));
        }
    }
    let mut identifiers = HashSet::new();
    let mut sources = Vec::new();
    for plugin in manifest.sources {
        let id = &plugin.descriptor.id;
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !identifiers.insert(id.clone())
        {
            return Err(SearchError::Runtime(
                "Source identifiers must be distinct Python module names".into(),
            ));
        }
        let site = url::Url::parse(&plugin.descriptor.site_url)
            .map_err(|_| SearchError::Runtime(format!("Invalid site URL for {id}")))?;
        if !matches!(site.scheme(), "http" | "https") || site.host_str().is_none() {
            return Err(SearchError::Runtime(format!("Invalid site URL for {id}")));
        }
        sources.push(Arc::new(QbitSource {
            assets: assets.clone(),
            python: python.into(),
            plugin,
        }) as Arc<dyn SearchSource>);
    }
    Ok(sources)
}

fn verify_file(root: &Path, relative: &str, expected: &str) -> Result<(), SearchError> {
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(SearchError::Runtime(
            "Asset paths must be relative without traversal".into(),
        ));
    }
    let file = root.join(relative_path).canonicalize()?;
    if !file.starts_with(root) {
        return Err(SearchError::Runtime("Asset escapes its directory".into()));
    }
    let actual = format!("{:x}", Sha256::digest(fs::read(&file)?));
    if actual != expected {
        return Err(SearchError::Runtime(format!(
            "Integrity check failed: {relative}"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
struct Envelope {
    protocol: u32,
    #[serde(flatten)]
    event: WorkerEvent,
}

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum WorkerEvent {
    Result { candidate: RawCandidate },
    Complete { limited: bool },
    Resolved { uri: String },
    Error { message: String },
}

#[derive(Deserialize)]
struct RawCandidate {
    title: String,
    uri: String,
    size_bytes: Option<u64>,
    seeders: Option<u64>,
    leechers: Option<u64>,
    description_url: Option<String>,
    published_unix: Option<u64>,
}

#[derive(Default)]
struct WorkerOutput {
    results: Vec<SearchResult>,
    resolved: Option<TorrentReference>,
    complete: bool,
    limited: bool,
}

struct WorkerRun {
    output: WorkerOutput,
    status: SourceStatus,
    error: Option<String>,
    elapsed_ms: u64,
}

impl QbitSource {
    async fn run(
        &self,
        operation: &str,
        request: &str,
        category: &str,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> WorkerRun {
        let started = Instant::now();
        let mut output = WorkerOutput::default();
        let result = self
            .execute(
                operation,
                request,
                category,
                limits,
                cancellation,
                &mut output,
            )
            .await;
        let (status, error) = match result {
            Ok(()) => (SourceStatus::Completed, None),
            Err((status, error)) => (status, Some(error)),
        };
        WorkerRun {
            output,
            status,
            error,
            elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        }
    }

    async fn execute(
        &self,
        operation: &str,
        request: &str,
        category: &str,
        limits: &SearchLimits,
        cancellation: CancellationToken,
        output: &mut WorkerOutput,
    ) -> Result<(), (SourceStatus, String)> {
        if cancellation.is_cancelled() {
            return Err((SourceStatus::Cancelled, "Search cancelled".into()));
        }
        verify_file(
            &self.assets,
            &format!("qbit/engines/{}.py", self.plugin.descriptor.id),
            &self.plugin.sha256,
        )
        .map_err(|error| (SourceStatus::Failed, error.to_string()))?;
        let deadline = Deadline::now() + limits.source_timeout;
        let mut command = Command::new(&self.python);
        command
            .args(["-I", "-B", "-u", "-X", "utf8"])
            .arg(self.assets.join("qbit_worker.py"))
            .args([
                "--plugin",
                &self.plugin.descriptor.id,
                "--operation",
                operation,
                "--category",
                category,
            ])
            .arg(format!("--value={request}"))
            .arg("--site-url")
            .arg(&self.plugin.descriptor.site_url)
            .arg("--limit")
            .arg(limits.results_per_source.to_string())
            .current_dir(&self.assets)
            .env_remove("qbt_socks_proxy")
            .env_remove("sock_proxy")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the private worker.
        let mut child = command.spawn().map_err(|error| {
            (
                SourceStatus::Failed,
                format!("Cannot start Python worker: {error}"),
            )
        })?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let diagnostics = tokio::spawn(drain_diagnostics(stderr));
        let execution = async {
            self.read_output(stdout, operation, limits, output).await?;
            let status = child.wait().await.map_err(|error| error.to_string())?;
            if !status.success() {
                return Err(format!("Python worker exited with {status}"));
            }
            if !output.complete {
                return Err("Worker closed without a completion event".into());
            }
            Ok::<(), String>(())
        };
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err((SourceStatus::Cancelled, "Search cancelled".into())),
            result = timeout_at(deadline, execution) => match result {
                Ok(result) => result.map_err(|error| (SourceStatus::Failed, error)),
                Err(_) => Err((SourceStatus::TimedOut, "Source deadline exceeded".into())),
            },
        };
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let diagnostics = diagnostics.await.unwrap_or_default();
        result.map_err(|(status, mut message)| {
            if !diagnostics.trim().is_empty() {
                message.push_str(&format!("; {}", diagnostics.trim()));
            }
            (status, message)
        })
    }

    async fn read_output(
        &self,
        stdout: impl AsyncRead + Unpin,
        operation: &str,
        limits: &SearchLimits,
        output: &mut WorkerOutput,
    ) -> Result<(), String> {
        let mut reader = BufReader::new(stdout.take(limits.output_bytes_per_source + 1));
        let mut line = Vec::new();
        let mut received = 0;
        loop {
            line.clear();
            let bytes = reader
                .read_until(b'\n', &mut line)
                .await
                .map_err(|error| error.to_string())?;
            if bytes == 0 {
                break;
            }
            received += bytes as u64;
            if received > limits.output_bytes_per_source || line.len() > 128 * 1024 {
                return Err("Worker output limit exceeded".into());
            }
            let envelope: Envelope = serde_json::from_slice(&line)
                .map_err(|error| format!("Invalid worker protocol: {error}"))?;
            if envelope.protocol != 1 || output.complete {
                return Err("Unexpected worker protocol version or event after completion".into());
            }
            match envelope.event {
                WorkerEvent::Result { candidate } if operation == "search" => {
                    if output.results.len() >= limits.results_per_source {
                        return Err("Worker exceeded the result limit".into());
                    }
                    output.results.push(
                        self.normalize(candidate)
                            .map_err(|error| error.to_string())?,
                    );
                }
                WorkerEvent::Resolved { uri }
                    if operation == "resolve" && output.resolved.is_none() =>
                {
                    let reference = parse_reference(&uri, &self.plugin.descriptor.id)
                        .map_err(|error| error.to_string())?;
                    if !matches!(reference, TorrentReference::Magnet { .. }) {
                        return Err("Resolver did not return a magnet".into());
                    }
                    output.resolved = Some(reference);
                }
                WorkerEvent::Complete { limited } => {
                    output.complete = true;
                    output.limited = limited;
                }
                WorkerEvent::Error { message } => return Err(message),
                _ => return Err("Unexpected worker event".into()),
            }
        }
        Ok(())
    }

    fn normalize(&self, raw: RawCandidate) -> Result<SearchResult, SearchError> {
        let title: String = raw
            .title
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if title.trim().is_empty() || title.len() > 16 * 1024 {
            return Err(SearchError::Runtime("Invalid result title".into()));
        }
        let reference = parse_reference(&raw.uri, &self.plugin.descriptor.id)?;
        let description_url = raw.description_url.filter(|uri| {
            matches!(
                parse_reference(uri, &self.plugin.descriptor.id),
                Ok(TorrentReference::Http { .. })
            )
        });
        Ok(SearchResult {
            title: title.clone(),
            reference,
            observations: vec![SourceObservation {
                source_id: self.plugin.descriptor.id.clone(),
                title,
                raw_title: Some(raw.title),
                site_url: self.plugin.descriptor.site_url.clone(),
                description_url,
                size_bytes: raw.size_bytes,
                seeders: raw.seeders,
                leechers: raw.leechers,
                published_unix: raw.published_unix,
                observed_at_unix: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }],
        })
    }
}

#[async_trait]
impl SearchSource for QbitSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.plugin.descriptor
    }

    async fn search(
        &self,
        request: &SearchRequest,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> SourceOutcome {
        let run = self
            .run(
                "search",
                &request.query,
                &request.category,
                limits,
                cancellation,
            )
            .await;
        SourceOutcome {
            report: SourceReport {
                source_id: self.plugin.descriptor.id.clone(),
                status: run.status,
                elapsed_ms: run.elapsed_ms,
                result_count: run.output.results.len(),
                limited: run.output.limited,
                error: run.error,
            },
            results: run.output.results,
        }
    }

    async fn resolve(
        &self,
        uri: &str,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> Result<TorrentReference, SearchError> {
        let reference = parse_reference(uri, &self.plugin.descriptor.id)?;
        if matches!(reference, TorrentReference::Magnet { .. }) {
            return Ok(reference);
        }
        if !self.plugin.descriptor.resolves_http {
            return Err(SearchError::Runtime(
                "This source does not resolve HTTP references".into(),
            ));
        }
        let url =
            url::Url::parse(uri).map_err(|error| SearchError::InvalidRequest(error.to_string()))?;
        let site = url::Url::parse(&self.plugin.descriptor.site_url).expect("Validated source URL");
        if url.host_str() != site.host_str() {
            return Err(SearchError::InvalidRequest(
                "Reference must belong to the selected source".into(),
            ));
        }
        let run = self.run("resolve", uri, "all", limits, cancellation).await;
        if run.status != SourceStatus::Completed {
            return Err(SearchError::Runtime(
                run.error.unwrap_or_else(|| "Resolution failed".into()),
            ));
        }
        run.output
            .resolved
            .ok_or_else(|| SearchError::Runtime("Source returned no resolved reference".into()))
    }
}

async fn drain_diagnostics(mut stream: impl AsyncRead + Unpin) -> String {
    let mut saved = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(count) => saved
                .extend_from_slice(&buffer[..count.min(8192_usize.saturating_sub(saved.len()))]),
        }
    }
    String::from_utf8_lossy(&saved).into_owned()
}
