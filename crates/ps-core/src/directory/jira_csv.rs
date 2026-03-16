//! Jira user CSV parser.
//!
//! Parses the CSV export from Jira Cloud's admin console
//! (`Organization → Users → Export users`). The CSV contains:
//! - `User id` — Jira `accountId` (opaque string)
//! - `email` — email address for matching against `org.people`
//! - `User name` — display name for warnings
//! - `User status` — only `Active` users are imported

use crate::Error;

/// A single Jira user parsed from the CSV export.
#[derive(Debug, Clone)]
pub struct JiraUserRecord {
    /// Jira `accountId` — stored as `platform_user_id`.
    pub account_id: String,
    /// Email address for matching against `org.people`.
    pub email: String,
    /// Display name for warning messages.
    pub display_name: String,
}

/// Parse a Jira user CSV export into a list of active user records.
///
/// Skips non-active users and rows with missing required fields.
/// Returns the parsed records and any warnings encountered.
pub fn parse_jira_user_csv(content: &str) -> Result<(Vec<JiraUserRecord>, Vec<String>), Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| Error::Validation(format!("invalid CSV headers: {e}")))?
        .clone();

    // Find column indices by name (case-insensitive)
    let user_id_idx = find_column(&headers, &["User id", "user_id", "accountId"]);
    let email_idx = find_column(&headers, &["email", "Email", "Email address"]);
    let name_idx = find_column(
        &headers,
        &["User name", "Display name", "name", "displayName"],
    );
    let status_idx = find_column(&headers, &["User status", "status", "Status"]);

    let user_id_idx = user_id_idx.ok_or_else(|| {
        Error::Validation("CSV missing required column: 'User id' (or 'accountId')".into())
    })?;
    let email_idx = email_idx
        .ok_or_else(|| Error::Validation("CSV missing required column: 'email'".into()))?;

    let mut records = Vec::new();
    let mut warnings = Vec::new();

    for (row_num, result) in reader.records().enumerate() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("row {}: parse error: {e}", row_num + 2));
                continue;
            }
        };

        let account_id = record.get(user_id_idx).unwrap_or("").trim().to_string();
        let email = record.get(email_idx).unwrap_or("").trim().to_string();
        let display_name = name_idx
            .and_then(|i| record.get(i))
            .unwrap_or("")
            .trim()
            .to_string();
        let status = status_idx
            .and_then(|i| record.get(i))
            .unwrap_or("Active")
            .trim()
            .to_string();

        // Skip non-active users
        if !status.eq_ignore_ascii_case("active") {
            continue;
        }

        if account_id.is_empty() {
            warnings.push(format!("row {}: missing User id, skipping", row_num + 2));
            continue;
        }

        if email.is_empty() {
            warnings.push(format!(
                "row {}: missing email for user {} ({}), skipping",
                row_num + 2,
                display_name,
                account_id
            ));
            continue;
        }

        records.push(JiraUserRecord {
            account_id,
            email,
            display_name,
        });
    }

    Ok((records, warnings))
}

/// Find a column index by trying multiple header name variants.
fn find_column(headers: &csv::StringRecord, names: &[&str]) -> Option<usize> {
    for name in names {
        for (i, header) in headers.iter().enumerate() {
            if header.trim().eq_ignore_ascii_case(name) {
                return Some(i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_csv() {
        let csv = "\
User id,email,User name,User status
abc123,alice@example.com,Alice Smith,Active
def456,bob@example.com,Bob Jones,Active
ghi789,charlie@example.com,Charlie Brown,Inactive
";
        let (records, warnings) = parse_jira_user_csv(csv).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].account_id, "abc123");
        assert_eq!(records[0].email, "alice@example.com");
        assert_eq!(records[1].display_name, "Bob Jones");
        assert!(warnings.is_empty());
    }

    #[test]
    fn skips_missing_email() {
        let csv = "\
User id,email,User name,User status
abc123,,Alice Smith,Active
";
        let (records, warnings) = parse_jira_user_csv(csv).unwrap();
        assert_eq!(records.len(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("missing email"));
    }

    #[test]
    fn missing_required_column() {
        let csv = "name,status\nAlice,Active\n";
        let result = parse_jira_user_csv(csv);
        assert!(result.is_err());
    }
}
