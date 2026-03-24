//! Canonical string ↔ proto enum conversions.
//!
//! These are the **single source of truth** for mapping between the lowercase
//! string identifiers used in the database / domain layer and the proto enum
//! variants used on the wire. All crates that need these conversions should
//! call these functions rather than rolling their own match arms.

use crate::canonical::prism::v1::{ContributionState, ContributionType, Platform};

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

impl Platform {
    /// Parse a database platform string (e.g. `"github"`, `"discourse-ubuntu"`)
    /// into a `(Platform, Option<instance>)` pair.
    ///
    /// Multi-instance platforms like Discourse store the instance in the string
    /// as `"discourse-{instance}"`.  Single-instance platforms are bare names.
    pub fn from_db_str(s: &str) -> (Self, Option<String>) {
        if let Some(instance) = s.strip_prefix("discourse-") {
            return (Self::Discourse, Some(instance.to_string()));
        }
        let platform = match s {
            "github" => Self::Github,
            "jira" => Self::Jira,
            "discourse" => Self::Discourse,
            "launchpad" => Self::Launchpad,
            "mattermost" => Self::Mattermost,
            "google_drive" => Self::GoogleDrive,
            "mailing_list" => Self::MailingList,
            _ => Self::Unspecified,
        };
        (platform, None)
    }

    /// Parse a user-facing / API string (case-insensitive) into a `Platform`.
    ///
    /// Use this for CLI arguments, MCP tool inputs, and search filters where
    /// the caller may pass `"GitHub"` or `"JIRA"`.
    pub fn from_user_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "github" => Self::Github,
            "jira" => Self::Jira,
            "discourse" => Self::Discourse,
            "launchpad" => Self::Launchpad,
            "mattermost" => Self::Mattermost,
            "google_drive" | "googledrive" => Self::GoogleDrive,
            "mailing_list" | "mailinglist" => Self::MailingList,
            _ => Self::Unspecified,
        }
    }

    /// Convert a proto `Platform` + optional instance back to a database string.
    ///
    /// Returns `None` for `Unspecified`.
    pub fn to_db_str(self, instance: Option<&str>) -> Option<String> {
        match self {
            Self::Github => Some("github".to_string()),
            Self::Jira => Some("jira".to_string()),
            Self::Launchpad => Some("launchpad".to_string()),
            Self::Mattermost => Some("mattermost".to_string()),
            Self::GoogleDrive => Some("google_drive".to_string()),
            Self::MailingList => Some("mailing_list".to_string()),
            Self::Discourse => match instance {
                Some(inst) => Some(format!("discourse-{inst}")),
                None => Some("discourse".to_string()),
            },
            Self::Unspecified => None,
        }
    }

    /// Short display name for CLI / UI output.
    pub fn display_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Jira => "jira",
            Self::Discourse => "discourse",
            Self::Launchpad => "launchpad",
            Self::Mattermost => "mattermost",
            Self::GoogleDrive => "google_drive",
            Self::MailingList => "mailing_list",
            Self::Unspecified => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// ContributionType
// ---------------------------------------------------------------------------

impl ContributionType {
    /// Parse a database contribution type string into a `ContributionType`.
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "pull_request" => Self::PullRequest,
            "pr_review" => Self::PrReview,
            "jira_ticket" => Self::JiraTicket,
            "discourse_topic" => Self::DiscourseTopic,
            "discourse_post" => Self::DiscoursePost,
            "discourse_like" => Self::DiscourseLike,
            _ => Self::Unspecified,
        }
    }

    /// Parse a user-facing string (case-insensitive, with aliases) into a
    /// `ContributionType`.
    pub fn from_user_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "pull_request" | "pr" => Self::PullRequest,
            "pr_review" | "review" => Self::PrReview,
            "jira_ticket" | "ticket" => Self::JiraTicket,
            "discourse_topic" | "topic" => Self::DiscourseTopic,
            "discourse_post" | "post" => Self::DiscoursePost,
            "discourse_like" | "like" => Self::DiscourseLike,
            _ => Self::Unspecified,
        }
    }

    /// Convert to the canonical database string. Returns `None` for
    /// `Unspecified`.
    pub fn to_db_str(self) -> Option<&'static str> {
        match self {
            Self::PullRequest => Some("pull_request"),
            Self::PrReview => Some("pr_review"),
            Self::JiraTicket => Some("jira_ticket"),
            Self::DiscourseTopic => Some("discourse_topic"),
            Self::DiscoursePost => Some("discourse_post"),
            Self::DiscourseLike => Some("discourse_like"),
            Self::Unspecified => None,
        }
    }

    /// Short display name for CLI / UI output.
    pub fn display_str(self) -> &'static str {
        self.to_db_str().unwrap_or("unknown")
    }
}

// ---------------------------------------------------------------------------
// ContributionState
// ---------------------------------------------------------------------------

impl ContributionState {
    /// Parse a database state string into a `ContributionState`.
    ///
    /// The database stores states in mixed case: lowercase for lifecycle states
    /// (`"open"`, `"merged"`) and UPPER_CASE for GitHub review verdicts
    /// (`"APPROVED"`, `"CHANGES_REQUESTED"`).
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "open" => Self::Open,
            "closed" => Self::Closed,
            "merged" => Self::Merged,
            "in_progress" => Self::InProgress,
            "done" => Self::Done,
            "APPROVED" => Self::Approved,
            "CHANGES_REQUESTED" => Self::ChangesRequested,
            "COMMENTED" => Self::Commented,
            "PENDING" => Self::Pending,
            "DISMISSED" => Self::Dismissed,
            _ => Self::Unspecified,
        }
    }

    /// Parse a user-facing string (case-insensitive) into a `ContributionState`.
    pub fn from_user_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "open" => Self::Open,
            "closed" => Self::Closed,
            "merged" => Self::Merged,
            "in_progress" => Self::InProgress,
            "done" => Self::Done,
            "approved" => Self::Approved,
            "changes_requested" => Self::ChangesRequested,
            "commented" => Self::Commented,
            "pending" => Self::Pending,
            "dismissed" => Self::Dismissed,
            _ => Self::Unspecified,
        }
    }

    /// Convert to the canonical database string. Returns `None` for
    /// `Unspecified`.
    pub fn to_db_str(self) -> Option<&'static str> {
        match self {
            Self::Open => Some("open"),
            Self::Closed => Some("closed"),
            Self::Merged => Some("merged"),
            Self::InProgress => Some("in_progress"),
            Self::Done => Some("done"),
            Self::Approved => Some("APPROVED"),
            Self::ChangesRequested => Some("CHANGES_REQUESTED"),
            Self::Commented => Some("COMMENTED"),
            Self::Pending => Some("PENDING"),
            Self::Dismissed => Some("DISMISSED"),
            Self::Unspecified => None,
        }
    }

    /// Short display name for CLI / UI output.
    pub fn display_str(self) -> &'static str {
        self.to_db_str().unwrap_or("\u{2014}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Platform --

    #[test]
    fn platform_from_db_str_simple() {
        assert_eq!(Platform::from_db_str("github"), (Platform::Github, None));
        assert_eq!(Platform::from_db_str("jira"), (Platform::Jira, None));
        assert_eq!(
            Platform::from_db_str("google_drive"),
            (Platform::GoogleDrive, None)
        );
    }

    #[test]
    fn platform_from_db_str_discourse_instance() {
        let (p, inst) = Platform::from_db_str("discourse-ubuntu");
        assert_eq!(p, Platform::Discourse);
        assert_eq!(inst.as_deref(), Some("ubuntu"));
    }

    #[test]
    fn platform_from_db_str_bare_discourse() {
        assert_eq!(
            Platform::from_db_str("discourse"),
            (Platform::Discourse, None)
        );
    }

    #[test]
    fn platform_from_user_str_case_insensitive() {
        assert_eq!(Platform::from_user_str("GitHub"), Platform::Github);
        assert_eq!(Platform::from_user_str("JIRA"), Platform::Jira);
        assert_eq!(Platform::from_user_str("Discourse"), Platform::Discourse);
        assert_eq!(Platform::from_user_str("bogus"), Platform::Unspecified);
    }

    #[test]
    fn platform_to_db_str_roundtrip() {
        assert_eq!(Platform::Github.to_db_str(None), Some("github".to_string()));
        assert_eq!(
            Platform::Discourse.to_db_str(Some("ubuntu")),
            Some("discourse-ubuntu".to_string())
        );
        assert_eq!(
            Platform::Discourse.to_db_str(None),
            Some("discourse".to_string())
        );
        assert_eq!(Platform::Unspecified.to_db_str(None), None);
    }

    // -- ContributionType --

    #[test]
    fn contribution_type_from_db_str_all_variants() {
        assert_eq!(
            ContributionType::from_db_str("pull_request"),
            ContributionType::PullRequest
        );
        assert_eq!(
            ContributionType::from_db_str("discourse_like"),
            ContributionType::DiscourseLike
        );
        assert_eq!(
            ContributionType::from_db_str("bogus"),
            ContributionType::Unspecified
        );
    }

    #[test]
    fn contribution_type_from_user_str_aliases() {
        assert_eq!(
            ContributionType::from_user_str("review"),
            ContributionType::PrReview
        );
        assert_eq!(
            ContributionType::from_user_str("PR"),
            ContributionType::PullRequest
        );
        assert_eq!(
            ContributionType::from_user_str("DISCOURSE_LIKE"),
            ContributionType::DiscourseLike
        );
    }

    #[test]
    fn contribution_type_roundtrip() {
        for ct in [
            ContributionType::PullRequest,
            ContributionType::PrReview,
            ContributionType::JiraTicket,
            ContributionType::DiscourseTopic,
            ContributionType::DiscoursePost,
            ContributionType::DiscourseLike,
        ] {
            let s = ct.to_db_str().unwrap();
            assert_eq!(ContributionType::from_db_str(s), ct);
        }
    }

    // -- ContributionState --

    #[test]
    fn contribution_state_from_db_str_all_variants() {
        assert_eq!(
            ContributionState::from_db_str("open"),
            ContributionState::Open
        );
        assert_eq!(
            ContributionState::from_db_str("APPROVED"),
            ContributionState::Approved
        );
        assert_eq!(
            ContributionState::from_db_str("done"),
            ContributionState::Done
        );
        assert_eq!(
            ContributionState::from_db_str("bogus"),
            ContributionState::Unspecified
        );
    }

    #[test]
    fn contribution_state_from_user_str_case_insensitive() {
        assert_eq!(
            ContributionState::from_user_str("MERGED"),
            ContributionState::Merged
        );
        assert_eq!(
            ContributionState::from_user_str("changes_requested"),
            ContributionState::ChangesRequested
        );
    }

    #[test]
    fn contribution_state_roundtrip() {
        for cs in [
            ContributionState::Open,
            ContributionState::Closed,
            ContributionState::Merged,
            ContributionState::InProgress,
            ContributionState::Done,
            ContributionState::Approved,
            ContributionState::ChangesRequested,
            ContributionState::Commented,
            ContributionState::Pending,
            ContributionState::Dismissed,
        ] {
            let s = cs.to_db_str().unwrap();
            assert_eq!(ContributionState::from_db_str(s), cs);
        }
    }
}
