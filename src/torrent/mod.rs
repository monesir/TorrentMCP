//! Torrent metadata and file inspection, independent of search adapters and the P2P implementation.
pub mod coverage;
mod pool;
mod rqbit;
mod torbox;
mod workflow;

use crate::{
    SearchError, TorrentReference,
    media::{MediaInput, MediaInspectionReport},
};
use async_trait::async_trait;
pub use pool::TorrentPool;
pub use rqbit::{ParkingLimits, RqbitService};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
pub use torbox::{RemoteFile, TorboxClient, TorboxFirst, TorboxKey, match_file, torbox_first};
pub use workflow::{
    FileChoice, FileInspection, InspectionFailure, InspectionTarget, MAX_FILES_PER_REQUEST,
    MAX_PARALLEL_INSPECTIONS, TorrentOperation, TorrentSource, has_duplicates, inspect_files,
    perform, run, run_with_service, sample_files, verify_catalog,
};

#[derive(Debug, Clone)]
pub struct TorrentRuntime {
    pub rqbit: PathBuf,
    pub cache_directory: PathBuf,
    /// None leaves downloads unthrottled: inspection reads only what ffprobe asks for.
    pub download_limit_bytes_per_second: Option<u64>,
    /// With a key, files of torrents TorBox has cached are read from TorBox.
    pub torbox: Option<TorboxKey>,
}

impl TorrentRuntime {
    pub fn bundled() -> Self {
        let directory = std::env::current_exe()
            .unwrap_or_default()
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_owned();
        let cache = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self {
            rqbit: directory.join(if cfg!(windows) {
                "runtime/torrent/rqbit.exe"
            } else {
                "runtime/torrent/rqbit"
            }),
            cache_directory: cache.join("torrent-search/inspections"),
            download_limit_bytes_per_second: None,
            torbox: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TorrentServiceLimits {
    pub timeout_seconds: u64,
}
impl Default for TorrentServiceLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: 90,
        }
    }
}
impl TorrentServiceLimits {
    pub fn validate(&self) -> Result<(), SearchError> {
        if !(1..=120).contains(&self.timeout_seconds) {
            return Err(SearchError::InvalidRequest(
                "Torrent service timeout_seconds must be 1..120".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorrentFile {
    pub id: usize,
    pub path: String,
    pub size_bytes: u64,
    pub media_kind: String,
}

pub fn media_kind(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mkv" | "mp4" | "m4v" | "avi" | "m2ts" | "ts" | "webm" | "mov" | "mpg" | "mpeg" => "video",
        "wav" | "flac" | "ogg" | "m4a" | "mp3" | "aac" => "audio",
        "srt" | "ass" | "ssa" | "sub" | "vtt" => "subtitle",
        _ => "other",
    }
}

pub(crate) fn safe_components(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.is_empty()
                && !matches!(part, "." | "..")
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorrentCatalog {
    pub schema_version: u32,
    pub source_reference: TorrentReference,
    pub resolved_reference: TorrentReference,
    pub info_hash_v1: String,
    pub name: Option<String>,
    pub files: Vec<TorrentFile>,
}
impl TorrentCatalog {
    pub fn validate(&self) -> Result<(), SearchError> {
        validate_reference(&self.source_reference)?;
        validate_reference(&self.resolved_reference)?;
        let mut paths = std::collections::HashSet::new();
        if self.schema_version != 1
            || self.info_hash_v1.len() != 40
            || !self
                .info_hash_v1
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || self.files.is_empty()
            || self.files.len() > 10000
            || self.files.iter().enumerate().any(|(i, f)| {
                f.id != i
                    || !safe_components(&f.path)
                    || !paths.insert(f.path.to_ascii_lowercase())
                    || f.size_bytes > 16 * 1024u64.pow(4)
                    || f.media_kind != media_kind(&f.path)
            })
        {
            return Err(SearchError::InvalidRequest(
                "Invalid torrent catalog or unsafe file paths".into(),
            ));
        }
        for reference in [&self.source_reference, &self.resolved_reference] {
            if let TorrentReference::Magnet {
                info_hash_v1: Some(hash),
                ..
            } = reference
                && hash != &self.info_hash_v1
            {
                return Err(SearchError::InvalidRequest(
                    "Catalog and magnet identities differ".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn selected_file(&self, id: usize) -> Result<&TorrentFile, SearchError> {
        self.files
            .get(id)
            .filter(|f| {
                f.id == id && f.size_bytes > 0 && matches!(f.media_kind.as_str(), "video" | "audio")
            })
            .ok_or_else(|| {
                SearchError::InvalidRequest(
                    "file_id must identify a nonempty video or audio file from the catalog".into(),
                )
            })
    }
}

/// Fill a magnet's omitted `info_hash_v1` from its `btih`, so a pasted magnet
/// works as is. A supplied hash is left for `validate_reference` to check.
pub fn complete_reference(reference: &mut TorrentReference) {
    if let TorrentReference::Magnet { uri, info_hash_v1 } = reference
        && info_hash_v1.is_none()
        && let Ok(TorrentReference::Magnet {
            info_hash_v1: parsed,
            ..
        }) = crate::reference::parse_reference(uri, "torrent")
    {
        *info_hash_v1 = parsed;
    }
}

pub fn validate_reference(reference: &TorrentReference) -> Result<(), SearchError> {
    let parsed = crate::reference::parse_reference(reference.uri(), "torrent")
        .map_err(|e| SearchError::InvalidRequest(e.to_string()))?;
    if std::mem::discriminant(reference) != std::mem::discriminant(&parsed) {
        return Err(SearchError::InvalidRequest(
            "Reference kind differs from URI scheme".into(),
        ));
    }
    if let (
        TorrentReference::Magnet {
            info_hash_v1: supplied,
            ..
        },
        TorrentReference::Magnet {
            info_hash_v1: actual,
            ..
        },
    ) = (reference, parsed)
        && (actual.is_none()
            || supplied
                .as_ref()
                .is_some_and(|s| Some(s) != actual.as_ref()))
    {
        return Err(SearchError::InvalidRequest(
            "rqbit requires a consistent v1 magnet identity".into(),
        ));
    }
    if let TorrentReference::Http {
        post_form: Some(form),
        ..
    } = reference
        && (form.is_empty()
            || form.len() > 64
            || form.iter().any(|(k, v)| k.len() > 1024 || v.len() > 32768))
    {
        return Err(SearchError::InvalidRequest(
            "Invalid torrent POST form".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct TorrentMetadata {
    pub info_hash_v1: String,
    pub name: Option<String>,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentServiceRun {
    pub backend: String,
    pub working_directory: PathBuf,
    pub started: bool,
    pub stopped: bool,
    pub shutdown_error: Option<String>,
    /// Why a file was not read the fastest way, e.g. not cached on TorBox.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentOperationReport {
    pub schema_version: u32,
    pub operation: String,
    pub torrent: Option<TorrentCatalog>,
    pub file: Option<TorrentFile>,
    pub media: Option<MediaInspectionReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inspections: Vec<FileInspection>,
    pub service: TorrentServiceRun,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<crate::comparison::MediaAssessment>,
}

#[derive(Debug)]
pub struct TorrentFailure {
    pub code: &'static str,
    pub message: String,
}
impl TorrentFailure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<SearchError> for TorrentFailure {
    fn from(error: SearchError) -> Self {
        Self::new("torrent_service_failed", error.to_string())
    }
}
impl From<std::io::Error> for TorrentFailure {
    fn from(error: std::io::Error) -> Self {
        Self::new("torrent_service_failed", error.to_string())
    }
}

/// Implementations supply verified file bytes; the search engine never handles P2P itself.
///
/// A service is shared: several operations may call it concurrently, and it
/// outlives any one of them until `shutdown`.
#[async_trait]
pub trait TorrentService: Send + Sync {
    fn description(&self) -> TorrentServiceRun;
    async fn metadata(
        &self,
        reference: &TorrentReference,
    ) -> Result<TorrentMetadata, TorrentFailure>;
    /// Make one file readable. Each successful call is paired with `close_file`.
    async fn open_file(
        &self,
        torrent: &TorrentCatalog,
        file: &TorrentFile,
    ) -> Result<MediaInput, TorrentFailure>;
    /// Release a file opened by `open_file`; the service may discard its data.
    async fn close_file(
        &self,
        _torrent: &TorrentCatalog,
        _file: &TorrentFile,
    ) -> Result<(), TorrentFailure> {
        Ok(())
    }
    async fn shutdown(&self) -> Result<(), TorrentFailure>;
}

pub(crate) fn cancelled() -> TorrentFailure {
    TorrentFailure::new("cancelled", "Torrent operation cancelled")
}
pub(crate) fn timeout() -> TorrentFailure {
    TorrentFailure::new(
        "torrent_service_timeout",
        "Torrent service deadline exceeded",
    )
}

pub(crate) async fn bounded<T>(
    seconds: u64,
    cancel: &CancellationToken,
    work: impl std::future::Future<Output = Result<T, TorrentFailure>>,
) -> Result<T, TorrentFailure> {
    tokio::select! { biased; _ = cancel.cancelled() => Err(cancelled()), result = tokio::time::timeout(std::time::Duration::from_secs(seconds), work) => result.unwrap_or_else(|_| Err(timeout())) }
}
