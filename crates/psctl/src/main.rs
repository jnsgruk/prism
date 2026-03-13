#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::Result;
use clap::{Parser, Subcommand};

mod client;
mod commands;
mod format;

#[derive(Parser)]
#[command(name = "psctl", about = "Prism CLI client", version)]
struct Cli {
    /// Server URL
    #[arg(long, env = "PS_SERVER_URL", default_value = "http://localhost:18080")]
    server: String,

    /// API token for authentication
    #[arg(long, env = "PS_API_TOKEN")]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show ingestion status for all sources
    Status,

    /// List configured data sources
    Sources,

    /// List recent ingestion runs
    Runs {
        /// Filter by source name
        source: Option<String>,
    },

    /// Trigger an ingestion run for a source
    Trigger {
        /// Source name
        source: String,
    },

    /// Trigger a backfill from a given date
    Backfill {
        /// Source name
        source: String,

        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        since: String,
    },

    /// Download a backup file
    Backup {
        /// Output file path (default: prism-backup-{date}.ps-backup)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Restore from a backup file into a fresh instance
    Restore {
        /// Path to .ps-backup file
        file: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env before clap parses args so env vars are available
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let (channel, auth) = client::connect(&cli.server, cli.token.as_ref())?;

    match cli.command {
        Command::Status => commands::status(&channel, &auth).await,
        Command::Sources => commands::sources(&channel, &auth).await,
        Command::Runs { source } => commands::runs(&channel, &auth, source).await,
        Command::Trigger { source } => commands::trigger(&channel, &auth, &source).await,
        Command::Backfill { source, since } => {
            commands::backfill(&channel, &auth, &source, &since).await
        }
        Command::Backup { output } => commands::backup(&channel, &auth, output).await,
        Command::Restore { file } => commands::restore(&channel, &auth, &file).await,
    }
}
