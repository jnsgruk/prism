use super::super::types::GraphQLSearchPr;
use super::parse_datetime;
use ps_core::ingestion::{ContributionInput, ContributionMetadata, ContributionMetrics};
use ps_core::models::{ContributionState, ContributionType, Platform};

/// Convert a GraphQL search PR to `ContributionInputs`.
pub(super) fn search_pr_to_contributions(
    owner: &str,
    repo: &str,
    pr: &GraphQLSearchPr,
) -> Result<Vec<ContributionInput>, ps_core::Error> {
    let number = pr.number.unwrap_or(0);
    let title = pr.title.clone().unwrap_or_default();
    let url = pr.url.clone().unwrap_or_default();
    let created_at_str = pr.created_at.as_deref().unwrap_or("1970-01-01T00:00:00Z");
    let updated_at_str = pr.updated_at.as_deref().unwrap_or("1970-01-01T00:00:00Z");
    let author = pr.author.as_ref().map_or("", |a| a.login.as_str());

    let pr_state = if pr.merged_at.is_some() {
        ContributionState::Merged
    } else {
        match pr.state.as_deref().unwrap_or("OPEN") {
            "CLOSED" => ContributionState::Closed,
            "MERGED" => ContributionState::Merged,
            _ => ContributionState::Open,
        }
    };

    let mut items = Vec::new();

    let mut state_history = vec![serde_json::json!({
        "state": ContributionState::Open.as_str(),
        "at": created_at_str,
    })];
    if let Some(ref closed_at) = pr.closed_at {
        state_history.push(serde_json::json!({
            "state": pr_state.as_str(),
            "at": closed_at,
        }));
    }

    let review_count = pr.reviews.as_ref().map_or(0, |r| {
        r.total_count.map_or(r.nodes.len(), |count| count as usize)
    });
    let labels: Vec<&str> = pr
        .labels
        .as_ref()
        .map(|l| l.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    // Build enrichment content blob for this PR.
    // Diff will be attached later by fetch_pr_diffs().
    // Only PRs with >50 lines changed are eligible for significance enrichment
    // (the only enrichment type targeting pull_requests), so skip small PRs.
    let lines_changed = pr.additions.unwrap_or(0) + pr.deletions.unwrap_or(0);
    let pr_enrichment = if lines_changed > 50 {
        Some(serde_json::json!({
            "title": &title,
            "description": pr.body_text.as_deref().unwrap_or(""),
            "labels": labels,
            "additions": pr.additions.unwrap_or(0),
            "deletions": pr.deletions.unwrap_or(0),
            "changed_files": pr.changed_files.unwrap_or(0),
            "draft": pr.is_draft.unwrap_or(false),
        }))
    } else {
        None
    };

    // Clone title before moving it — reviews need pr_title.
    let pr_title_for_reviews = title.clone();

    items.push(ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PullRequest,
        platform_id: format!("{owner}/{repo}/pull/{number}").into(),
        platform_username: author.to_lowercase().into(),
        title: Some(title),
        url: Some(url.clone()),
        state: Some(pr_state),
        created_at: parse_datetime(created_at_str)?,
        updated_at: Some(parse_datetime(updated_at_str)?),
        closed_at: pr.closed_at.as_deref().map(parse_datetime).transpose()?,
        #[allow(clippy::cast_possible_wrap)]
        metrics: serde_json::to_value(ContributionMetrics {
            additions: pr.additions.map(|v| v as i32),
            deletions: pr.deletions.map(|v| v as i32),
            changed_files: pr.changed_files.map(|v| v as i32),
            review_count: Some(review_count as i32),
            draft: Some(pr.is_draft.unwrap_or(false)),
            ..Default::default()
        })
        .unwrap_or_default(),
        metadata: serde_json::to_value(ContributionMetadata {
            repo: Some(format!("{owner}/{repo}")),
            head_ref: pr.head_ref_name.clone(),
            base_ref: pr.base_ref_name.clone(),
            labels: if labels.is_empty() {
                None
            } else {
                Some(labels.into_iter().map(String::from).collect())
            },
            ..Default::default()
        })
        .unwrap_or_default(),
        content: None,
        state_history: Some(serde_json::Value::Array(state_history)),
        enrichment_content: pr_enrichment,
    });

    // Map reviews.
    if let Some(reviews) = &pr.reviews {
        for review in &reviews.nodes {
            if !is_submitted_review(review) {
                continue;
            }
            items.push(search_review_to_contribution(
                owner,
                repo,
                number,
                &url,
                &pr_title_for_reviews,
                created_at_str,
                review,
            )?);
        }
    }

    Ok(items)
}

/// Pending and deleted reviews lack stable submitted activity attribution.
pub(super) fn is_submitted_review(review: &super::super::types::GraphQLReview) -> bool {
    review.database_id.is_some_and(|id| id > 0)
        && review
            .author
            .as_ref()
            .is_some_and(|actor| !actor.login.is_empty())
        && review.submitted_at.is_some()
        && ContributionState::from_str_opt(&review.state)
            .is_some_and(|state| state != ContributionState::Pending)
}

/// Convert a single GraphQL review into a `ContributionInput`.
pub(super) fn search_review_to_contribution(
    owner: &str,
    repo: &str,
    pr_number: u32,
    pr_url: &str,
    pr_title: &str,
    pr_created_at: &str,
    review: &super::super::types::GraphQLReview,
) -> Result<ContributionInput, ps_core::Error> {
    let reviewer = review.author.as_ref().map_or("", |a| a.login.as_str());

    let submitted_at = review
        .submitted_at
        .as_deref()
        .map(parse_datetime)
        .transpose()?;

    let review_id = review.database_id.unwrap_or(0);
    let review_state = ContributionState::from_str_opt(&review.state);

    let inline_comments: Vec<serde_json::Value> = review
        .comments
        .as_ref()
        .map(|c| {
            c.nodes
                .iter()
                .filter_map(|comment| {
                    let body = comment.body.as_deref().unwrap_or("");
                    if body.is_empty() {
                        return None;
                    }
                    Some(serde_json::json!({
                        "path": comment.path.as_deref().unwrap_or(""),
                        "body": body,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let review_body = review.body.as_deref().unwrap_or("");
    let review_enrichment = if !review_body.is_empty() || !inline_comments.is_empty() {
        Some(serde_json::json!({
            "pr_title": pr_title,
            "pr_number": pr_number,
            "state": review.state,
            "body": review_body,
            "inline_comments": inline_comments,
            "inline_comments_truncated": review.comments.as_ref().is_some_and(|comments| comments.page_info.as_ref().is_none_or(|page| page.has_next_page)),
        }))
    } else {
        None
    };

    Ok(ContributionInput {
        platform: Platform::Github,
        contribution_type: ContributionType::PrReview,
        platform_id: format!("{owner}/{repo}/review/{review_id}").into(),
        platform_username: reviewer.to_lowercase().into(),
        title: Some(format!("Review on #{pr_number}")),
        url: Some(format!("{pr_url}#pullrequestreview-{review_id}")),
        state: review_state,
        created_at: submitted_at.unwrap_or(parse_datetime(pr_created_at)?),
        updated_at: submitted_at,
        closed_at: None,
        metrics: serde_json::to_value(ContributionMetrics {
            review_state: Some(review.state.clone()),
            ..Default::default()
        })
        .unwrap_or_default(),
        metadata: serde_json::to_value(ContributionMetadata {
            repo: Some(format!("{owner}/{repo}")),
            pr_number: Some(pr_number),
            pr_platform_id: Some(format!("{owner}/{repo}/pull/{pr_number}")),
            ..Default::default()
        })
        .unwrap_or_default(),
        content: review.body.clone(),
        state_history: None,
        enrichment_content: review_enrichment,
    })
}
