use std::collections::HashSet;

use super::{ContributionInput, SourceRunContext};
use crate::{
    Error,
    models::{ContributionType, Platform},
};

impl SourceRunContext {
    /// Repeat adapter checks at the write boundary; context actors never
    /// become eligible simply because their PR/topic was fetched.
    pub fn eligible_contribution(&self, item: &ContributionInput) -> Result<bool, Error> {
        let identity = self
            .source
            .identity
            .as_ref()
            .ok_or_else(|| Error::Validation("person identity is required".into()))?;
        if item.platform != self.source.platform {
            return Ok(false);
        }
        let actor_matches = match self.source.platform {
            Platform::Jira => {
                identity.platform_user_id.as_deref() == Some(item.platform_username.as_str())
            }
            _ => identity
                .username
                .eq_ignore_ascii_case(&item.platform_username),
        };
        let event_time = match self.source.platform {
            Platform::Jira => item.updated_at.unwrap_or(item.created_at),
            _ => item.created_at,
        };
        let type_matches = match self.source.platform {
            Platform::Github => matches!(
                item.contribution_type,
                ContributionType::PullRequest | ContributionType::PrReview
            ),
            Platform::Jira => item.contribution_type == ContributionType::JiraTicket,
            Platform::Discourse(_) => matches!(
                item.contribution_type,
                ContributionType::DiscourseTopic
                    | ContributionType::DiscoursePost
                    | ContributionType::DiscourseLike
            ),
            _ => false,
        };
        Ok(actor_matches && type_matches && self.contains_event(event_time)?)
    }

    pub fn eligible_batch<'a>(
        &self,
        items: &'a [ContributionInput],
    ) -> Result<Vec<&'a ContributionInput>, Error> {
        let mut seen = HashSet::new();
        let mut eligible = Vec::new();
        // Last occurrence wins when an upstream page contains overlapping data.
        for item in items.iter().rev() {
            if self.eligible_contribution(item)? && seen.insert(item.key()) {
                eligible.push(item);
            }
        }
        eligible.reverse();
        Ok(eligible)
    }
}
