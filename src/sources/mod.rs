pub mod limetorrents;
pub mod native;
pub mod qbit;

use crate::{
    SearchError, SearchLimits, SearchRequest, SearchResult, SourceDescriptor, SourceReport,
    TorrentReference,
};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

pub struct SourceOutcome {
    pub report: SourceReport,
    pub results: Vec<SearchResult>,
}

/// A native Rust source implements the same contract as a qBittorrent adapter.
#[async_trait]
pub trait SearchSource: Send + Sync {
    fn descriptor(&self) -> &SourceDescriptor;
    async fn search(
        &self,
        request: &SearchRequest,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> SourceOutcome;
    async fn resolve(
        &self,
        uri: &str,
        limits: &SearchLimits,
        cancellation: CancellationToken,
    ) -> Result<TorrentReference, SearchError>;
}
