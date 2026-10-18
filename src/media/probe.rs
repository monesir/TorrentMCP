use super::{
    MediaInput, MediaInspectionReport, MediaInspectionRequest, gateway::ReadGateway,
    report::build_report,
};
use crate::SearchError;
use std::{
    path::Path,
    process::Stdio,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub struct InspectionOutcome {
    pub report: Option<MediaInspectionReport>,
    pub failure_code: Option<String>,
    pub diagnostic: Option<String>,
}

async fn capped(mut reader: impl AsyncRead + Unpin, cap: u64) -> Result<Vec<u8>, SearchError> {
    let mut data = Vec::new();
    (&mut reader).take(cap + 1).read_to_end(&mut data).await?;
    if data.len() as u64 > cap {
        return Err(SearchError::Runtime("ffprobe output limit exceeded".into()));
    }
    Ok(data)
}

pub async fn inspect_media(
    request: MediaInspectionRequest,
    ffprobe: &Path,
    cancellation: CancellationToken,
) -> Result<InspectionOutcome, SearchError> {
    request.validate()?;
    if cancellation.is_cancelled() {
        return Ok(InspectionOutcome {
            report: None,
            failure_code: Some("cancelled".into()),
            diagnostic: None,
        });
    }
    let started = Instant::now();
    let gateway = ReadGateway::start(&request.input, request.limits.max_read_bytes).await?;
    let mut command = Command::new(ffprobe);
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
        .args([
            "-v",
            "error",
            "-of",
            "json",
            "-show_streams",
            "-show_format",
            "-show_error",
            "-protocol_whitelist",
            "http,tcp",
            "-format_whitelist",
            "matroska,webm,mov,mp4,m4a,3gp,3g2,mj2,mpegts,avi,mpeg,ogg,flac,wav",
            "-threads",
            "1",
            "-max_alloc",
            "67108864",
            "-probesize",
            "5000000",
            "-analyzeduration",
            "5000000",
        ])
        // A cold torrent may need much longer than five seconds for its first piece.
        // The outer deadline still bounds the whole operation and cancels the gateway.
        .arg("-rw_timeout")
        .arg((request.limits.timeout_seconds * 1_000_000).to_string())
        .arg("-i")
        .arg(&gateway.url);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            gateway.stop().await;
            return Err(SearchError::Runtime(format!(
                "Cannot start ffprobe: {error}"
            )));
        }
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let work = async {
        let (out, err, status) = tokio::try_join!(
            capped(stdout, 2 * 1024 * 1024),
            capped(stderr, 128 * 1024),
            async { child.wait().await.map_err(SearchError::from) }
        )?;
        Ok::<_, SearchError>((out, err, status.success()))
    };
    let result = tokio::select! {
        _ = cancellation.cancelled() => Err("cancelled"),
        output = tokio::time::timeout(Duration::from_secs(request.limits.timeout_seconds), work) => match output {
            Err(_) => Err("probe_timeout"), Ok(output) => Ok(output),
        }
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    let state = gateway.state.clone();
    let size = gateway.size;
    gateway.stop().await;
    let bytes = state.bytes_read.load(Ordering::Relaxed);
    let requests = state.requests.load(Ordering::Relaxed);
    let read_failure = state.failure.lock().unwrap().clone();
    let (stdout, stderr, successful) = match result {
        Err(code) => {
            return Ok(InspectionOutcome {
                report: None,
                failure_code: Some(code.into()),
                diagnostic: read_failure,
            });
        }
        Ok(Err(error)) => return Err(error),
        Ok(Ok(output)) => output,
    };
    let raw = serde_json::from_slice(&stdout).unwrap_or(serde_json::Value::Null);
    let kind = match request.input {
        MediaInput::LocalFile { .. } => "local_file",
        MediaInput::DownloadService { .. } => "download_service",
        MediaInput::RemoteFile { .. } => "remote_file",
    };
    let mut report = build_report(raw, &request.selection, size, kind);
    let failure_code = read_failure
        .as_ref()
        .map(|message| {
            if message == "read_budget_exceeded" {
                "read_budget_exceeded"
            } else {
                "source_read_failed"
            }
        })
        .or_else(|| {
            if !successful || report.is_none() {
                Some("probe_failed")
            } else if !stderr.is_empty() {
                Some("probe_diagnostics")
            } else {
                None
            }
        })
        .map(str::to_owned);
    if let Some(report) = &mut report {
        report.read_bytes_charged = bytes;
        report.read_requests = requests;
        report.read_budget_bytes = request.limits.max_read_bytes;
        report.elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        if let Some(code) = &failure_code {
            report.limitations.push(code.clone());
        }
    }
    let diagnostic = read_failure.or_else(|| {
        if stderr.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&stderr).trim().to_owned())
        }
    });
    Ok(InspectionOutcome {
        report,
        failure_code,
        diagnostic,
    })
}
