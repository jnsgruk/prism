//! Structured JQL and scoped preflight. Never interpolate unescaped identities.

use ps_core::Error;
use ps_core::ingestion::{IngestionContext, SourceRunContext, parse_since_date};

use super::{Cursor, parse_jira_datetime};

pub(super) fn configured_projects(settings: &serde_json::Value) -> Result<Vec<String>, Error> {
    let projects = match settings.get("projects") {
        None => vec![],
        Some(value) => serde_json::from_value::<Vec<String>>(value.clone()).map_err(|_| {
            Error::Validation("Jira projects must be an array of project keys".into())
        })?,
    };
    for project in &projects {
        quoted_value(project)?;
    }
    Ok(projects)
}

pub(super) fn validate_person_mode(
    ctx: &IngestionContext,
    request: &SourceRunContext,
) -> Result<(), Error> {
    let mode = ctx
        .source_config
        .settings
        .get("api_mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("cloud");
    if mode != "cloud" {
        return Err(Error::Validation(
            "Jira person backfill supports Cloud only; Server/Data Center is unsupported".into(),
        ));
    }
    account_id(request)?;
    Ok(())
}

pub(super) fn validate_cursor_scope(ctx: &IngestionContext, cur: &Cursor) -> Result<(), Error> {
    if let Some(request) = ctx.person_request()? {
        validate_person_mode(ctx, request)?;
        if cur.projects != configured_projects(&ctx.source_config.settings)? {
            return Err(Error::Validation(
                "Jira cursor projects no longer match the selected source".into(),
            ));
        }
        if cur.request.as_ref() != Some(request) {
            return Err(Error::Validation(
                "Jira cursor scope does not match the frozen run".into(),
            ));
        }
        let current = ctx
            .source_config
            .settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("https://jira.atlassian.net");
        if current.trim_end_matches('/') != cur.base_url.trim_end_matches('/') {
            return Err(Error::Validation(
                "Jira endpoint changed during the frozen run; restart this backfill".into(),
            ));
        }
    } else if cur
        .request
        .as_ref()
        .is_some_and(|request| request.scope.person_id().is_some())
    {
        return Err(Error::Validation(
            "person Jira cursor requires a scoped context".into(),
        ));
    }
    Ok(())
}

pub(super) fn account_id(request: &SourceRunContext) -> Result<&str, Error> {
    let id = request
        .source
        .identity
        .as_ref()
        .and_then(|identity| identity.platform_user_id.as_deref())
        .ok_or_else(|| Error::Validation("Jira Cloud account ID is required".into()))?;
    quoted_value(id)?;
    Ok(id)
}

fn quoted_value(value: &str) -> Result<String, Error> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(Error::Validation("invalid Jira JQL identifier".into()));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub(super) fn build_jql(cur: &Cursor, project: Option<&str>) -> Result<String, Error> {
    let mut clauses = Vec::new();
    if let Some(project) = project {
        clauses.push(format!("project = {}", quoted_value(project)?));
    }
    if let Some(request) = cur
        .request
        .as_ref()
        .filter(|request| request.scope.person_id().is_some())
    {
        clauses.push(format!(
            "assignee = {}",
            quoted_value(account_id(request)?)?
        ));
        let timezone = cur
            .api_timezone
            .as_deref()
            .ok_or_else(|| Error::Validation("Jira API user timezone is required".into()))?;
        let since = request
            .since_date
            .as_deref()
            .ok_or_else(|| Error::Validation("Jira person backfill requires since_date".into()))?;
        let lower = parse_since_date(since)?.midnight().assume_utc();
        clauses.push(format!(
            "updated >= \"{}\"",
            conservative_local_lower(lower, timezone)?
        ));
    } else if let Some(watermark) = &cur.watermark {
        let dt = parse_jira_datetime(watermark)?;
        clauses.push(format!(
            "updated >= \"{:04}-{:02}-{:02} {:02}:{:02}\"",
            dt.year(),
            dt.month() as u8,
            dt.day(),
            dt.hour(),
            dt.minute()
        ));
    }
    let ordering = if cur
        .request
        .as_ref()
        .is_some_and(|request| request.scope.person_id().is_some())
    {
        "ORDER BY updated ASC, key ASC"
    } else {
        "ORDER BY updated ASC"
    };
    if clauses.is_empty() {
        Ok(ordering.into())
    } else {
        Ok(format!("{} {ordering}", clauses.join(" AND ")))
    }
}

/// JQL interprets naive minutes in the authenticated user's zone. Widen one
/// local day to include minute rounding and repeated/skipped DST wall times;
/// exact inclusive UTC eligibility is applied to returned updated timestamps.
/// Avoid an upper JQL cutoff: timezone changes and search-index lag must never
/// exclude an eligible result before that exact check.
fn conservative_local_lower(lower: time::OffsetDateTime, timezone: &str) -> Result<String, Error> {
    let timezone: chrono_tz::Tz = timezone
        .parse()
        .map_err(|_| Error::Validation("Jira API user returned an unsupported timezone".into()))?;
    let lower = chrono::DateTime::from_timestamp(lower.unix_timestamp(), 0)
        .ok_or_else(|| Error::Validation("Jira backfill date out of range".into()))?;
    let local = lower.with_timezone(&timezone).naive_local() - chrono::Duration::days(1);
    Ok(local.format("%Y-%m-%d %H:%M").to_string())
}

pub(super) fn eligible_issue(
    cur: &Cursor,
    issue: &super::super::client::JiraIssue,
) -> Result<bool, Error> {
    let Some(request) = cur
        .request
        .as_ref()
        .filter(|request| request.scope.person_id().is_some())
    else {
        return Ok(true);
    };
    let assignee = issue
        .fields
        .assignee
        .as_ref()
        .and_then(|user| user.account_id.as_deref());
    if assignee != Some(account_id(request)?) {
        return Ok(false);
    }
    // Reject unexpected project results even when Jira's index is stale.
    if !cur.projects.is_empty()
        && !cur.projects.iter().any(|project| {
            issue
                .key
                .strip_prefix(project)
                .is_some_and(|suffix| suffix.starts_with('-'))
        })
    {
        return Ok(false);
    }
    let updated = issue.fields.updated.as_deref().ok_or_else(|| {
        Error::Validation("Jira person search returned an issue without updated timestamp".into())
    })?;
    request.contains_event(parse_jira_datetime(updated)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_escape_quotes_backslashes_and_cloud_punctuation() {
        assert_eq!(
            quoted_value(r#"557:abc\id" OR assignee != "x"#).unwrap(),
            r#""557:abc\\id\" OR assignee != \"x""#
        );
        for invalid in ["", " ", "abc\nOR true", "abc\u{0}"] {
            assert!(quoted_value(invalid).is_err());
        }
    }

    #[test]
    fn projects_never_silently_widen_malformed_configuration() {
        for value in [
            serde_json::json!({"projects":"PROJ"}),
            serde_json::json!({"projects":["PROJ", 1]}),
            serde_json::json!({"projects":[""]}),
        ] {
            assert!(configured_projects(&value).is_err());
        }
        assert!(
            configured_projects(&serde_json::json!({"projects":[]}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn all_queries_keep_original_project_and_lower_date_semantics() {
        let mut cursor: Cursor = serde_json::from_value(serde_json::json!({
            "watermark":"2025-03-01T10:31:59Z", "projects":["PROJ"],
            "next_page_token":null, "max_updated_at":null,
            "base_url":"https://example.atlassian.net", "story_points_field":null,
            "api_mode":"cloud"
        }))
        .unwrap();
        assert_eq!(
            build_jql(&cursor, Some("PROJ")).unwrap(),
            "project = \"PROJ\" AND updated >= \"2025-03-01 10:31\" ORDER BY updated ASC"
        );
        assert_eq!(
            build_jql(&cursor, None).unwrap(),
            "updated >= \"2025-03-01 10:31\" ORDER BY updated ASC"
        );
        cursor.watermark = None;
        assert_eq!(
            build_jql(&cursor, Some("PROJ")).unwrap(),
            "project = \"PROJ\" ORDER BY updated ASC"
        );
        assert_eq!(build_jql(&cursor, None).unwrap(), "ORDER BY updated ASC");
    }

    #[test]
    fn timezone_lower_is_conservative_near_midnight_and_dst() {
        for (utc, zone, expected) in [
            (
                "2025-03-30T00:00:00Z",
                "Europe/Copenhagen",
                "2025-03-29 01:00",
            ),
            (
                "2025-10-26T00:00:00Z",
                "Europe/Copenhagen",
                "2025-10-25 02:00",
            ),
            (
                "2025-03-01T00:00:00Z",
                "America/Los_Angeles",
                "2025-02-27 16:00",
            ),
            (
                "2025-03-01T00:00:00Z",
                "Pacific/Kiritimati",
                "2025-02-28 14:00",
            ),
        ] {
            assert_eq!(
                conservative_local_lower(parse_jira_datetime(utc).unwrap(), zone).unwrap(),
                expected
            );
        }
        assert!(conservative_local_lower(time::OffsetDateTime::UNIX_EPOCH, "invalid").is_err());
    }
}
