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
    /// Install and start as an OS service (needs root / Administrator)
    Install,
    /// Stop and remove the OS service
    Uninstall,
    /// Start the installed service
    Start,
    /// Stop the installed service
    Stop,
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
        Cmd::Install => ziewer::service::install(&cli.config).map(|()| println!("ziewer service installed and started")),
        Cmd::Uninstall => ziewer::service::uninstall().map(|()| println!("ziewer service removed")),
        Cmd::Start => ziewer::service::start(),
        Cmd::Stop => ziewer::service::stop(),
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
