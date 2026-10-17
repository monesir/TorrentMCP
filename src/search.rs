use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    Runtime(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub id: String,
    pub name: String,
    pub site_url: String,
    pub categories: Vec<String>,
    pub resolves_http: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub category: String,
    /// Empty selects all registered sources. Unknown identifiers are errors.
    pub sources: Vec<String>,
}

impl SearchRequest {
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.query.trim().is_empty() || self.query.chars().count() > 512 {
            return Err(SearchError::InvalidRequest(
                "Query must contain 1 to 512 characters".into(),
            ));
        }
        if ![
            "all", "anime", "books", "games", "movies", "music", "pictures", "software", "tv",
        ]
        .contains(&self.category.as_str())
        {
            return Err(SearchError::InvalidRequest(format!(
                "Unknown category: {}",
                self.category
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SearchLimits {
    pub source_timeout: Duration,
    pub parallel_sources: usize,
    pub results_per_source: usize,
    pub output_bytes_per_source: u64,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            source_timeout: Duration::from_secs(20),
            parallel_sources: 4,
            results_per_source: 100,
            output_bytes_per_source: 8 * 1024 * 1024,
        }
    }
}

impl SearchLimits {
    pub fn validate(&self) -> Result<(), SearchError> {
        if self.source_timeout.is_zero()
            || !(1..=16).contains(&self.parallel_sources)
            || !(1..=1000).contains(&self.results_per_source)
            || !(1024..=64 * 1024 * 1024).contains(&self.output_bytes_per_source)
        {
            return Err(SearchError::InvalidRequest(
                "Invalid timeout, concurrency, result or output limit".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReport {
    pub source_id: String,
    pub status: SourceStatus,
    pub elapsed_ms: u64,
    pub result_count: usize,
    pub limited: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceObservation {
    pub source_id: String,
    pub title: String,
    /// Source text before display sanitization; absent in legacy reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_title: Option<String>,
    pub site_url: String,
    pub description_url: Option<String>,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u64>,
    pub leechers: Option<u64>,
    pub published_unix: Option<u64>,
    pub observed_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorrentReference {
    Magnet {
        uri: String,
        info_hash_v1: Option<String>,
    },
    /// An HTTP URL is not assumed to be a .torrent download until resolved.
    Http {
        uri: String,
        source_id: String,
        /// Some public sources serve metadata only through an HTML POST form.
        /// Absent means an ordinary GET; credentials and cookies are not serialized.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        post_form: Option<std::collections::BTreeMap<String, String>>,
    },
}

impl TorrentReference {
    pub fn uri(&self) -> &str {
        match self {
            Self::Magnet { uri, .. } | Self::Http { uri, .. } => uri,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub reference: TorrentReference,
    pub observations: Vec<SourceObservation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchReport {
    pub schema_version: u32,
    pub query: String,
    pub category: String,
    pub partial: bool,
    pub sources: Vec<SourceReport>,
    pub results: Vec<SearchResult>,
}
