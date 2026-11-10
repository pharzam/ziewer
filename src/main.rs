use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use ziewer::config::ConfigStore;

#[derive(Parser)]
#[command(name = "ziewer", version, about = "GitHub-style Markdown/HTML folder viewer with IP allow-listing")]
struct Cli {
    /// Path to the TOML config file
    #[arg(long, env = "ZIEWER_CONFIG", default_value = "ziewer.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the web server in the foreground
    Serve,
}

fn main() -> ExitCode {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Serve => serve(cli.config),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn serve(config: PathBuf) -> anyhow::Result<()> {
    let store = ConfigStore::load(&config)?;
    let state = ziewer::web::build_state(store)?;
    tokio::runtime::Runtime::new()?.block_on(ziewer::web::serve(state))?;
    Ok(())
}
