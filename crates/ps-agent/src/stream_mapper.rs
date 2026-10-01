//! Adapt current `OpenCode` wire events to the SDK's message-part snapshots.
//!
//! The SDK's typed stream discards `message.part.delta`. Subscribe to raw
//! frames and reconstruct snapshots so text and reasoning remain incremental.

use std::collections::{BTreeMap, BTreeSet};

use opencode_sdk::types::event::Event;
use serde_json::Value;

#[derive(Default)]
pub struct StreamDecoder {
    parts: BTreeMap<String, Value>,
    part_indices: BTreeMap<String, usize>,
    user_messages: BTreeSet<String>,
}

impl StreamDecoder {
    /// Decode a frame, preserving stable part identity across snapshots and deltas.
    pub fn decode(&mut self, data: &str) -> Result<Option<Event>, serde_json::Error> {
        let mut event: Value = serde_json::from_str(data)?;

        match event.get("type").and_then(Value::as_str) {
            Some("message.updated") => {
                if event
                    .pointer("/properties/info/role")
                    .and_then(Value::as_str)
                    == Some("user")
                    && let Some(id) = event.pointer("/properties/info/id").and_then(Value::as_str)
                {
                    self.user_messages.insert(id.to_owned());
                }
            }
            Some("message.part.updated") => {
                let Some(props) = event.get_mut("properties") else {
                    return Ok(None);
                };
                if !self.normalize_part(props) {
                    return Ok(None);
                }
            }
            Some("message.part.delta") => {
                let Some(props) = event.get("properties") else {
                    return Ok(None);
                };
                if props.get("field").and_then(Value::as_str) != Some("text") {
                    return Ok(None);
                }
                let Some(part_id) = props.get("partID").and_then(Value::as_str) else {
                    return Ok(None);
                };
                let Some(delta) = props.get("delta").and_then(Value::as_str) else {
                    return Ok(None);
                };
                let Some(snapshot) = self.parts.get_mut(part_id) else {
                    return Ok(None);
                };
                let Some(text_value) = snapshot.pointer_mut("/part/text") else {
                    return Ok(None);
                };
                let Some(text) = text_value.as_str() else {
                    return Ok(None);
                };

                *text_value = Value::String(format!("{text}{delta}"));
                event = serde_json::json!({
                    "type": "message.part.updated",
                    "properties": snapshot,
                });
            }
            _ => {}
        }

        serde_json::from_value(event).map(Some)
    }

    fn normalize_part(&mut self, props: &mut Value) -> bool {
        let message_id = props
            .pointer("/part/messageID")
            .and_then(Value::as_str)
            .or_else(|| props.get("messageID").and_then(Value::as_str))
            .or_else(|| props.get("messageId").and_then(Value::as_str));
        if message_id.is_some_and(|id| self.user_messages.contains(id)) {
            return false;
        }

        if !matches!(
            props.pointer("/part/type").and_then(Value::as_str),
            Some("text" | "reasoning")
        ) {
            return true;
        }

        let Some(part_id) = props
            .pointer("/part/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return true;
        };
        let next_index = self.part_indices.len();
        let index = *self
            .part_indices
            .entry(part_id.clone())
            .or_insert(next_index);

        let Some(object) = props.as_object_mut() else {
            return false;
        };
        object.insert("index".into(), serde_json::json!(index));
        // A completed snapshot replaces the delta accumulation; it must not
        // append the same content a second time.
        self.parts.insert(part_id, props.clone());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_mapper::EventMapper;
    use ps_proto::canonical::prism::v1::ask_question_response;

    fn snapshot(part_id: &str, message_id: &str, kind: &str, text: &str) -> String {
        serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s1",
                "part": {
                    "id": part_id,
                    "sessionID": "s1",
                    "messageID": message_id,
                    "type": kind,
                    "text": text,
                },
                "time": 1000,
            },
        })
        .to_string()
    }

    fn delta(part_id: &str, text: &str) -> String {
        serde_json::json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "s1",
                "messageID": "m1",
                "partID": part_id,
                "field": "text",
                "delta": text,
            },
        })
        .to_string()
    }

    #[test]
    fn text_deltas_stream_before_the_completed_snapshot() {
        let mut decoder = StreamDecoder::default();
        let mut mapper = EventMapper::new();
        let start = decoder
            .decode(&snapshot("p1", "m1", "text", ""))
            .unwrap()
            .unwrap();
        assert!(mapper.map_event(&start).is_none());

        for (wire, expected) in [
            (delta("p1", "Hello"), "Hello"),
            (delta("p1", " world"), "Hello world"),
            (snapshot("p1", "m1", "text", "Hello world"), "Hello world"),
        ] {
            let event = decoder.decode(&wire).unwrap().unwrap();
            let response = mapper.map_event(&event).unwrap();
            let Some(ask_question_response::Event::PartialAnswer(answer)) = response.event else {
                panic!("expected partial answer");
            };
            assert_eq!(answer.text, expected);
        }
    }

    #[test]
    fn reasoning_deltas_keep_identity_and_distinct_parts_keep_their_order() {
        let mut decoder = StreamDecoder::default();
        let mut mapper = EventMapper::new();
        decoder
            .decode(&snapshot("r1", "m1", "reasoning", ""))
            .unwrap();
        let event = decoder
            .decode(&delta("r1", "Checking data"))
            .unwrap()
            .unwrap();
        let response = mapper.map_event(&event).unwrap();
        let Some(ask_question_response::Event::Thinking(thinking)) = response.event else {
            panic!("expected thinking");
        };
        assert_eq!(thinking.text, "Checking data");
        assert_eq!(thinking.part_index, 0);

        let event = decoder.decode(&delta("r1", " now")).unwrap().unwrap();
        let response = mapper.map_event(&event).unwrap();
        let Some(ask_question_response::Event::Thinking(thinking)) = response.event else {
            panic!("expected thinking");
        };
        assert_eq!(thinking.text, "Checking data now");
        assert_eq!(thinking.part_index, 0);

        for wire in [
            snapshot("p2", "m1", "text", "First"),
            snapshot("p3", "m2", "text", "Second"),
        ] {
            let event = decoder.decode(&wire).unwrap().unwrap();
            mapper.map_event(&event).unwrap();
        }
        let event = decoder.decode(&delta("p3", " block")).unwrap().unwrap();
        let response = mapper.map_event(&event).unwrap();
        let Some(ask_question_response::Event::PartialAnswer(answer)) = response.event else {
            panic!("expected partial answer");
        };
        assert_eq!(answer.text, "First\n\nSecond block");
    }

    #[test]
    fn user_message_snapshots_and_deltas_are_not_answers() {
        let mut decoder = StreamDecoder::default();
        let user = serde_json::json!({
            "type": "message.updated",
            "properties": {"info": {"id": "u1", "role": "user", "time": {"created": 1000}}},
        });
        decoder.decode(&user.to_string()).unwrap();
        assert!(
            decoder
                .decode(&snapshot("u-part", "u1", "text", "Question"))
                .unwrap()
                .is_none()
        );
        assert!(decoder.decode(&delta("u-part", "?")).unwrap().is_none());
    }

    #[test]
    fn unknown_parts_fields_and_event_types_are_ignored() {
        let mut decoder = StreamDecoder::default();
        assert!(decoder.decode(&delta("missing", "text")).unwrap().is_none());
        decoder.decode(&snapshot("p1", "m1", "text", "")).unwrap();
        let mut wire: Value = serde_json::from_str(&delta("p1", "ignored")).unwrap();
        *wire.pointer_mut("/properties/field").unwrap() = Value::String("other".into());
        assert!(decoder.decode(&wire.to_string()).unwrap().is_none());
        assert!(matches!(
            decoder
                .decode(r#"{"type":"future.event","properties":{}}"#)
                .unwrap(),
            Some(Event::Unknown)
        ));
        assert!(decoder.decode("invalid json").is_err());
    }
}
