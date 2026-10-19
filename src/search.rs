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
    /// Categories the site's own search accepts; others are searched as "all".
    pub categories: Vec<String>,
    pub resolves_http: bool,
    /// What the site actually carries; decides default source selection.
    #[serde(default)]
    pub scope: SourceScope,
    /// Set for sites whose releases are mainly in one language (ISO 639-1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Another plugin for the same site that default searches use instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate_of: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceScope {
    #[default]
    General,
    Video,
    Movies,
    Tv,
    Anime,
    Games,
    Software,
    Music,
    Books,
    Academic,
    Adult,
}

impl SourceDescriptor {
    /// Whether a search with no explicit sources should query this one.
    /// Adult, single-language and duplicate-site plugins run only when named.
    pub fn selected_by_default(&self, category: &str) -> bool {
        use SourceScope::*;
        if self.scope == Adult || self.language.is_some() || self.alternate_of.is_some() {
            return false;
        }
        match category {
            "all" => true,
            "movies" => matches!(self.scope, General | Video | Movies),
            "tv" => matches!(self.scope, General | Video | Tv),
            "anime" => matches!(self.scope, General | Anime),
            "games" => matches!(self.scope, General | Games),
            "software" => matches!(self.scope, General | Software),
            "music" => matches!(self.scope, General | Music),
            "books" => matches!(self.scope, General | Books | Academic),
            _ => self.scope == General,
        }
    }

    /// The category to send to the site: the requested one when its search
    /// supports it, otherwise "all" (results are then filtered by analysis).
    pub fn query_category<'a>(&self, category: &'a str) -> &'a str {
        if self.categories.iter().any(|value| value == category) {
            category
        } else {
            "all"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    /// Omitted means `all`.
    #[serde(default = "all_categories")]
    pub category: String,
    /// Empty or omitted selects all registered sources. Unknown identifiers are errors.
    #[serde(default)]
    pub sources: Vec<String>,
}

fn all_categories() -> String {
    "all".into()
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
    /// Returned results, then stopped on an error or the deadline (a later
    /// page failed, a rate limit). The results are real; `error` says why
    /// there may be more.
    Partial,
    Failed,
    TimedOut,
    Cancelled,
    Unsupported,
}

impl SourceStatus {
    /// Whether the source delivered usable results.
    pub fn answered(self) -> bool {
        matches!(self, Self::Completed | Self::Partial)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReport {
    pub source_id: String,
    pub status: SourceStatus,
    pub elapsed_ms: u64,
    pub result_count: usize,
    pub limited: bool,
    pub error: Option<String>,
    /// Present when the site was searched with "all" instead of the requested category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queried_category: Option<String>,
    /// For a failed source, what kind of failure `error` describes, so an
    /// agent can tell a blocked network from a broken site; see `failure_kind`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
}

/// Classify a source error message:
/// - `dns_failed`: the name did not resolve (a dead domain, or DNS blocking).
/// - `connection_reset`: the connection was cut, typical of network blocking.
/// - `network_unreachable`: no route to the site at all.
/// - `timeout`: no answer in time.
/// - `tls_failed`: the secure connection broke during setup.
/// - `refused`: HTTP 401/403, usually bot protection.
/// - `rate_limited`: HTTP 429.
/// - `site_error`: HTTP 5xx, the site or its proxy is down.
/// - `not_configured`: the source needs local setup first.
/// - `other`: anything else; read `error`.
pub fn failure_kind(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| error.contains(n));
    if has(&["requires local", "not configured"]) {
        "not_configured"
    } else if has(&[
        "dns error",
        "no such host",
        "getaddrinfo",
        "name or service not known",
        "failed to lookup address",
    ]) {
        "dns_failed"
    } else if has(&[
        "10054",
        "forcibly closed",
        "connection reset",
        "reset by peer",
    ]) {
        "connection_reset"
    } else if has(&["unreachable network", "network is unreachable", "10051"]) {
        "network_unreachable"
    } else if has(&["(429", " 429 "]) {
        "rate_limited"
    } else if has(&["(401", "(403"]) {
        "refused"
    } else if has(&["server error (5", "http 5"]) {
        "site_error"
    } else if has(&["handshake", "ssl", "tls", "certificate"]) {
        "tls_failed"
    } else if has(&["timed out", "timeout", "deadline exceeded"]) {
        "timeout"
    } else {
        "other"
    }
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

#[cfg(test)]
mod tests {
    use super::failure_kind;

    #[test]
    fn classifies_the_failures_seen_from_real_sources() {
        for (error, kind) in [
            (
                "error sending request: client error (Connect): dns error: No such host is known. (os error 11001)",
                "dns_failed",
            ),
            (
                "error sending request: client error (Connect): An existing connection was forcibly closed by the remote host. (os error 10054)",
                "connection_reset",
            ),
            (
                "error sending request: client error (Connect): tcp connect error: A socket operation was attempted to an unreachable network. (os error 10051)",
                "network_unreachable",
            ),
            ("error sending request: operation timed out", "timeout"),
            (
                "error sending request: client error (Connect): unexpected EOF during handshake",
                "tls_failed",
            ),
            ("HTTP status client error (403 Forbidden)", "refused"),
            (
                "HTTP status client error (429 Too Many Requests)",
                "rate_limited",
            ),
            ("HTTP status server error (526 <none>)", "site_error"),
            (
                "HTTP status server error (503 Service Unavailable)",
                "site_error",
            ),
            (
                "Jackett requires local qbit/engines/jackett.json with url and api_key",
                "not_configured",
            ),
            ("Source deadline exceeded", "timeout"),
            ("unexpected page layout", "other"),
        ] {
            assert_eq!(failure_kind(error), kind, "{error}");
        }
    }
}
