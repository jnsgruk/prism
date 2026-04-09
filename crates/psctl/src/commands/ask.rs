use std::io::{self, Write};

use anyhow::Result;
use ps_proto::canonical::prism::v1::{AskQuestionRequest, ask_question_response};
use tokio_stream::StreamExt;

use crate::client::Clients;

/// Run an interactive agentic query and stream the response.
pub async fn ask(clients: &mut Clients, question: &str, json: bool) -> Result<()> {
    let req = AskQuestionRequest {
        question: question.to_string(),
        conversation_id: None,
        model_override: None,
        image_model: None,
        attached_files: vec![],
    };

    let resp = clients.reasoning.ask_question(req).await?;
    let mut stream = resp.into_inner();

    let mut tool_calls = 0i32;
    let mut final_answer = String::new();
    let mut total_duration_ms = 0i32;
    let mut had_container_ready = false;

    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let Some(event) = msg.event else {
            continue;
        };

        match event {
            ask_question_response::Event::ContainerStatus(s) => {
                if json {
                    continue;
                }
                match s.status.as_str() {
                    "ready" => {
                        if !had_container_ready {
                            eprintln!("\x1b[32m\u{2705} Agent ready\x1b[0m");
                            had_container_ready = true;
                        }
                    }
                    _ => {
                        eprintln!(
                            "\x1b[33m\u{23f3} {}\x1b[0m",
                            if s.message.is_empty() {
                                format!("Container {}", s.status)
                            } else {
                                s.message
                            }
                        );
                    }
                }
            }

            ask_question_response::Event::ToolCallStarted(t) => {
                if !json {
                    eprint!("\x1b[36m\u{1f527} {}", t.tool_name);
                    if !t.arguments_json.is_empty()
                        && t.arguments_json != "{}"
                        && t.arguments_json.len() < 120
                    {
                        eprint!("({})", truncate_args(&t.arguments_json, 80));
                    }
                    eprintln!("\x1b[0m");
                }
            }

            ask_question_response::Event::ToolCallCompleted(t) => {
                tool_calls += 1;
                total_duration_ms += t.duration_ms;
                if !json {
                    let icon = if t.success { "\u{2705}" } else { "\u{274c}" };
                    if !t.result_summary.is_empty() {
                        eprintln!(
                            "  {icon} {} ({:.1}s)",
                            truncate(&t.result_summary, 60),
                            f64::from(t.duration_ms) / 1000.0,
                        );
                    }
                }
            }

            ask_question_response::Event::ConversationCreated(_)
            | ask_question_response::Event::Thinking(_)
            | ask_question_response::Event::TokenUsage(_) => {}

            ask_question_response::Event::PartialAnswer(a) => {
                // PartialAnswer sends accumulated text, so we just track the latest.
                final_answer = a.text;
            }

            ask_question_response::Event::FinalAnswer(a) => {
                final_answer = a.answer;
                tool_calls = a.tool_call_count;
                total_duration_ms = a.duration_ms;
            }

            ask_question_response::Event::Error(e) => {
                if json {
                    let out = serde_json::json!({
                        "error": e.message,
                        "retryable": e.retryable,
                    });
                    println!("{}", serde_json::to_string_pretty(&out)?);
                } else {
                    eprintln!("\x1b[31mError: {}\x1b[0m", e.message);
                }
                return Ok(());
            }
        }
    }

    if json {
        let out = serde_json::json!({
            "answer": final_answer,
            "tool_calls": tool_calls,
            "duration_ms": total_duration_ms,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        // Print a blank line before the answer.
        if !final_answer.is_empty() {
            println!();
            println!("{final_answer}");
        }

        // Footer with stats.
        if tool_calls > 0 {
            println!();
            println!(
                "\x1b[2m---\n{tool_calls} tool call{} | {:.1}s\x1b[0m",
                if tool_calls == 1 { "" } else { "s" },
                f64::from(total_duration_ms) / 1000.0,
            );
        }

        // Flush stdout in case output is piped.
        let _ = io::stdout().flush();
    }

    Ok(())
}

/// Truncate a string for display, appending an ellipsis if necessary.
fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let end = s
            .char_indices()
            .nth(max.saturating_sub(1))
            .map_or(0, |(i, _)| i);
        format!("{}\u{2026}", &s[..end])
    }
}

/// Truncate JSON arguments string for inline display.
fn truncate_args(s: &str, max: usize) -> String {
    // Strip outer braces for cleaner display.
    let inner = s.trim().strip_prefix('{').unwrap_or(s);
    let inner = inner.strip_suffix('}').unwrap_or(inner);
    let inner = inner.trim();
    truncate(inner, max)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::items_after_statements)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hello world", 20), "hello world");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let input = "abcdefghij";
        let result = truncate(input, 5);
        // Should keep 4 chars + ellipsis (max - 1 chars + \u{2026})
        assert_eq!(result, "abcd\u{2026}");
    }

    #[test]
    fn truncate_newlines_replaced() {
        assert_eq!(truncate("a\nb\nc", 20), "a b c");
    }

    #[test]
    fn truncate_args_strips_braces() {
        let input = r#"{"team": "Kernel"}"#;
        let result = truncate_args(input, 80);
        assert_eq!(result, r#""team": "Kernel""#);
    }

    #[test]
    fn truncate_args_handles_no_braces() {
        let input = "plain text";
        let result = truncate_args(input, 80);
        assert_eq!(result, "plain text");
    }
}
