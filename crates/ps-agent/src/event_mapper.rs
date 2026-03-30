//! Maps `OpenCode` SSE events to `AskQuestionResponse` proto messages.

use std::collections::BTreeMap;

use opencode_sdk::types::event::Event;
use opencode_sdk::types::message::{Part, ToolState};
use ps_proto::canonical::prism::v1::{
    AgentContainerStatus, AgentError, AgentPartialAnswer, AgentThinking, AgentTokenUsage,
    AgentToolCallCompleted, AgentToolCallStarted, AskQuestionResponse, ask_question_response,
};

/// Stateful mapper that accumulates text across multiple `Part::Text` blocks.
///
/// `OpenCode` emits each message part independently. When a model generates
/// text → tool call → text, each text part has its own index and its own
/// cumulative content. Without tracking all text parts, only the latest
/// part's text survives — previous text blocks are lost.
#[derive(Default)]
pub struct EventMapper {
    /// Text content per part index, ordered by index for stable concatenation.
    text_parts: BTreeMap<i32, String>,
    /// Cumulative input tokens across all steps.
    cumulative_input: u64,
    /// Cumulative output tokens across all steps.
    cumulative_output: u64,
}

impl EventMapper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Map an `OpenCode` SSE event to zero or more `AskQuestionResponse` proto messages.
    ///
    /// Returns `None` for events we don't surface to the client (heartbeats, etc.).
    pub fn map_event(&mut self, event: &Event) -> Option<AskQuestionResponse> {
        match event {
            Event::MessagePartUpdated { properties } => self.map_message_part(properties),
            Event::SessionError { properties } => {
                let msg = properties
                    .error
                    .as_ref()
                    .map_or("Unknown error".to_string(), |e| format!("{e:?}"));
                Some(agent_event(ask_question_response::Event::Error(
                    AgentError {
                        message: msg,
                        retryable: false,
                    },
                )))
            }
            _ => None,
        }
    }

    /// Map a message part update to the appropriate proto event.
    fn map_message_part(
        &mut self,
        props: &opencode_sdk::types::event::MessagePartEventProps,
    ) -> Option<AskQuestionResponse> {
        let part = props.part.as_ref()?;
        let part_index = i32::try_from(props.index.unwrap_or(0)).unwrap_or(0);

        match part {
            Part::Tool {
                tool,
                state,
                input,
                call_id,
                ..
            } => map_tool_part(tool, state.as_ref(), input, call_id),

            Part::Text { text, .. } => {
                // Track each text part by index and emit the full concatenation.
                if text.is_empty() {
                    return None;
                }
                self.text_parts.insert(part_index, text.clone());
                let accumulated = self.accumulated_text();
                Some(agent_event(ask_question_response::Event::PartialAnswer(
                    AgentPartialAnswer { text: accumulated },
                )))
            }

            Part::Reasoning { text, .. } => {
                if text.is_empty() {
                    return None;
                }
                Some(agent_event(ask_question_response::Event::Thinking(
                    AgentThinking {
                        text: text.clone(),
                        part_index,
                        step_id: String::new(),
                        step_seq: 0,
                    },
                )))
            }

            Part::StepFinish { tokens, .. } => {
                if let Some(usage) = tokens {
                    self.cumulative_input += usage.input;
                    self.cumulative_output += usage.output;
                    Some(agent_event(ask_question_response::Event::TokenUsage(
                        AgentTokenUsage {
                            input_tokens: self.cumulative_input.cast_signed(),
                            output_tokens: self.cumulative_output.cast_signed(),
                            context_window: 0, // Populated by the handler with model info
                        },
                    )))
                } else {
                    None
                }
            }

            _ => None,
        }
    }

    /// Return cumulative token totals (input, output).
    pub fn token_totals(&self) -> (u64, u64) {
        (self.cumulative_input, self.cumulative_output)
    }

    /// Concatenate all tracked text parts in index order.
    fn accumulated_text(&self) -> String {
        let parts: Vec<&str> = self.text_parts.values().map(String::as_str).collect();
        parts.join("\n\n")
    }
}

/// Pick the best available input — prefer state-level input over the top-level
/// `Part::Tool.input` which is often empty for Running/Pending updates.
fn best_input<'a>(
    input: &'a serde_json::Value,
    state: Option<&'a ToolState>,
) -> &'a serde_json::Value {
    let is_empty = input.is_null() || input.as_object().is_some_and(serde_json::Map::is_empty);
    if !is_empty {
        return input;
    }
    match state {
        Some(ToolState::Pending(p)) => &p.input,
        Some(ToolState::Running(r)) => &r.input,
        _ => input,
    }
}

/// Map a tool part to either a started or completed event.
fn map_tool_part(
    tool_name: &str,
    state: Option<&ToolState>,
    input: &serde_json::Value,
    call_id: &str,
) -> Option<AskQuestionResponse> {
    match state {
        Some(ToolState::Pending(_) | ToolState::Running(_)) => Some(agent_event(
            ask_question_response::Event::ToolCallStarted(AgentToolCallStarted {
                tool_name: tool_name.to_string(),
                arguments_json: best_input(input, state).to_string(),
                call_id: call_id.to_string(),
                step_id: String::new(),
                step_seq: 0,
            }),
        )),
        Some(ToolState::Completed(completed)) => {
            let duration_ms = completed
                .time
                .end
                .checked_sub(completed.time.start)
                .and_then(|d| i32::try_from(d).ok())
                .unwrap_or(0);

            Some(agent_event(
                ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
                    tool_name: tool_name.to_string(),
                    result_summary: truncate_output(&completed.output, 4000),
                    duration_ms,
                    success: true,
                    call_id: call_id.to_string(),
                    step_id: String::new(),
                    step_seq: 0,
                }),
            ))
        }
        Some(ToolState::Error(error)) => Some(agent_event(
            ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
                tool_name: tool_name.to_string(),
                result_summary: truncate_output(&error.error, 200),
                duration_ms: 0,
                success: false,
                call_id: call_id.to_string(),
                step_id: String::new(),
                step_seq: 0,
            }),
        )),
        _ => None,
    }
}

/// Build a container status proto event.
pub fn container_status_event(status: &str, message: &str) -> AskQuestionResponse {
    agent_event(ask_question_response::Event::ContainerStatus(
        AgentContainerStatus {
            status: status.to_string(),
            message: message.to_string(),
        },
    ))
}

/// Wrap an event variant into the proto response.
fn agent_event(event: ask_question_response::Event) -> AskQuestionResponse {
    AskQuestionResponse { event: Some(event) }
}

/// Truncate output for display, preserving the first `max_len` characters.
fn truncate_output(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencode_sdk::types::event::MessagePartEventProps;
    use opencode_sdk::types::message::{
        ToolStateCompleted, ToolStateError, ToolStatePending, ToolTimeRange,
    };

    fn text_event(text: &str) -> Event {
        text_event_at_index(text, 0)
    }

    fn text_event_at_index(text: &str, index: usize) -> Event {
        Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(index),
                part: Some(Part::Text {
                    id: None,
                    text: text.to_string(),
                    synthetic: None,
                    ignored: None,
                    metadata: None,
                }),
                delta: Some(text.to_string()),
                extra: serde_json::Value::Null,
            }),
        }
    }

    fn tool_pending_event(tool_name: &str) -> Event {
        tool_pending_event_with_id(tool_name, "call-1")
    }

    fn tool_pending_event_with_id(tool_name: &str, call_id: &str) -> Event {
        Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(0),
                part: Some(Part::Tool {
                    id: None,
                    call_id: call_id.to_string(),
                    tool: tool_name.to_string(),
                    input: serde_json::json!({"team_name": "Kernel"}),
                    state: Some(ToolState::Pending(ToolStatePending {
                        status: "pending".to_string(),
                        input: serde_json::json!({"team_name": "Kernel"}),
                        raw: "{}".to_string(),
                        extra: serde_json::Value::Null,
                    })),
                    metadata: None,
                }),
                delta: None,
                extra: serde_json::Value::Null,
            }),
        }
    }

    fn tool_completed_event(tool_name: &str, output: &str) -> Event {
        tool_completed_event_with_id(tool_name, output, "call-1")
    }

    fn tool_completed_event_with_id(tool_name: &str, output: &str, call_id: &str) -> Event {
        Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(0),
                part: Some(Part::Tool {
                    id: None,
                    call_id: call_id.to_string(),
                    tool: tool_name.to_string(),
                    input: serde_json::json!({}),
                    state: Some(ToolState::Completed(ToolStateCompleted {
                        status: "completed".to_string(),
                        input: serde_json::json!({}),
                        output: output.to_string(),
                        title: tool_name.to_string(),
                        metadata: serde_json::Value::Null,
                        time: ToolTimeRange {
                            start: 1000,
                            end: 1045,
                            compacted: None,
                        },
                        attachments: None,
                        extra: serde_json::Value::Null,
                    })),
                    metadata: None,
                }),
                delta: None,
                extra: serde_json::Value::Null,
            }),
        }
    }

    fn tool_error_event(tool_name: &str, error: &str) -> Event {
        Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(0),
                part: Some(Part::Tool {
                    id: None,
                    call_id: "call-1".to_string(),
                    tool: tool_name.to_string(),
                    input: serde_json::json!({}),
                    state: Some(ToolState::Error(ToolStateError {
                        status: "error".to_string(),
                        input: serde_json::json!({}),
                        error: error.to_string(),
                        metadata: None,
                        time: ToolTimeRange {
                            start: 1000,
                            end: 1010,
                            compacted: None,
                        },
                        extra: serde_json::Value::Null,
                    })),
                    metadata: None,
                }),
                delta: None,
                extra: serde_json::Value::Null,
            }),
        }
    }

    fn reasoning_event(text: &str, index: usize) -> Event {
        Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(index),
                part: Some(Part::Reasoning {
                    id: None,
                    text: text.to_string(),
                    metadata: None,
                }),
                delta: None,
                extra: serde_json::Value::Null,
            }),
        }
    }

    #[test]
    fn text_event_maps_to_partial_answer() {
        let mut mapper = EventMapper::new();
        let event = text_event("Hello world");
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::PartialAnswer(a) => {
                assert_eq!(a.text, "Hello world");
            }
            other => panic!("Expected PartialAnswer, got {other:?}"),
        }
    }

    #[test]
    fn multiple_text_parts_accumulate() {
        let mut mapper = EventMapper::new();

        // First text part at index 0.
        mapper.map_event(&text_event_at_index("First block", 0));

        // Second text part at index 2 (index 1 might be a tool).
        let result = mapper
            .map_event(&text_event_at_index("Second block", 2))
            .unwrap();

        match result.event.unwrap() {
            ask_question_response::Event::PartialAnswer(a) => {
                assert_eq!(a.text, "First block\n\nSecond block");
            }
            other => panic!("Expected PartialAnswer, got {other:?}"),
        }
    }

    #[test]
    fn cumulative_update_to_same_part_replaces() {
        let mut mapper = EventMapper::new();

        // Part 0 grows cumulatively.
        mapper.map_event(&text_event_at_index("Hello", 0));
        let result = mapper
            .map_event(&text_event_at_index("Hello world", 0))
            .unwrap();

        match result.event.unwrap() {
            ask_question_response::Event::PartialAnswer(a) => {
                assert_eq!(a.text, "Hello world");
            }
            other => panic!("Expected PartialAnswer, got {other:?}"),
        }
    }

    #[test]
    fn empty_text_is_skipped() {
        let mut mapper = EventMapper::new();
        assert!(mapper.map_event(&text_event("")).is_none());
    }

    #[test]
    fn empty_reasoning_is_skipped() {
        let mut mapper = EventMapper::new();
        assert!(mapper.map_event(&reasoning_event("", 0)).is_none());
    }

    #[test]
    fn tool_pending_maps_to_started() {
        let mut mapper = EventMapper::new();
        let event = tool_pending_event("mcp_prism_list_teams");
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::ToolCallStarted(s) => {
                assert_eq!(s.tool_name, "mcp_prism_list_teams");
                assert!(s.arguments_json.contains("Kernel"));
                assert_eq!(s.call_id, "call-1");
            }
            other => panic!("Expected ToolCallStarted, got {other:?}"),
        }
    }

    #[test]
    fn tool_completed_maps_to_completed() {
        let mut mapper = EventMapper::new();
        let event = tool_completed_event("bash", "3 files found");
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::ToolCallCompleted(c) => {
                assert_eq!(c.tool_name, "bash");
                assert_eq!(c.result_summary, "3 files found");
                assert_eq!(c.duration_ms, 45);
                assert!(c.success);
                assert_eq!(c.call_id, "call-1");
            }
            other => panic!("Expected ToolCallCompleted, got {other:?}"),
        }
    }

    #[test]
    fn tool_error_maps_to_failed_completed() {
        let mut mapper = EventMapper::new();
        let event = tool_error_event("bash", "command not found");
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::ToolCallCompleted(c) => {
                assert!(!c.success);
                assert!(c.result_summary.contains("command not found"));
                assert_eq!(c.call_id, "call-1");
            }
            other => panic!("Expected ToolCallCompleted, got {other:?}"),
        }
    }

    #[test]
    fn session_error_maps_to_agent_error() {
        let mut mapper = EventMapper::new();
        let event = Event::SessionError {
            properties: opencode_sdk::types::event::SessionErrorProps {
                session_id: Some("s1".to_string()),
                error: None,
                extra: serde_json::Value::Null,
            },
        };
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::Error(e) => {
                assert!(!e.retryable);
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[test]
    fn heartbeat_returns_none() {
        let mut mapper = EventMapper::new();
        let event = Event::ServerHeartbeat {
            properties: serde_json::Value::Null,
        };
        assert!(mapper.map_event(&event).is_none());
    }

    #[test]
    fn container_status_event_builds_correctly() {
        let event = container_status_event("creating", "Starting agent container...");
        match event.event.unwrap() {
            ask_question_response::Event::ContainerStatus(s) => {
                assert_eq!(s.status, "creating");
                assert_eq!(s.message, "Starting agent container...");
            }
            other => panic!("Expected ContainerStatus, got {other:?}"),
        }
    }

    #[test]
    fn truncate_output_respects_limit() {
        assert_eq!(truncate_output("short", 200), "short");
        let long = "x".repeat(300);
        let truncated = truncate_output(&long, 200);
        assert_eq!(truncated.len(), 203); // 200 + "..."
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn state_input_used_when_top_level_empty() {
        let mut mapper = EventMapper::new();
        // Top-level input is empty, but state.input has the command.
        let event = Event::MessagePartUpdated {
            properties: Box::new(MessagePartEventProps {
                session_id: Some("s1".to_string()),
                message_id: Some("m1".to_string()),
                index: Some(0),
                part: Some(Part::Tool {
                    id: None,
                    call_id: "call-1".to_string(),
                    tool: "bash".to_string(),
                    input: serde_json::json!({}), // empty top-level
                    state: Some(ToolState::Pending(ToolStatePending {
                        status: "pending".to_string(),
                        input: serde_json::json!({"command": "uv run python script.py"}),
                        raw: "{}".to_string(),
                        extra: serde_json::Value::Null,
                    })),
                    metadata: None,
                }),
                delta: None,
                extra: serde_json::Value::Null,
            }),
        };
        let result = mapper.map_event(&event).unwrap();
        match result.event.unwrap() {
            ask_question_response::Event::ToolCallStarted(s) => {
                assert!(s.arguments_json.contains("uv run python script.py"));
            }
            other => panic!("Expected ToolCallStarted, got {other:?}"),
        }
    }
}
