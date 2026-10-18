mod gateway;
mod probe;
mod report;

use crate::SearchError;
pub use probe::inspect_media;
pub(crate) use report::language as normalize_language;
pub use report::*;
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaInput {
    LocalFile {
        path: PathBuf,
    },
    /// A byte-addressable endpoint supplied by an independent local downloader.
    DownloadService {
        url: String,
        size_bytes: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaInspectionRequest {
    pub input: MediaInput,
    #[serde(default)]
    pub limits: MediaInspectionLimits,
    #[serde(default)]
    pub selection: StreamSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaInspectionLimits {
    pub timeout_seconds: u64,
    pub max_read_bytes: u64,
}

impl Default for MediaInspectionLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: 60,
            max_read_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StreamSelection {
    pub video_stream_index: Option<u64>,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
}

impl MediaInspectionRequest {
    pub fn validate(&self) -> Result<(), SearchError> {
        let invalid = |text: &str| SearchError::InvalidRequest(text.into());
        if !(1..=120).contains(&self.limits.timeout_seconds)
            || !(64 * 1024..=128 * 1024 * 1024).contains(&self.limits.max_read_bytes)
        {
            return Err(invalid(
                "Media timeout must be 1..120 seconds and read budget 64 KiB..128 MiB",
            ));
        }
        for list in [
            &self.selection.audio_languages,
            &self.selection.subtitle_languages,
        ] {
            if list.len() > 32
                || list.iter().any(|code| {
                    !(2..=3).contains(&code.len())
                        || !code.bytes().all(|byte| byte.is_ascii_lowercase())
                        || matches!(code.as_str(), "und" | "mul" | "zxx")
                })
            {
                return Err(invalid(
                    "Use at most 32 lowercase ISO language codes per selection",
                ));
            }
        }
        match &self.input {
            MediaInput::LocalFile { path } if path.as_os_str().is_empty() => {
                Err(invalid("A media file path is required"))
            }
            MediaInput::DownloadService { url, size_bytes } => {
                let parsed =
                    url::Url::parse(url).map_err(|_| invalid("Invalid download service URL"))?;
                let loopback = match parsed.host() {
                    Some(url::Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
                    Some(url::Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
                    _ => false,
                };
                if parsed.scheme() != "http"
                    || !loopback
                    || parsed.port_or_known_default().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                    || *size_bytes == 0
                    || *size_bytes > 16 * 1024u64.pow(4)
                    || url.len() > 4096
                {
                    Err(invalid(
                        "Download service requires an HTTP loopback IP URL and a known nonzero size (<=16 TiB)",
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}
