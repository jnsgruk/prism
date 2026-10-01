//! Context hints and recovery recaps for agent prompts.

use ps_proto::canonical::prism::v1::{Mention, MentionType};

/// Build a recap of prior conversation messages so a freshly-created session
/// has enough context to resolve references from earlier turns.
///
/// Returns `None` when there are no prior assistant messages worth recapping.
pub(super) fn build_conversation_recap(
    messages: &[ps_core::repo::reasoning::ConversationMessage],
) -> Option<String> {
    const MAX_ASSISTANT_CHARS: usize = 500;
    const MAX_RECAP_CHARS: usize = 4000;

    // Only include user and assistant messages (skip error messages).
    let relevant: Vec<_> = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .collect();

    // Nothing to recap if there are fewer than 2 messages (i.e. no prior
    // assistant response — just the current user question).
    if relevant.len() < 2 {
        return None;
    }

    // Exclude the last message — it is the current user question that will be
    // sent as the prompt itself.
    let prior = relevant.get(..relevant.len() - 1).unwrap_or_default();
    if prior.is_empty() {
        return None;
    }

    let mut recap = String::from(
        "## Prior conversation context\n\
         This conversation was started earlier but the session was reset. \
         Here is a summary of the prior exchanges so you can resolve \
         references (\"they\", \"their\", \"this team\", etc.):\n\n",
    );

    for (i, msg) in prior.iter().enumerate() {
        let entry = if msg.role == "user" {
            format!("{}. **User:** {}\n", i + 1, msg.content)
        } else {
            let truncated = if msg.content.len() > MAX_ASSISTANT_CHARS {
                // Find a char boundary at or before MAX_ASSISTANT_CHARS.
                let end = msg
                    .content
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|&i| i <= MAX_ASSISTANT_CHARS)
                    .last()
                    .unwrap_or(0);
                format!("{}…", &msg.content[..end])
            } else {
                msg.content.clone()
            };
            format!("{}. **Assistant:** {}\n", i + 1, truncated)
        };

        if recap.len() + entry.len() > MAX_RECAP_CHARS {
            recap.push_str("\n(earlier messages truncated for brevity)\n");
            break;
        }
        recap.push_str(&entry);
    }

    Some(recap)
}

/// Serialise proto `Mention` messages to a JSONB-compatible value for storage.
pub(super) fn mentions_to_json(mentions: &[Mention]) -> serde_json::Value {
    serde_json::Value::Array(
        mentions
            .iter()
            .map(|m| {
                let type_str = match MentionType::try_from(m.r#type) {
                    Ok(MentionType::Person) => "person",
                    Ok(MentionType::Team) => "team",
                    _ => "file",
                };
                serde_json::json!({
                    "id": m.id,
                    "name": m.name,
                    "type": type_str,
                })
            })
            .collect(),
    )
}

/// Build a combined system hint from attached files and structured mentions.
///
/// The hint is injected as a system-level message so the agent knows about
/// referenced entities without polluting the user's visible message text.
pub(super) fn build_system_hint(attached_files: &[String], mentions: &[Mention]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Collect file paths from both attached_files and file-type mentions,
    // deduplicating so the same path is not listed twice.
    let mut file_paths: Vec<String> = attached_files
        .iter()
        .map(|f| format!("/workspace/{f}"))
        .collect();
    for m in mentions {
        if MentionType::try_from(m.r#type) == Ok(MentionType::File) {
            let wp = format!("/workspace/{}", m.id);
            if !file_paths.contains(&wp) {
                file_paths.push(wp);
            }
        }
    }
    if !file_paths.is_empty() {
        parts.push(format!(
            "The user referenced files in the workspace. Read them from: {}",
            file_paths.join(", ")
        ));
    }

    // Person mentions.
    let people: Vec<&Mention> = mentions
        .iter()
        .filter(|m| MentionType::try_from(m.r#type) == Ok(MentionType::Person))
        .collect();
    if !people.is_empty() {
        let list = people
            .iter()
            .map(|m| format!("{} (ID: {})", m.name, m.id))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "The user mentioned these people: {list}. \
             You already have their names — pass them directly to tools like \
             get_person_profile(person_name=...) or get_person_contributions(person_name=...). \
             No need to call list_people to look them up."
        ));
    }

    // Team mentions.
    let teams: Vec<&Mention> = mentions
        .iter()
        .filter(|m| MentionType::try_from(m.r#type) == Ok(MentionType::Team))
        .collect();
    if !teams.is_empty() {
        let list = teams
            .iter()
            .map(|m| format!("{} (ID: {})", m.name, m.id))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "The user mentioned these teams: {list}. \
             You already have their names — pass them directly to tools like \
             query_contributions(team_name=...) or query_team_metrics(team_name=...). \
             No need to call list_teams to discover them."
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_mention(id: &str, name: &str, mention_type: MentionType) -> Mention {
        Mention {
            id: id.to_string(),
            name: name.to_string(),
            r#type: mention_type as i32,
        }
    }

    #[test]
    fn hint_none_when_empty() {
        assert!(build_system_hint(&[], &[]).is_none());
    }

    #[test]
    fn hint_file_only_from_attached_files() {
        let hint = build_system_hint(&["src/main.rs".to_string()], &[]).unwrap();
        assert!(hint.contains("/workspace/src/main.rs"));
    }

    #[test]
    fn hint_person_only() {
        let mentions = vec![make_mention("abc-123", "Alice", MentionType::Person)];
        let hint = build_system_hint(&[], &mentions).unwrap();
        assert!(hint.contains("Alice (ID: abc-123)"));
        assert!(hint.contains("list_people"));
    }

    #[test]
    fn hint_team_only() {
        let mentions = vec![make_mention("team-456", "Platform", MentionType::Team)];
        let hint = build_system_hint(&[], &mentions).unwrap();
        assert!(hint.contains("Platform (ID: team-456)"));
        assert!(hint.contains("list_teams"));
    }

    #[test]
    fn hint_mixed_mentions() {
        let mentions = vec![
            make_mention("abc-123", "Alice", MentionType::Person),
            make_mention("team-456", "Platform", MentionType::Team),
        ];
        let hint = build_system_hint(&["README.md".to_string()], &mentions).unwrap();
        assert!(hint.contains("/workspace/README.md"));
        assert!(hint.contains("Alice"));
        assert!(hint.contains("Platform"));
    }

    #[test]
    fn hint_file_mention_deduplicates() {
        let mentions = vec![make_mention("src/main.rs", "main.rs", MentionType::File)];
        let hint = build_system_hint(&["src/main.rs".to_string()], &mentions).unwrap();
        // Should only appear once.
        let count = hint.matches("/workspace/src/main.rs").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn hint_file_mention_without_attached_files() {
        let mentions = vec![make_mention("src/lib.rs", "lib.rs", MentionType::File)];
        let hint = build_system_hint(&[], &mentions).unwrap();
        assert!(hint.contains("/workspace/src/lib.rs"));
    }

    #[test]
    fn mentions_to_json_round_trip() {
        let mentions = vec![
            make_mention("abc", "Alice", MentionType::Person),
            make_mention("team-1", "Core", MentionType::Team),
            make_mention("src/main.rs", "main.rs", MentionType::File),
        ];
        let json = mentions_to_json(&mentions);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["type"], "person");
        assert_eq!(arr[1]["type"], "team");
        assert_eq!(arr[2]["type"], "file");
    }

    // -- build_conversation_recap tests --

    fn make_message(role: &str, content: &str) -> ps_core::repo::reasoning::ConversationMessage {
        ps_core::repo::reasoning::ConversationMessage {
            id: Uuid::new_v4(),
            conversation_id: Uuid::new_v4(),
            role: role.to_string(),
            content: content.to_string(),
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            created_at: time::OffsetDateTime::now_utc(),
            attached_files: vec![],
            mentions: serde_json::json!([]),
        }
    }

    #[test]
    fn recap_none_for_single_user_message() {
        let messages = vec![make_message("user", "Tell me about Harry")];
        assert!(build_conversation_recap(&messages).is_none());
    }

    #[test]
    fn recap_none_for_empty() {
        assert!(build_conversation_recap(&[]).is_none());
    }

    #[test]
    fn recap_includes_prior_turns() {
        let messages = vec![
            make_message("user", "Tell me about Harry Pidcock"),
            make_message("assistant", "Harry has 108 contributions and 68 reviews."),
            make_message("user", "Draw a graph of their activity"),
        ];
        let recap = build_conversation_recap(&messages).unwrap();
        assert!(recap.contains("Harry Pidcock"));
        assert!(recap.contains("108 contributions"));
        // Current question should NOT be in the recap.
        assert!(!recap.contains("Draw a graph"));
    }

    #[test]
    fn recap_truncates_long_assistant_content() {
        let long_content = "x".repeat(1000);
        let messages = vec![
            make_message("user", "question"),
            make_message("assistant", &long_content),
            make_message("user", "follow-up"),
        ];
        let recap = build_conversation_recap(&messages).unwrap();
        // Should be truncated with ellipsis.
        assert!(recap.contains('…'));
        assert!(recap.len() < 1000);
    }

    #[test]
    fn recap_skips_error_messages() {
        let messages = vec![
            make_message("user", "Tell me about Harry"),
            make_message("error", "something went wrong"),
            make_message("user", "Try again"),
        ];
        // Only one user+assistant pair before current question — the error is
        // skipped, leaving just the first user message as prior context.
        let recap = build_conversation_recap(&messages).unwrap();
        assert!(recap.contains("Harry"));
        assert!(!recap.contains("something went wrong"));
    }
}
