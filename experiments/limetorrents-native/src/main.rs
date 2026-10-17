use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode, time::Duration};
use tokio_util::sync::CancellationToken;
use torrent_search::{
    SearchEngine, SearchError, SearchLimits, SearchRequest, SourceStatus,
    sources::qbit::load_sources,
};

#[derive(Parser)]
#[command(
    version,
    about = "Local torrent search; no qBittorrent installation required"
)]
struct Cli {
    #[arg(long, global = true, value_enum, default_value_t = LimeTorrentsBackend::Python)]
    limetorrents_backend: LimeTorrentsBackend,
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

#[derive(Subcommand)]
enum Action {
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
    let mut sources = load_sources(&assets, &python)?;
    if matches!(cli.limetorrents_backend, LimeTorrentsBackend::Rust) {
        for source in &mut sources {
            if source.descriptor().id == "limetorrents" {
                *source = std::sync::Arc::new(
                    torrent_search::sources::limetorrents::LimeTorrentsSource::new(
                        source.descriptor().clone(),
                    )?,
                );
            }
        }
    }
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    let interrupt = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let result = match cli.command {
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
