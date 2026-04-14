pub mod discourse;
pub mod github;
pub mod jira;
pub mod lib;

use restate_sdk::endpoint::Builder;

use crate::infra::SharedState;

/// Bind all ingestion handlers to the Restate endpoint.
pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    use discourse::handler::{DiscourseIngestionHandler, DiscourseIngestionHandlerImpl};
    use github::handler::{GithubIngestionHandler, GithubIngestionHandlerImpl};
    use github::team_sync::{GithubTeamSyncHandler, GithubTeamSyncHandlerImpl};
    use jira::handler::{JiraIngestionHandler, JiraIngestionHandlerImpl};
    use lib::chunk::{IngestionChunkService, IngestionChunkServiceImpl};

    let github = GithubIngestionHandlerImpl {
        state: state.clone(),
    };
    let team_sync = GithubTeamSyncHandlerImpl {
        state: state.clone(),
    };
    let jira = JiraIngestionHandlerImpl {
        state: state.clone(),
    };
    let discourse = DiscourseIngestionHandlerImpl {
        state: state.clone(),
    };
    let chunk = IngestionChunkServiceImpl {
        state: state.clone(),
    };

    endpoint
        .bind(github.serve())
        .bind(team_sync.serve())
        .bind(jira.serve())
        .bind(discourse.serve())
        .bind(chunk.serve())
}
