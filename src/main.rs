use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchEngine, SearchError, SearchLimits, SearchRequest,
    sources::{native::SourceBackend, qbit::load_sources},
};

#[derive(Parser)]
#[command(
    name = "torrentx",
    version,
    about = "Local torrent search; no qBittorrent installation required"
)]
struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = SourceBackend::Auto)]
    source_backend: SourceBackend,
    /// Compatibility override for the original LimeTorrents comparison command.
    #[arg(long, global = true, value_enum)]
    limetorrents_backend: Option<LimeTorrentsBackend>,
    #[arg(
        long,
        global = true,
        help = "Resource directory (defaults to resources beside the executable)"
    )]
    assets: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Explicit Python executable for development; release bundles its own"
    )]
    python: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Explicit ffprobe executable; release bundles its own"
    )]
    ffprobe: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Independent rqbit executable; defaults to runtime/torrent/rqbit.exe"
    )]
    rqbit: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Directory for owned torrent inspection sessions; partial files are preserved"
    )]
    torrent_cache: Option<PathBuf>,
    #[arg(long, global = true, help = "Print a versioned JSON result")]
    json: bool,
    #[command(subcommand)]
    command: Action,
}

#[derive(Clone, Copy, ValueEnum)]
enum LimeTorrentsBackend {
    Python,
    Rust,
}

#[derive(Clone, Copy, ValueEnum)]
enum ReportFormat {
    Json,
    Markdown,
}

#[derive(Subcommand)]
enum Action {
    /// One versioned JSON request from a file or stdin; JSON response on stdout.
    Agent {
        #[arg(long)]
        request: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Json)]
        format: ReportFormat,
        #[arg(
            long,
            help = "Write report to a new file; existing files are preserved"
        )]
        output: Option<PathBuf>,
        #[arg(
            long,
            help = "Run in this process even when a daemon with the same configuration is running"
        )]
        no_daemon: bool,
    },
    /// Keep sources loaded and the torrent session warm; `agent` forwards to it.
    Serve {
        #[arg(
            long,
            default_value_t = 10,
            value_parser = clap::value_parser!(u64).range(1..=1440),
            help = "Stop rqbit after this many minutes without torrent work; it restarts on demand"
        )]
        idle_minutes: u64,
    },
    /// Serve MCP on stdin/stdout: every agent operation as a tool, with a warm torrent session.
    Mcp {
        #[arg(
            long,
            default_value_t = 10,
            value_parser = clap::value_parser!(u64).range(1..=1440),
            help = "Stop rqbit after this many minutes without torrent work; it restarts on demand"
        )]
        idle_minutes: u64,
    },
    /// Register `torrentx mcp` with the agents installed here (Claude Code, Codex, Antigravity, ...).
    Setup {
        #[arg(long = "client", value_delimiter = ',', help = "Only these agents: claude_code, codex, antigravity, claude_desktop, gemini_cli, cursor")]
        clients: Vec<String>,
        #[arg(long, help = "Apply without asking")]
        yes: bool,
        #[arg(long, help = "Remove the registration instead")]
        remove: bool,
    },
    /// Inspect or stop the running daemon.
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Export a saved agent response or raw search report as human-readable tables.
    ExportMarkdown {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Sources,
    Search {
        query: String,
        #[arg(long, default_value = "all")]
        category: String,
        #[arg(long = "source", value_delimiter = ',')]
        sources: Vec<String>,
        #[command(flatten)]
        limits: Limits,
    },
    Resolve {
        #[arg(long)]
        source: String,
        uri: String,
        #[command(flatten)]
        limits: Limits,
    },
}

#[derive(Subcommand, Clone, Copy)]
enum DaemonCommand {
    /// Print the running daemon's health as JSON.
    Status,
    /// Ask the running daemon to stop and wait until it has.
    Stop,
}

#[derive(Args)]
struct Limits {
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..=120))]
    timeout: u64,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(1..=1000))]
    limit: u16,
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=16))]
    parallel: u8,
}

impl Limits {
    fn into_search_limits(self) -> SearchLimits {
        SearchLimits {
            source_timeout: Duration::from_secs(self.timeout),
            parallel_sources: self.parallel.into(),
            results_per_source: self.limit.into(),
            ..SearchLimits::default()
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match execute(cli).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(cli: Cli) -> Result<u8, SearchError> {
    if let Action::ExportMarkdown { input, output } = &cli.command {
        let bytes = read_report(Some(input))?;
        let report: serde_json::Value = serde_json::from_slice(&bytes)?;
        let rendered = torrent_search::markdown::render(&report)?;
        write_report(&rendered, output.as_deref())?;
        return Ok(0);
    }
    if let Action::Daemon { command } = &cli.command {
        return daemon_command(*command).await;
    }
    if let Action::Setup {
        clients,
        yes,
        remove,
    } = &cli.command
    {
        return setup(clients, *yes, *remove);
    }
    let executable = std::env::current_exe()?;
    let directory = executable.parent().expect("Executable has a directory");
    let assets = cli.assets.unwrap_or_else(|| directory.join("resources"));
    let python = cli.python.unwrap_or_else(|| {
        directory.join(if cfg!(windows) {
            "runtime/python.exe"
        } else {
            "runtime/bin/python3"
        })
    });
    let ffprobe = cli.ffprobe.unwrap_or_else(|| {
        directory.join(if cfg!(windows) {
            "runtime/media/ffprobe.exe"
        } else {
            "runtime/media/ffprobe"
        })
    });
    let mut torrent_runtime = torrent_search::torrent::TorrentRuntime::bundled();
    if let Some(path) = cli.rqbit {
        torrent_runtime.rqbit = path;
    }
    if let Some(path) = cli.torrent_cache {
        torrent_runtime.cache_directory = path;
    }
    let configuration = daemon_configuration(
        &assets,
        &python,
        &ffprobe,
        &torrent_runtime,
        cli.source_backend,
        cli.limetorrents_backend,
    );
    if let Action::Agent {
        request,
        format,
        output,
        no_daemon,
    } = &cli.command
    {
        return Ok(execute_agent(
            request.as_deref(),
            &assets,
            &python,
            cli.source_backend,
            cli.limetorrents_backend,
            &ffprobe,
            &torrent_runtime,
            (!no_daemon).then_some(&configuration),
            *format,
            output.as_deref(),
        )
        .await);
    }
    if let Action::Mcp { idle_minutes } = &cli.command {
        // stdout carries the protocol; diagnostics go to stderr.
        let sources = configured_sources(
            &assets,
            &python,
            cli.source_backend,
            cli.limetorrents_backend,
        )?;
        let config = torrent_search::mcp::McpConfig {
            sources,
            ffprobe,
            runtime: torrent_runtime,
            idle: Duration::from_secs(idle_minutes * 60),
        };
        torrent_search::mcp::serve(
            config,
            tokio::io::BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        )
        .await?;
        return Ok(0);
    }
    if let Action::Serve { idle_minutes } = &cli.command {
        let sources = configured_sources(
            &assets,
            &python,
            cli.source_backend,
            cli.limetorrents_backend,
        )?;
        let shutdown = CancellationToken::new();
        let signal = shutdown.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal.cancel();
            }
        });
        let discovery = torrent_search::daemon::discovery_path();
        let config = torrent_search::daemon::DaemonConfig {
            sources,
            ffprobe,
            runtime: torrent_runtime,
            idle: Duration::from_secs(idle_minutes * 60),
            configuration,
            discovery: discovery.clone(),
        };
        torrent_search::daemon::serve(config, shutdown, |info| {
            eprintln!(
                "Daemon listening on 127.0.0.1:{} (pid {}); discovery file {}",
                info.port,
                info.pid,
                discovery.display()
            );
        })
        .await?;
        eprintln!("Daemon stopped");
        return Ok(0);
    }
    let sources = configured_sources(
        &assets,
        &python,
        cli.source_backend,
        cli.limetorrents_backend,
    )?;
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let result = match cli.command {
        Action::Agent { .. }
        | Action::ExportMarkdown { .. }
        | Action::Serve { .. }
        | Action::Mcp { .. }
        | Action::Setup { .. }
        | Action::Daemon { .. } => {
            unreachable!("Handled before loading sources")
        }
        Action::Sources => {
            let descriptors: Vec<_> = sources.iter().map(|source| source.descriptor()).collect();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&descriptors)?);
            } else {
                for descriptor in descriptors {
                    let mut notes = Vec::new();
                    if let Some(language) = &descriptor.language {
                        notes.push(format!("language={language}"));
                    }
                    if let Some(primary) = &descriptor.alternate_of {
                        notes.push(format!("alternate_of={primary}"));
                    }
                    println!(
                        "{}\t{}\t{:?}\t{}\t{}",
                        descriptor.id,
                        descriptor.name,
                        descriptor.scope,
                        descriptor.categories.join(","),
                        notes.join(" ")
                    );
                }
            }
            Ok(0)
        }
        Action::Search {
            query,
            category,
            sources: selected,
            limits,
        } => {
            let engine = SearchEngine::new(sources, limits.into_search_limits())?;
            let report = engine
                .search(
                    SearchRequest {
                        query,
                        category,
                        sources: selected,
                    },
                    cancellation.clone(),
                )
                .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for (index, result) in report.results.iter().enumerate() {
                    let seeds = result
                        .observations
                        .iter()
                        .filter_map(|origin| origin.seeders)
                        .max()
                        .map_or("?".into(), |count| count.to_string());
                    println!(
                        "{}\tseeds={}\t{}\n  {}",
                        index + 1,
                        seeds,
                        result.title,
                        result.reference.uri()
                    );
                }
                for source in &report.sources {
                    eprintln!(
                        "{}: {:?}; {} results; {}ms{}{}",
                        source.source_id,
                        source.status,
                        source.result_count,
                        source.elapsed_ms,
                        if source.limited {
                            "; limit reached"
                        } else {
                            ""
                        },
                        source
                            .error
                            .as_ref()
                            .map_or(String::new(), |error| format!("; {error}"))
                    );
                }
            }
            Ok(if cancellation.is_cancelled() {
                130
            } else if !report.sources.iter().any(|source| source.status.answered()) {
                1
            } else if report.partial {
                3
            } else {
                0
            })
        }
        Action::Resolve {
            source,
            uri,
            limits,
        } => {
            let engine = SearchEngine::new(sources, limits.into_search_limits())?;
            let reference = engine.resolve(&source, &uri, cancellation.clone()).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&reference)?);
            } else {
                println!("{}", reference.uri());
            }
            Ok(0)
        }
    };
    interrupt.abort();
    result
}

fn setup(selected: &[String], yes: bool, remove: bool) -> Result<u8, SearchError> {
    use torrent_search::setup::{Change, apply, clients, installed, plan};
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| SearchError::Runtime("Cannot find the home directory".into()))?;
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let all = clients(&home, app_data.as_deref());
    if let Some(unknown) = selected.iter().find(|id| !all.iter().any(|c| c.id == *id)) {
        return Err(SearchError::InvalidRequest(format!("Unknown agent: {unknown}")));
    }
    let chosen: Vec<_> = installed(all)
        .into_iter()
        .filter(|c| selected.is_empty() || selected.iter().any(|id| id == c.id))
        .collect();
    if chosen.is_empty() {
        eprintln!("No supported agent found. See docs/mcp.md to register manually.");
        return Ok(1);
    }
    let exe = std::env::current_exe()?;
    let changes = plan(&chosen, &exe, remove);
    eprintln!("torrentx MCP server: {} mcp", exe.display());
    for change in &changes {
        eprintln!("  {change}");
    }
    if changes.iter().all(|c| matches!(c, Change::Unchanged { .. })) {
        eprintln!("Nothing to change.");
        return Ok(0);
    }
    if !yes {
        eprint!("Apply these changes? [y/N] ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            eprintln!("Nothing changed.");
            return Ok(1);
        }
    }
    let mut failed = false;
    for change in &changes {
        match apply(change) {
            Ok(done) => eprintln!("  {done}"),
            Err(error) => {
                failed = true;
                eprintln!("  {error}");
            }
        }
    }
    eprintln!("Restart each agent to load the change.");
    Ok(u8::from(failed))
}

/// Everything that decides how a request runs. `agent` forwards to a daemon
/// only when the daemon was started with exactly the same values.
fn daemon_configuration(
    assets: &Path,
    python: &Path,
    ffprobe: &Path,
    runtime: &torrent_search::torrent::TorrentRuntime,
    backend: SourceBackend,
    lime_override: Option<LimeTorrentsBackend>,
) -> serde_json::Value {
    serde_json::json!({
        "assets": assets,
        "python": python,
        "ffprobe": ffprobe,
        "rqbit": runtime.rqbit,
        "torrent_cache": runtime.cache_directory,
        "download_limit_bytes_per_second": runtime.download_limit_bytes_per_second,
        "source_backend": format!("{backend:?}"),
        "limetorrents_backend": lime_override.map(|value| match value {
            LimeTorrentsBackend::Python => "python",
            LimeTorrentsBackend::Rust => "rust",
        }),
    })
}

async fn daemon_command(command: DaemonCommand) -> Result<u8, SearchError> {
    use torrent_search::daemon::{DaemonClient, discovery_path, read_discovery};
    let path = discovery_path();
    let Some(client) = DaemonClient::discover(&path).await else {
        eprintln!("No daemon is running");
        return Ok(1);
    };
    match command {
        DaemonCommand::Status => {
            println!("{}", serde_json::to_string_pretty(&client.health().await?)?);
            Ok(0)
        }
        DaemonCommand::Stop => {
            client.shutdown().await?;
            // The daemon removes its discovery file as the last step of stopping.
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while read_discovery(&path).is_some_and(|info| info.token == client.info().token) {
                if std::time::Instant::now() >= deadline {
                    eprintln!("The daemon did not stop within 60 s");
                    return Ok(1);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            eprintln!("Daemon stopped");
            Ok(0)
        }
    }
}

/// Run the request in a daemon started with `configuration`. None when no
/// such daemon answers; the caller then runs it in this process.
async fn forward_agent(
    request: Vec<u8>,
    configuration: &serde_json::Value,
    format: ReportFormat,
    output: Option<&Path>,
    cancellation: &CancellationToken,
) -> Option<u8> {
    use torrent_search::daemon::{DaemonClient, discovery_path};
    let client = DaemonClient::discover(&discovery_path()).await?;
    if client.info().configuration != *configuration
        || client.info().version != env!("CARGO_PKG_VERSION")
    {
        return None;
    }
    // Dropping the forwarded request closes the connection; the daemon cancels it.
    let reply = tokio::select! {
        reply = client.agent(request) => reply,
        _ = cancellation.cancelled() => {
            eprintln!("Cancelled");
            return Some(130);
        }
    };
    let parsed = reply.and_then(|body| {
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        Ok((body, value))
    });
    let (body, value) = match parsed {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{error}; running in this process");
            return None;
        }
    };
    let rendered = match format {
        ReportFormat::Json => Ok(String::from_utf8_lossy(&body).into_owned()),
        ReportFormat::Markdown => torrent_search::markdown::render(&value),
    };
    if let Err(error) = rendered.and_then(|rendered| write_report(&rendered, output)) {
        eprintln!("Cannot write report: {error}");
        return Some(1);
    }
    Some(torrent_search::daemon::exit_code(&value))
}

fn configured_sources(
    assets: &Path,
    python: &Path,
    backend: SourceBackend,
    lime_override: Option<LimeTorrentsBackend>,
) -> Result<Vec<Arc<dyn torrent_search::SearchSource>>, SearchError> {
    let sources = load_sources(assets, python)?;
    let lime_override = lime_override.map(|value| match value {
        LimeTorrentsBackend::Python => SourceBackend::Python,
        LimeTorrentsBackend::Rust => SourceBackend::Rust,
    });
    torrent_search::sources::native::configure(sources, assets, backend, lime_override)
}

#[allow(clippy::too_many_arguments)] // CLI options passed through unchanged.
async fn execute_agent(
    path: Option<&Path>,
    assets: &Path,
    python: &Path,
    backend: SourceBackend,
    lime_override: Option<LimeTorrentsBackend>,
    ffprobe: &Path,
    torrent_runtime: &torrent_search::torrent::TorrentRuntime,
    daemon: Option<&serde_json::Value>,
    format: ReportFormat,
    output: Option<&Path>,
) -> u8 {
    use torrent_search::agent::{AgentRequest, AgentResponse};
    let started = std::time::Instant::now();
    let read_request = || -> Result<(Vec<u8>, AgentRequest), SearchError> {
        let bytes = read_report(path)?;
        let request: AgentRequest = serde_json::from_slice(&bytes)?;
        request.validate()?;
        Ok((bytes, request))
    };
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let parsed = read_request();
    if let (Ok((bytes, _)), Some(configuration)) = (&parsed, daemon)
        && let Some(code) =
            forward_agent(bytes.clone(), configuration, format, output, &cancellation).await
    {
        interrupt.abort();
        return code;
    }
    let mut response = match parsed.map(|(_, request)| request) {
        Err(error) => AgentResponse::failure(error),
        Ok(request) => {
            let sources = if request.requires_sources() {
                configured_sources(assets, python, backend, lime_override)
            } else {
                Ok(vec![])
            };
            match sources {
                Err(error) => AgentResponse::failure(error),
                Ok(sources) => {
                    torrent_search::agent::execute_with_runtime(
                        request,
                        sources,
                        cancellation,
                        ffprobe,
                        torrent_runtime,
                    )
                    .await
                }
            }
        }
    };
    interrupt.abort();
    response.elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let rendered = match format {
        ReportFormat::Json => serde_json::to_string(&response).map_err(SearchError::from),
        ReportFormat::Markdown => serde_json::to_value(&response)
            .map_err(SearchError::from)
            .and_then(|value| torrent_search::markdown::render(&value)),
    };
    if let Err(error) = rendered.and_then(|rendered| write_report(&rendered, output)) {
        eprintln!("Cannot write report: {error}");
        return 1;
    }
    response.exit_code()
}

fn read_report(path: Option<&Path>) -> Result<Vec<u8>, SearchError> {
    let reader: Box<dyn Read> = match path {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None => Box::new(std::io::stdin()),
    };
    let mut bytes = Vec::new();
    reader
        .take(torrent_search::agent::MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > torrent_search::agent::MAX_REQUEST_BYTES {
        return Err(SearchError::InvalidRequest(
            "Report/request exceeds 16 MiB".into(),
        ));
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes.drain(..3);
    }
    Ok(bytes)
}

fn write_report(report: &str, path: Option<&Path>) -> Result<(), SearchError> {
    match path {
        Some(path) => {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(report.as_bytes())?;
            file.write_all(b"\n")?;
        }
        None => println!("{report}"),
    }
    Ok(())
}
