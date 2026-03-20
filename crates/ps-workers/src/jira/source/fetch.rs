use ps_core::ingestion::{ContributionInput, FailedItem, FetchResult, IngestionContext};
use ps_core::models::{ContributionState, ContributionType, JiraTicketData, Platform};
use tracing::{debug, warn};

use super::{
    Cursor, MAX_RESULTS_PER_PAGE, decrypt_email, decrypt_token, parse_jira_datetime,
    serialise_cursor,
};
use crate::jira::client::{JiraChangeHistory, JiraIssue};
use crate::retry::retry_transient;

pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    let token = decrypt_token(ctx)?;

    // For Cloud auth, we need the email for Basic auth.
    let email = decrypt_email(ctx);

    let settings = &ctx.source_config.settings;
    let base_url = settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://jira.atlassian.net");
    let api_mode = settings
        .get("api_mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("cloud");

    let client = crate::jira::client::JiraClient::new(
        ctx.http_client.clone(),
        base_url,
        api_mode,
        email.as_deref(),
        &token,
    );

    // Build single-project JQL when iterating per-project
    let current_project = if cur.projects.is_empty() {
        None
    } else {
        let Some(proj) = cur.projects.get(cur.project_index) else {
            // All projects exhausted — done
            let final_cursor = serialise_cursor(&cur)?;
            return Ok(FetchResult {
                items: vec![],
                next_cursor: None,
                rate_limit: None,
                etag: Some(final_cursor),
                skipped_diffs: vec![],
            });
        };
        Some(proj.clone())
    };

    let project_filter = current_project
        .as_ref()
        .map(|p| format!("project = \"{}\"", p.replace('"', "\\\"")))
        .unwrap_or_default();

    let jql = match (project_filter.is_empty(), &cur.watermark) {
        (false, Some(wm)) => {
            let jira_date = format_watermark_for_jql(wm);
            format!("{project_filter} AND updated >= \"{jira_date}\" ORDER BY updated ASC")
        }
        (false, None) => format!("{project_filter} ORDER BY updated ASC"),
        (true, Some(wm)) => {
            let jira_date = format_watermark_for_jql(wm);
            format!("updated >= \"{jira_date}\" ORDER BY updated ASC")
        }
        (true, None) => "ORDER BY updated ASC".into(),
    };

    let fields = "summary,description,status,issuetype,priority,labels,assignee,reporter,created,updated,resolutiondate,parent";
    let next_page_token = cur.next_page_token.clone();
    let (response, rate_limit) = match retry_transient(
        current_project.as_deref().unwrap_or("jira search"),
        ps_core::Error::is_transient,
        || {
            client.search(
                &jql,
                MAX_RESULTS_PER_PAGE,
                fields,
                "changelog",
                next_page_token.as_deref(),
            )
        },
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            if let Some(ref proj) = current_project {
                warn!(
                    source = ctx.source_config.name,
                    project = proj,
                    error = %e,
                    "skipping project due to fetch error"
                );
                cur.failed_items.push(FailedItem {
                    key: proj.clone(),
                    error: e.to_string(),
                });
                cur.project_index += 1;
                cur.next_page_token = None;
                let final_cursor = serialise_cursor(&cur)?;
                let next_cursor = if cur.project_index < cur.projects.len() {
                    Some(serialise_cursor(&cur)?)
                } else {
                    None
                };
                return Ok(FetchResult {
                    items: vec![],
                    next_cursor,
                    rate_limit: None,
                    etag: Some(final_cursor),
                    skipped_diffs: vec![],
                });
            }
            return Err(e);
        }
    };

    let returned = response.issues.len();

    debug!(
        returned,
        project = ?current_project,
        is_last = ?response.is_last,
        "fetched Jira issues"
    );

    // Convert issues to ContributionInput
    let items: Vec<ContributionInput> = response
        .issues
        .into_iter()
        .filter_map(|issue| convert_issue(&cur, &issue))
        .collect();

    // Track max updated_at for watermark advancement
    for item in &items {
        if let Some(ref updated) = item.updated_at {
            let updated_str = updated
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            if cur
                .max_updated_at
                .as_ref()
                .is_none_or(|current| updated_str > *current)
            {
                cur.max_updated_at = Some(updated_str);
            }
        }
    }

    // Determine if there are more pages using cursor-based pagination
    let has_more = response
        .is_last
        .map_or(response.next_page_token.is_some(), |last| !last);

    // Always serialize the current cursor state so the handler can extract
    // max_updated_at for watermark advancement, even on the final page.
    let final_cursor = serialise_cursor(&cur)?;

    let next_cursor = if has_more {
        cur.next_page_token = response.next_page_token;
        Some(serialise_cursor(&cur)?)
    } else if !cur.projects.is_empty() && cur.project_index + 1 < cur.projects.len() {
        // Move to next project
        cur.project_index += 1;
        cur.next_page_token = None;
        Some(serialise_cursor(&cur)?)
    } else {
        None
    };

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit,
        etag: Some(final_cursor),
        skipped_diffs: vec![],
    })
}

/// Convert a Jira issue into a `ContributionInput`.
fn convert_issue(cur: &Cursor, issue: &JiraIssue) -> Option<ContributionInput> {
    let fields = &issue.fields;

    // Determine state from status category
    let state = fields
        .status
        .as_ref()
        .and_then(|s| s.status_category.as_ref())
        .and_then(|sc| sc.key.as_deref())
        .and_then(map_status_category);

    // Extract assignee account ID for identity resolution
    let platform_username = fields
        .assignee
        .as_ref()
        .and_then(|a| a.account_id.as_deref().or(a.name.as_deref()))
        .unwrap_or("")
        .to_string();

    // Parse timestamps
    let created_at = fields
        .created
        .as_deref()
        .and_then(|s| parse_jira_datetime(s).ok())?;
    let updated_at = fields
        .updated
        .as_deref()
        .and_then(|s| parse_jira_datetime(s).ok());
    let closed_at = fields
        .resolution_date
        .as_deref()
        .and_then(|s| parse_jira_datetime(s).ok());

    // Build state history from changelog
    let state_history = issue
        .changelog
        .as_ref()
        .map(|cl| build_state_history(&cl.histories));

    // Extract story points from the configured custom field
    let story_points = cur
        .story_points_field
        .as_ref()
        .and_then(|field| fields.extra.get(field))
        .and_then(serde_json::Value::as_f64);

    // Compute cycle time from state history
    let cycle_time_hours = compute_cycle_time(state_history.as_ref());

    // Build typed metrics
    let metrics_data = JiraTicketData {
        issue_type: fields.issuetype.as_ref().and_then(|t| t.name.clone()),
        story_points,
        cycle_time_hours,
        priority: fields.priority.as_ref().and_then(|p| p.name.clone()),
        labels: fields.labels.clone().unwrap_or_default(),
        parent_key: fields.parent.as_ref().and_then(|p| p.key.clone()),
    };

    let metrics = serde_json::to_value(&metrics_data).unwrap_or_default();

    // Build metadata with display name for unresolved identity debugging
    let mut metadata = serde_json::Map::new();
    if let Some(ref assignee) = fields.assignee {
        if let Some(ref name) = assignee.display_name {
            metadata.insert("assignee_display_name".into(), serde_json::json!(name));
        }
        if let Some(ref account_id) = assignee.account_id {
            metadata.insert("assignee_account_id".into(), serde_json::json!(account_id));
        }
    }
    if let Some(ref reporter) = fields.reporter
        && let Some(ref name) = reporter.display_name
    {
        metadata.insert("reporter_display_name".into(), serde_json::json!(name));
    }

    // Issue URL
    let url = format!("{}/browse/{}", cur.base_url, issue.key);

    Some(ContributionInput {
        platform: Platform::Jira,
        contribution_type: ContributionType::JiraTicket,
        platform_id: issue.key.clone(),
        platform_username,
        title: fields.summary.clone(),
        url: Some(url),
        state,
        created_at,
        updated_at,
        closed_at,
        metrics,
        metadata: serde_json::Value::Object(metadata),
        content: None,
        state_history,
        // No enrichment types target jira_ticket yet — don't enqueue.
        enrichment_content: None,
    })
}

/// Map Jira status category key to `ContributionState`.
fn map_status_category(category_key: &str) -> Option<ContributionState> {
    match category_key {
        "new" => Some(ContributionState::Open),
        "indeterminate" => Some(ContributionState::InProgress),
        "done" => Some(ContributionState::Closed),
        _ => {
            debug!(category = category_key, "unknown Jira status category");
            None
        }
    }
}

/// Build a state history JSON array from Jira changelog histories.
fn build_state_history(histories: &[JiraChangeHistory]) -> serde_json::Value {
    let mut transitions = Vec::new();

    for history in histories {
        for item in &history.items {
            if item.field.as_deref() == Some("status")
                && let (Some(to), Some(created)) = (&item.to_string, &history.created)
            {
                transitions.push(serde_json::json!({
                    "state": to,
                    "at": created,
                }));
            }
        }
    }

    serde_json::Value::Array(transitions)
}

/// Compute cycle time in hours from state history.
///
/// Cycle time = first transition to "In Progress" status category → first
/// transition to "Done" status category.
fn compute_cycle_time(state_history: Option<&serde_json::Value>) -> Option<f64> {
    let transitions = state_history?.as_array()?;
    if transitions.is_empty() {
        return None;
    }

    // Find earliest in-progress transition and earliest done transition
    let mut in_progress_at: Option<time::OffsetDateTime> = None;
    let mut done_at: Option<time::OffsetDateTime> = None;

    for t in transitions {
        let Some(state) = t.get("state").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(at_str) = t.get("at").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(at) = parse_jira_datetime(at_str) else {
            continue;
        };

        // We check for common in-progress and done status names.
        // A more robust approach would check status categories, but the
        // changelog only gives us status names.
        let state_lower = state.to_lowercase();
        if in_progress_at.is_none()
            && (state_lower.contains("progress")
                || state_lower.contains("review")
                || state_lower.contains("development"))
        {
            in_progress_at = Some(at);
        }
        if state_lower == "done" || state_lower == "closed" || state_lower == "resolved" {
            done_at = Some(at);
        }
    }

    match (in_progress_at, done_at) {
        (Some(start), Some(end)) if end > start => {
            let duration = end - start;
            // Precision loss is acceptable for cycle time in hours.
            #[allow(clippy::cast_precision_loss)]
            let hours =
                duration.whole_hours() as f64 + (duration.whole_minutes() % 60) as f64 / 60.0;
            Some(hours)
        }
        _ => None,
    }
}

/// Extract plain text from Atlassian Document Format (ADF) JSON.
///
/// ADF is a nested document structure used by Jira Cloud API v3. This
/// recursively walks the content tree extracting text nodes, with paragraph
/// breaks between block-level elements.
fn adf_to_plain_text(adf: &serde_json::Value) -> String {
    let mut output = String::new();
    adf_extract_text(adf, &mut output);
    output.trim().to_string()
}

fn adf_extract_text(node: &serde_json::Value, output: &mut String) {
    // Text node: {"type": "text", "text": "..."}
    if let Some(text) = node.get("text").and_then(|t| t.as_str()) {
        output.push_str(text);
        return;
    }

    // Container node with children: {"type": "paragraph", "content": [...]}
    let is_block = matches!(
        node.get("type").and_then(|t| t.as_str()),
        Some(
            "paragraph"
                | "heading"
                | "bulletList"
                | "orderedList"
                | "listItem"
                | "blockquote"
                | "codeBlock"
                | "table"
                | "tableRow"
                | "tableCell"
                | "tableHeader"
        )
    );

    if let Some(content) = node.get("content").and_then(|c| c.as_array()) {
        for child in content {
            adf_extract_text(child, output);
        }
        if is_block && !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
    }
}

/// Format an RFC 3339 watermark into Jira JQL datetime format.
///
/// Jira JQL expects `"yyyy/MM/dd HH:mm"` or `"yyyy-MM-dd HH:mm"` format.
fn format_watermark_for_jql(watermark: &str) -> String {
    // Parse as RFC 3339 and reformat
    if let Ok(dt) =
        time::OffsetDateTime::parse(watermark, &time::format_description::well_known::Rfc3339)
    {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            dt.year(),
            dt.month() as u8,
            dt.day(),
            dt.hour(),
            dt.minute()
        )
    } else {
        // Fallback: try to extract date portion
        watermark.replace('T', " ").chars().take(16).collect()
    }
}
