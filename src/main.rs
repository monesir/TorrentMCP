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
    SearchEngine, SearchError, SearchLimits, SearchRequest, SourceStatus,
    sources::{native::SourceBackend, qbit::load_sources},
};

#[derive(Parser)]
#[command(
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
    if let Action::Agent {
        request,
        format,
        output,
    } = &cli.command
    {
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
        return Ok(execute_agent(
            request.as_deref(),
            &assets,
            &python,
            cli.source_backend,
            cli.limetorrents_backend,
            &ffprobe,
            &torrent_runtime,
            *format,
            output.as_deref(),
        )
        .await);
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
        Action::Agent { .. } | Action::ExportMarkdown { .. } => {
            unreachable!("Reports are handled before loading sources")
        }
        Action::Sources => {
            let descriptors: Vec<_> = sources.iter().map(|source| source.descriptor()).collect();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&descriptors)?);
            } else {
                for descriptor in descriptors {
                    println!(
                        "{}\t{}\t{}",
                        descriptor.id,
                        descriptor.name,
                        descriptor.categories.join(",")
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
            } else if !report
                .sources
                .iter()
                .any(|source| source.status == SourceStatus::Completed)
            {
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

async fn execute_agent(
    path: Option<&Path>,
    assets: &Path,
    python: &Path,
    backend: SourceBackend,
    lime_override: Option<LimeTorrentsBackend>,
    ffprobe: &Path,
    torrent_runtime: &torrent_search::torrent::TorrentRuntime,
    format: ReportFormat,
    output: Option<&Path>,
) -> u8 {
    use torrent_search::agent::{AgentRequest, AgentResponse};
    let started = std::time::Instant::now();
    let read_request = || -> Result<AgentRequest, SearchError> {
        let bytes = read_report(path)?;
        let request: AgentRequest = serde_json::from_slice(&bytes)?;
        request.validate()?;
        Ok(request)
    };
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let mut response = match read_request() {
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
