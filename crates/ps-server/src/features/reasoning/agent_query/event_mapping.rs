use ps_proto::canonical::prism::v1::{
    AgentContainerStatus, AgentError, AgentFinalAnswer, AgentPartialAnswer, AgentThinking,
    AgentTokenUsage, AgentToolCallCompleted, AgentToolCallStarted, AskQuestionResponse,
    ResumeStreamResponse, ask_question_response,
};

/// Extract a string field from a JSON payload, returning `""` if missing.
fn json_str<'a>(payload: &'a serde_json::Value, key: &str) -> &'a str {
    payload.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Extract an i64 field from a JSON payload, returning 0 if missing.
fn json_i64(payload: &serde_json::Value, key: &str) -> i64 {
    payload
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}

/// Extract an i32 field (stored as i64 in JSON), returning 0 if missing.
fn json_i32(payload: &serde_json::Value, key: &str) -> i32 {
    payload
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0)
}

fn map_container_status(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::ContainerStatus(AgentContainerStatus {
        status: json_str(payload, "status").to_string(),
        message: json_str(payload, "message").to_string(),
    })
}

fn map_tool_call_started(
    payload: &serde_json::Value,
    step_id: String,
    step_seq: i32,
) -> ask_question_response::Event {
    ask_question_response::Event::ToolCallStarted(AgentToolCallStarted {
        tool_name: json_str(payload, "tool_name").to_string(),
        arguments_json: payload
            .get("arguments_json")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string(),
        call_id: json_str(payload, "call_id").to_string(),
        step_id,
        step_seq,
    })
}

fn map_tool_call_completed(
    payload: &serde_json::Value,
    step_id: String,
    step_seq: i32,
) -> ask_question_response::Event {
    ask_question_response::Event::ToolCallCompleted(AgentToolCallCompleted {
        tool_name: json_str(payload, "tool_name").to_string(),
        result_summary: json_str(payload, "result_summary").to_string(),
        duration_ms: json_i32(payload, "duration_ms"),
        success: payload
            .get("success")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        call_id: json_str(payload, "call_id").to_string(),
        step_id,
        step_seq,
    })
}

fn map_partial_answer(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::PartialAnswer(AgentPartialAnswer {
        text: json_str(payload, "text").to_string(),
    })
}

fn map_thinking(
    payload: &serde_json::Value,
    step_id: String,
    step_seq: i32,
) -> ask_question_response::Event {
    ask_question_response::Event::Thinking(AgentThinking {
        text: json_str(payload, "text").to_string(),
        part_index: json_i64(payload, "part_index") as i32,
        step_id,
        step_seq,
    })
}

fn map_artifact_uploaded(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::ArtifactUploaded(
        ps_proto::canonical::prism::v1::AgentArtifactUploaded {
            artifact_id: json_str(payload, "artifact_id").to_string(),
            display_name: json_str(payload, "display_name").to_string(),
            content_type: payload
                .get("content_type")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream")
                .to_string(),
            size_bytes: json_i64(payload, "size_bytes"),
            download_url: String::new(),
        },
    )
}

fn map_token_usage(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::TokenUsage(AgentTokenUsage {
        input_tokens: json_i64(payload, "input_tokens"),
        output_tokens: json_i64(payload, "output_tokens"),
        context_window: json_i64(payload, "context_window"),
    })
}

fn map_final_answer(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::FinalAnswer(AgentFinalAnswer {
        answer: json_str(payload, "answer").to_string(),
        conversation_id: json_str(payload, "conversation_id").to_string(),
        supporting_data_json: String::new(),
        prompt_tokens: json_i32(payload, "prompt_tokens"),
        completion_tokens: json_i32(payload, "completion_tokens"),
        estimated_cost_usd: 0.0,
        tool_call_count: json_i32(payload, "tool_call_count"),
        duration_ms: 0,
        artifacts: vec![],
    })
}

fn map_error(payload: &serde_json::Value) -> ask_question_response::Event {
    ask_question_response::Event::Error(AgentError {
        message: payload
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error")
            .to_string(),
        retryable: payload
            .get("retryable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

/// Map a database event row to a proto `AskQuestionResponse`.
pub fn map_db_event_to_proto(
    event: &ps_core::repo::reasoning::ConversationEvent,
) -> Option<AskQuestionResponse> {
    let payload = &event.payload;
    let step_id = event.step_id.clone().unwrap_or_default();
    let step_seq = event.step_seq.unwrap_or(0);

    let proto_event = match event.event_type.as_str() {
        "container_status" => map_container_status(payload),
        "tool_call_started" => map_tool_call_started(payload, step_id, step_seq),
        "tool_call_completed" => map_tool_call_completed(payload, step_id, step_seq),
        "partial_answer" => map_partial_answer(payload),
        "thinking" => map_thinking(payload, step_id, step_seq),
        "artifact_uploaded" => map_artifact_uploaded(payload),
        "token_usage" => map_token_usage(payload),
        "final_answer" => map_final_answer(payload),
        "error" => map_error(payload),
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
