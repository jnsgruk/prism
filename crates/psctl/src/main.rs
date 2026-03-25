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

    /// API token for authentication.
    /// Prefer setting `PS_API_TOKEN` env var over passing on the command line
    /// (CLI args are visible in process listings).
    #[arg(long, env = "PS_API_TOKEN")]
    token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ask a natural-language question about engineering data
    Ask {
        /// The question to ask
        question: String,

        /// Output structured JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },

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

    /// Show team metrics (flow, DORA, review turnaround)
    Metrics {
        /// Team name or ID
        team: String,

        /// Period type: week, month, or quarter
        #[arg(long, default_value = "month")]
        period: String,
    },

    /// List people in the organisation
    People {
        /// Filter by team name or ID
        #[arg(long)]
        team: Option<String>,

        /// Show only unresolved/unassigned identities
        #[arg(long)]
        unresolved: bool,
    },

    /// List contributions for a person
    Contributions {
        /// Person ID (UUID)
        #[arg(long)]
        person: String,

        /// Filter by platform (e.g. github, jira)
        #[arg(long)]
        platform: Option<String>,

        /// Only contributions since this date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
    },

    /// Show embedding pipeline status
    EmbedStatus,

    /// Find similar contributions to a given contribution
    Similar {
        /// Contribution ID (UUID)
        contribution_id: String,

        /// Max results (default 10)
        #[arg(long, default_value = "10")]
        limit: i32,

        /// Filter by platform
        #[arg(long)]
        platform: Option<String>,
    },

    /// Free-text similarity search across all contributions
    Search {
        /// Query text
        query: String,

        /// Max results (default 10)
        #[arg(long, default_value = "10")]
        limit: i32,

        /// Filter by platform
        #[arg(long)]
        platform: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env before clap parses args so env vars are available
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let mut clients = client::connect(&cli.server, cli.token.as_ref())?;

    match cli.command {
        Command::Ask { question, json } => commands::ask(&mut clients, &question, json).await,
        Command::Status => commands::status(&mut clients).await,
        Command::Sources => commands::sources(&mut clients).await,
        Command::Runs { source } => commands::runs(&mut clients, source).await,
        Command::Trigger { source } => commands::trigger(&mut clients, &source).await,
        Command::Backfill { source, since } => {
            commands::backfill(&mut clients, &source, &since).await
        }
        Command::Backup { output } => commands::backup(&mut clients, output).await,
        Command::Restore { file } => commands::restore(&mut clients, &file).await,
        Command::Metrics { team, period } => commands::metrics(&mut clients, &team, &period).await,
        Command::People { team, unresolved } => {
            commands::people(&mut clients, team.as_deref(), unresolved).await
        }
        Command::Contributions {
            person,
            platform,
            since,
        } => {
            commands::contributions(&mut clients, &person, platform.as_deref(), since.as_deref())
                .await
        }
        Command::EmbedStatus => commands::embed_status(&mut clients).await,
        Command::Similar {
            contribution_id,
            limit,
            platform,
        } => commands::similar(&mut clients, &contribution_id, limit, platform.as_deref()).await,
        Command::Search {
            query,
            limit,
            platform,
        } => commands::search(&mut clients, &query, limit, platform.as_deref()).await,
    }
}
