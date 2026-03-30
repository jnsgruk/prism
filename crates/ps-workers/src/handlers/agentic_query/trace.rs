/// Derive the reasoning trace from conversation events.
/// This produces the same structure as the frontend's `deriveSteps()`.
pub fn derive_trace_from_events(
    events: &[ps_core::repo::reasoning::ConversationEvent],
) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;

    // BTreeMap keyed by step_seq for deterministic ordering.
    let mut steps: BTreeMap<i32, serde_json::Value> = BTreeMap::new();

    for event in events {
        let Some(step_seq) = event.step_seq else {
            continue;
        };
        let Some(ref step_id) = event.step_id else {
            continue;
        };

        match event.event_type.as_str() {
            "thinking" => {
                let text = event
                    .payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let part_index = event
                    .payload
                    .get("part_index")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                // Always overwrite — later events have more complete text.
                steps.insert(
                    step_seq,
                    serde_json::json!({
                        "kind": "reasoning",
                        "text": text,
                        "part_index": part_index,
                        "step_id": step_id,
                    }),
                );
            }
            "tool_call_started" => {
                let tool_name = event
                    .payload
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let call_id = event
                    .payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = event
                    .payload
                    .get("arguments_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                steps
                    .entry(step_seq)
                    .and_modify(|step| {
                        // Update arguments if the new event has non-empty args
                        // (e.g. Running event may have args that Pending lacked).
                        if args != "{}"
                            && let Some(obj) = step.as_object_mut()
                        {
                            obj.insert("arguments".into(), serde_json::json!(args));
                        }
                    })
                    .or_insert_with(|| {
                        serde_json::json!({
                            "kind": "tool",
                            "tool_name": tool_name,
                            "call_id": call_id,
                            "arguments": args,
                            "step_id": step_id,
                        })
                    });
            }
            "tool_call_completed" => {
                if let Some(step) = steps.get_mut(&step_seq)
                    && let Some(obj) = step.as_object_mut()
                {
                    obj.insert(
                        "result_summary".into(),
                        event
                            .payload
                            .get("result_summary")
                            .cloned()
                            .unwrap_or_default(),
                    );
                    obj.insert(
                        "duration_ms".into(),
                        event
                            .payload
                            .get("duration_ms")
                            .cloned()
                            .unwrap_or_default(),
                    );
                    obj.insert(
                        "success".into(),
                        event
                            .payload
                            .get("success")
                            .cloned()
                            .unwrap_or(serde_json::Value::Bool(true)),
                    );
                }
            }
            _ => {}
        }
    }

    steps.into_values().collect()
}
