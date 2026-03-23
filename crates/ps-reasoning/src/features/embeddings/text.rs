use ps_core::repo::reasoning::{QueuedEmbedding, QueuedEnrichmentData};

/// Maximum characters before truncation (~8K tokens at ~4 chars/token).
const MAX_CHARS: usize = 32_000;

/// Build the text to embed for a queued contribution.
///
/// Combines raw contribution text with enrichment rationale for richer
/// embeddings. Returns `None` only if there is no content AND no enrichments.
pub fn build_embedding_text(item: &QueuedEmbedding) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();

    // 1. Raw contribution text
    match item.contribution_type.as_str() {
        "pull_request" | "discourse_topic" | "jira_ticket" => {
            let title = item.title.as_deref().unwrap_or_default();
            let body = item.body.as_deref().unwrap_or_default();
            if !title.is_empty() || !body.is_empty() {
                sections.push(format!("{title}\n\n{body}"));
            }
        }
        "pr_review" => {
            let body = item.body.as_deref().unwrap_or_default();
            if !body.is_empty() {
                sections.push(body.to_string());
            }
        }
        _ => return None,
    }

    // 2. Enrichment rationale — appended as labelled sections
    for enrichment in &item.enrichments {
        if let Some(text) = format_enrichment(enrichment) {
            sections.push(text);
        }
    }

    if sections.is_empty() {
        return None;
    }

    Some(normalise_text(&sections.join("\n\n")))
}

/// Format an enrichment's value into a labelled text section for embedding.
fn format_enrichment(enrichment: &QueuedEnrichmentData) -> Option<String> {
    let v = &enrichment.value;
    match enrichment.enrichment_type.as_str() {
        "significance" => {
            let label = v.get("label")?.as_str()?;
            let rationale = v.get("rationale")?.as_str()?;
            Some(format!("Significance: {label} — {rationale}"))
        }
        "review_depth" => {
            let score = v.get("score")?;
            let rationale = v.get("rationale")?.as_str()?;
            Some(format!("Review depth: {score}/5 — {rationale}"))
        }
        "sentiment" => {
            let label = v.get("label")?.as_str()?;
            Some(format!("Sentiment: {label}"))
        }
        "topic" => {
            let categories = v.get("categories")?;
            Some(format!("Topics: {categories}"))
        }
        _ => None,
    }
}

/// Strip HTML tags, collapse whitespace, truncate to ~32k chars.
pub fn normalise_text(input: &str) -> String {
    // Strip HTML tags (simple regex-free approach: remove <...> sequences)
    let mut result = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Collapse whitespace runs to single spaces
    let collapsed: String = result.split_whitespace().collect::<Vec<_>>().join(" ");

    // Truncate to MAX_CHARS on a char boundary
    if collapsed.len() > MAX_CHARS {
        collapsed.chars().take(MAX_CHARS).collect()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_text_for_pr_with_enrichments() {
        let item = QueuedEmbedding {
            id: uuid::Uuid::nil(),
            contribution_id: uuid::Uuid::nil(),
            content_hash: String::new(),
            title: Some("Fix auth race condition".into()),
            body: Some("This PR fixes a race condition in the auth middleware.".into()),
            contribution_type: "pull_request".into(),
            platform: "github".into(),
            enrichments: vec![QueuedEnrichmentData {
                enrichment_type: "significance".into(),
                value: serde_json::json!({
                    "label": "Notable",
                    "rationale": "Fixes a critical auth bug affecting all users"
                }),
            }],
        };

        let text = build_embedding_text(&item).expect("should produce text");
        assert!(text.contains("Fix auth race condition"));
        assert!(text.contains("Significance: Notable"));
    }

    #[test]
    fn build_text_returns_none_for_unknown_type() {
        let item = QueuedEmbedding {
            id: uuid::Uuid::nil(),
            contribution_id: uuid::Uuid::nil(),
            content_hash: String::new(),
            title: None,
            body: None,
            contribution_type: "unknown".into(),
            platform: "github".into(),
            enrichments: vec![],
        };
        assert!(build_embedding_text(&item).is_none());
    }

    #[test]
    fn normalise_strips_html_and_collapses_whitespace() {
        let input = "<p>Hello   <b>world</b></p>  \n\n  foo";
        let result = normalise_text(input);
        assert_eq!(result, "Hello world foo");
    }

    #[test]
    fn normalise_truncates_long_text() {
        let input = "a".repeat(40_000);
        let result = normalise_text(&input);
        assert_eq!(result.len(), MAX_CHARS);
    }

    #[test]
    fn enrichment_only_pr_review() {
        let item = QueuedEmbedding {
            id: uuid::Uuid::nil(),
            contribution_id: uuid::Uuid::nil(),
            content_hash: String::new(),
            title: None,
            body: Some(String::new()),
            contribution_type: "pr_review".into(),
            platform: "github".into(),
            enrichments: vec![QueuedEnrichmentData {
                enrichment_type: "review_depth".into(),
                value: serde_json::json!({
                    "score": 4,
                    "rationale": "Thorough review with inline suggestions"
                }),
            }],
        };

        let text = build_embedding_text(&item).expect("should produce text from enrichment alone");
        assert!(text.contains("Review depth: 4/5"));
    }
}
