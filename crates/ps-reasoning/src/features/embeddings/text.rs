use ps_core::models::EnrichmentType;
use ps_core::repo::reasoning::{QueuedEmbedding, QueuedEnrichmentData};
use serde_json::Value;

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

    let text = normalise_text(&sections.join("\n\n"));
    if text.is_empty() { None } else { Some(text) }
}

/// Format an enrichment's value into a labelled text section for embedding.
fn format_enrichment(enrichment: &QueuedEnrichmentData) -> Option<String> {
    let value = &enrichment.value;
    match enrichment.enrichment_type.parse::<EnrichmentType>().ok()? {
        EnrichmentType::Significance => {
            let label =
                string_field(value, "significance").or_else(|| string_field(value, "label"))?;
            Some(with_rationale("Significance", label, value))
        }
        EnrichmentType::ReviewDepth => {
            let score = value.get("score")?.as_u64()?;
            if !(1..=5).contains(&score) {
                return None;
            }

            Some(with_rationale("Review depth", &format!("{score}/5"), value))
        }
        EnrichmentType::Sentiment => {
            let label =
                string_field(value, "sentiment").or_else(|| string_field(value, "label"))?;
            Some(format!("Sentiment: {label}"))
        }
        EnrichmentType::Topic => {
            let categories = topic_categories(value)?;
            Some(with_rationale("Topic", &categories, value))
        }
    }
}

fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn with_rationale(title: &str, content: &str, value: &Value) -> String {
    match string_field(value, "rationale") {
        Some(rationale) => format!("{title}: {content} — {rationale}"),
        None => format!("{title}: {content}"),
    }
}

fn topic_categories(value: &Value) -> Option<String> {
    if let Some(primary) = string_field(value, "primary_category") {
        return Some(match string_field(value, "secondary_category") {
            Some(secondary) if secondary != primary => format!("{primary}, {secondary}"),
            _ => primary.to_owned(),
        });
    }

    // Retain compatibility with older category strings and category arrays.
    if let Some(category) =
        string_field(value, "category").or_else(|| string_field(value, "categories"))
    {
        return Some(category.to_owned());
    }

    let categories: Option<Vec<&str>> = value
        .get("categories")?
        .as_array()?
        .iter()
        .map(|category| category.as_str().map(str::trim).filter(|s| !s.is_empty()))
        .collect();
    let categories = categories?;
    if categories.is_empty() {
        None
    } else {
        Some(categories.join(", "))
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
    fn build_text_returns_none_when_content_normalises_to_empty() {
        for body in ["   \n\t", "<p> </p><br>"] {
            let item = QueuedEmbedding {
                id: uuid::Uuid::nil(),
                contribution_id: uuid::Uuid::nil(),
                content_hash: String::new(),
                title: None,
                body: Some(body.into()),
                contribution_type: "pr_review".into(),
                platform: "github".into(),
                enrichments: vec![],
            };

            assert!(build_embedding_text(&item).is_none(), "body: {body:?}");
        }
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

    #[test]
    fn embeddings_include_serialized_enrichment_outputs() {
        use crate::features::enrichment::types::{
            ReviewDepthScore, Sentiment, SentimentLabel, Significance, SignificanceLabel,
            TopicCategory, TopicClassification,
        };

        let enrichments = vec![
            (
                EnrichmentType::Significance,
                serde_json::to_value(SignificanceLabel {
                    significance: Significance::Significant,
                    rationale: "Major architectural feature".into(),
                    confidence: 0.9,
                })
                .unwrap(),
            ),
            (
                EnrichmentType::Sentiment,
                serde_json::to_value(SentimentLabel {
                    sentiment: Sentiment::Constructive,
                    rationale: "Supportive guidance".into(),
                    confidence: 0.8,
                })
                .unwrap(),
            ),
            (
                EnrichmentType::ReviewDepth,
                serde_json::to_value(ReviewDepthScore {
                    score: 4,
                    rationale: "Detailed technical alternatives".into(),
                    confidence: 0.8,
                })
                .unwrap(),
            ),
            (
                EnrichmentType::Topic,
                serde_json::to_value(TopicClassification {
                    primary_category: TopicCategory::Announcement,
                    secondary_category: Some(TopicCategory::Tutorial),
                    rationale: "Release announcement with a walkthrough".into(),
                    confidence: 0.9,
                })
                .unwrap(),
            ),
        ];
        let item = QueuedEmbedding {
            id: uuid::Uuid::nil(),
            contribution_id: uuid::Uuid::nil(),
            content_hash: String::new(),
            title: Some("Add capacity planning module".into()),
            body: Some("Implements capacity planning logic.".into()),
            contribution_type: "pull_request".into(),
            platform: "github".into(),
            enrichments: enrichments
                .into_iter()
                .map(|(kind, value)| QueuedEnrichmentData {
                    enrichment_type: kind.to_string(),
                    value,
                })
                .collect(),
        };

        let text = build_embedding_text(&item).expect("should produce text");
        assert!(text.contains("Significance: significant — Major architectural feature"));
        assert!(text.contains("Sentiment: constructive"));
        assert!(text.contains("Review depth: 4/5 — Detailed technical alternatives"));
        assert!(
            text.contains(
                "Topic: announcement, tutorial — Release announcement with a walkthrough"
            )
        );
    }

    #[test]
    fn formats_legacy_labels_and_topics_without_rationale() {
        let cases = [
            (
                EnrichmentType::Significance,
                serde_json::json!({"label": "notable"}),
                "Significance: notable",
            ),
            (
                EnrichmentType::Sentiment,
                serde_json::json!({"label": "neutral"}),
                "Sentiment: neutral",
            ),
            (
                EnrichmentType::Topic,
                serde_json::json!({"category": "discussion"}),
                "Topic: discussion",
            ),
            (
                EnrichmentType::Topic,
                serde_json::json!({"categories": ["discussion", "tutorial"]}),
                "Topic: discussion, tutorial",
            ),
            (
                EnrichmentType::Topic,
                serde_json::json!({"categories": "discussion"}),
                "Topic: discussion",
            ),
            (
                EnrichmentType::ReviewDepth,
                serde_json::json!({"score": 3, "rationale": "  "}),
                "Review depth: 3/5",
            ),
        ];

        for (kind, value, expected) in cases {
            let enrichment = QueuedEnrichmentData {
                enrichment_type: kind.to_string(),
                value,
            };
            assert_eq!(format_enrichment(&enrichment).as_deref(), Some(expected));
        }
    }

    #[test]
    fn canonical_labels_take_precedence_with_typed_legacy_fallbacks() {
        let cases = [
            (
                EnrichmentType::Significance,
                serde_json::json!({"significance": "significant", "label": "routine"}),
                "Significance: significant",
            ),
            (
                EnrichmentType::Sentiment,
                serde_json::json!({"sentiment": null, "label": "neutral"}),
                "Sentiment: neutral",
            ),
            (
                EnrichmentType::Significance,
                serde_json::json!({"significance": [], "label": "notable"}),
                "Significance: notable",
            ),
            (
                EnrichmentType::Topic,
                serde_json::json!({"primary_category": "discussion", "secondary_category": "discussion", "categories": ["other"]}),
                "Topic: discussion",
            ),
        ];

        for (kind, value, expected) in cases {
            let enrichment = QueuedEnrichmentData {
                enrichment_type: kind.to_string(),
                value,
            };
            assert_eq!(format_enrichment(&enrichment).as_deref(), Some(expected));
        }
    }

    #[test]
    fn malformed_enrichments_do_not_create_embedding_text() {
        let mut item = QueuedEmbedding {
            id: uuid::Uuid::nil(),
            contribution_id: uuid::Uuid::nil(),
            content_hash: String::new(),
            title: None,
            body: None,
            contribution_type: "pr_review".into(),
            platform: "github".into(),
            enrichments: vec![],
        };
        let cases = [
            (EnrichmentType::ReviewDepth, serde_json::json!({"score": 0})),
            (EnrichmentType::ReviewDepth, serde_json::json!({"score": 6})),
            (
                EnrichmentType::ReviewDepth,
                serde_json::json!({"score": "4"}),
            ),
            (
                EnrichmentType::ReviewDepth,
                serde_json::json!({"score": 3.5}),
            ),
            (
                EnrichmentType::ReviewDepth,
                serde_json::json!({"score": {"value": 4}}),
            ),
            (
                EnrichmentType::Significance,
                serde_json::json!({"significance": null}),
            ),
            (
                EnrichmentType::Sentiment,
                serde_json::json!({"sentiment": " "}),
            ),
            (
                EnrichmentType::Topic,
                serde_json::json!({"primary_category": {"name": "discussion"}}),
            ),
            (EnrichmentType::Topic, serde_json::json!({"categories": []})),
            (
                EnrichmentType::Topic,
                serde_json::json!({"categories": ["discussion", 3]}),
            ),
        ];

        for (kind, value) in cases {
            item.enrichments = vec![QueuedEnrichmentData {
                enrichment_type: kind.to_string(),
                value,
            }];
            assert!(
                build_embedding_text(&item).is_none(),
                "{kind}: {:?}",
                item.enrichments[0].value
            );
        }
    }
}
