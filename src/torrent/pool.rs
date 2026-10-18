//! One rqbit session kept alive across requests by a long-running process.
use super::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Starts rqbit on first use, restarts it if it died, and stops it after
/// `idle` without any request holding it. Metadata, DHT state and peer
/// connections survive between requests while it runs.
pub struct TorrentPool {
    runtime: TorrentRuntime,
    idle: Duration,
    state: tokio::sync::Mutex<PoolState>,
}

struct PoolState {
    service: Option<Arc<RqbitService>>,
    last_used: Instant,
}

/// The longest per-request timeout; the shared session must serve any of them.
const SHARED_LIMITS: TorrentServiceLimits = TorrentServiceLimits {
    timeout_seconds: 120,
};

impl TorrentPool {
    pub fn new(runtime: TorrentRuntime, idle: Duration) -> Self {
        Self {
            runtime,
            idle,
            state: tokio::sync::Mutex::new(PoolState {
                service: None,
                last_used: Instant::now(),
            }),
        }
    }

    /// The running session, started (or restarted after a crash) when needed.
    /// Hold the returned Arc for the whole operation: the reaper never stops a
    /// session that a request still holds.
    pub async fn acquire(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Arc<RqbitService>, TorrentFailure> {
        let mut state = self.state.lock().await;
        state.last_used = Instant::now();
        if let Some(service) = &state.service {
            if service.alive().await {
                return Ok(service.clone());
            }
            let _ = service.shutdown().await;
            state.service = None;
        }
        let service = Arc::new(RqbitService::start(&self.runtime, &SHARED_LIMITS, cancel).await?);
        state.service = Some(service.clone());
        Ok(service)
    }

    /// Whether a session is running now.
    pub async fn running(&self) -> bool {
        self.state.lock().await.service.is_some()
    }

    /// Stop the session when nothing holds it and it has been idle long enough.
    pub async fn reap_idle(&self) {
        let mut state = self.state.lock().await;
        let idle = state.last_used.elapsed() >= self.idle;
        let unused = state
            .service
            .as_ref()
            .is_some_and(|service| Arc::strong_count(service) == 1);
        if idle
            && unused
            && let Some(service) = state.service.take()
        {
            let _ = service.shutdown().await;
        }
    }

    /// Mark the session used now, so the idle period counts from the end of an operation.
    pub async fn touch(&self) {
        self.state.lock().await.last_used = Instant::now();
    }

    pub async fn shutdown(&self) -> Result<(), TorrentFailure> {
        match self.state.lock().await.service.take() {
            Some(service) => service.shutdown().await,
            None => Ok(()),
        }
    }
}
