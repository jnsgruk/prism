use ps_proto::canonical::prism::v1::{
    AgentContainerStatus, AgentError, AgentFinalAnswer, AgentPartialAnswer, AgentThinking,
    AgentTokenUsage, AgentToolCallCompleted, AgentToolCallStarted, AskQuestionResponse,
    ResumeStreamResponse, ask_question_response,
};

/// Map a database event row to a proto `AskQuestionResponse`.
#[allow(clippy::too_many_lines)]
pub fn map_db_event_to_proto(
    event: &ps_core::repo::reasoning::ConversationEvent,
) -> Option<AskQuestionResponse> {
    let event_type = &event.event_type;
    let payload = &event.payload;
    let step_id = event.step_id.clone().unwrap_or_default();
    let step_seq = event.step_seq.unwrap_or(0);

    let proto_event = match event_type.as_str() {
        "container_status" => ask_question_response::Event::ContainerStatus(AgentContainerStatus {
            status: payload
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            message: payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "tool_call_started" => {
            ask_question_response::Event::ToolCallStarted(AgentToolCallStarted {
                tool_name: payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                arguments_json: payload
                    .get("arguments_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}")
                    .to_string(),
                call_id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                step_id: step_id.clone(),
                step_seq,
            })
        }
        "tool_call_completed" => {
            ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
                tool_name: payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                result_summary: payload
                    .get("result_summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration_ms: payload
                    .get("duration_ms")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(0),
                success: payload
                    .get("success")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
                call_id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                step_id: step_id.clone(),
                step_seq,
            })
        }
        "partial_answer" => ask_question_response::Event::PartialAnswer(AgentPartialAnswer {
            text: payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "thinking" => ask_question_response::Event::Thinking(AgentThinking {
            text: payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            part_index: payload
                .get("part_index")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0) as i32,
            step_id,
            step_seq,
        }),
        "artifact_uploaded" => ask_question_response::Event::ArtifactUploaded(
            ps_proto::canonical::prism::v1::AgentArtifactUploaded {
                artifact_id: payload
                    .get("artifact_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                display_name: payload
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                content_type: payload
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream")
                    .to_string(),
                size_bytes: payload
                    .get("size_bytes")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                download_url: String::new(),
            },
        ),
        "token_usage" => ask_question_response::Event::TokenUsage(AgentTokenUsage {
            input_tokens: payload
                .get("input_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
            output_tokens: payload
                .get("output_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
            context_window: payload
                .get("context_window")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
        }),
        "final_answer" => ask_question_response::Event::FinalAnswer(AgentFinalAnswer {
            answer: payload
                .get("answer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            conversation_id: payload
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            supporting_data_json: String::new(),
            prompt_tokens: payload
                .get("prompt_tokens")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
            completion_tokens: payload
                .get("completion_tokens")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
            estimated_cost_usd: 0.0,
            tool_call_count: payload
                .get("tool_call_count")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
            duration_ms: 0,
            artifacts: vec![],
        }),
        "error" => ask_question_response::Event::Error(AgentError {
            message: payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string(),
            retryable: payload
                .get("retryable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        _ => return None,
    };

    Some(AskQuestionResponse {
        event: Some(proto_event),
    })
}

/// Map a database event to a `ResumeStreamResponse` proto.
pub fn map_db_event_to_resume_proto(
    event: &ps_core::repo::reasoning::ConversationEvent,
) -> Option<ResumeStreamResponse> {
    use ps_proto::canonical::prism::v1::resume_stream_response;
    let ask_resp = map_db_event_to_proto(event)?;
    let ask_evt = ask_resp.event?;

    let event = match ask_evt {
        ask_question_response::Event::ToolCallStarted(v) => {
            resume_stream_response::Event::ToolCallStarted(v)
        }
        ask_question_response::Event::ToolCallCompleted(v) => {
            resume_stream_response::Event::ToolCallCompleted(v)
        }
        ask_question_response::Event::PartialAnswer(v) => {
            resume_stream_response::Event::PartialAnswer(v)
        }
        ask_question_response::Event::FinalAnswer(v) => {
            resume_stream_response::Event::FinalAnswer(v)
        }
        ask_question_response::Event::Error(v) => resume_stream_response::Event::Error(v),
        ask_question_response::Event::Thinking(v) => resume_stream_response::Event::Thinking(v),
        ask_question_response::Event::ContainerStatus(v) => {
            resume_stream_response::Event::ContainerStatus(v)
        }
        ask_question_response::Event::ArtifactUploaded(v) => {
            resume_stream_response::Event::ArtifactUploaded(v)
        }
        ask_question_response::Event::TokenUsage(v) => resume_stream_response::Event::TokenUsage(v),
        ask_question_response::Event::ConversationCreated(_) => return None,
    };

    Some(ResumeStreamResponse { event: Some(event) })
}
