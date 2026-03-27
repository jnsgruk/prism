use std::collections::HashMap;

/// Assigns stable identities and display ordering to agentic query steps.
///
/// Each logical step (reasoning block or tool call) gets a unique `step_id`
/// and a monotonically increasing `step_seq`. Updates to existing steps
/// (cumulative thinking text, tool completion) reuse the original identity.
#[derive(Default)]
pub struct StepRegistry {
    next_seq: i32,
    steps: HashMap<String, i32>,
    thinking: HashMap<i32, ThinkingState>,
}

struct ThinkingState {
    generation: u32,
    text_prefix: String,
}

/// Identity assigned to an event.
pub struct StepIdentity {
    pub step_id: String,
    pub step_seq: i32,
}

impl StepRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign identity for a thinking event. Returns existing identity if
    /// this is a cumulative update to the same reasoning block, or creates
    /// a new step if the `part_index` has been recycled.
    pub fn thinking_step(&mut self, part_index: i32, text: &str) -> StepIdentity {
        if let Some(state) = self.thinking.get(&part_index) {
            let is_continuation =
                text.starts_with(&state.text_prefix) || state.text_prefix.starts_with(text);

            if is_continuation {
                let step_id = format!("think-{part_index}-{}", state.generation);
                if let Some(&step_seq) = self.steps.get(&step_id) {
                    // Update prefix to latest text (in case it grew).
                    if text.len() > state.text_prefix.len()
                        && let Some(s) = self.thinking.get_mut(&part_index)
                    {
                        s.text_prefix = text.chars().take(80).collect();
                    }
                    return StepIdentity { step_id, step_seq };
                }
            }
            // Recycled part_index or missing step — fall through to create new generation.
        }

        let generation = self
            .thinking
            .get(&part_index)
            .map_or(0, |s| s.generation + 1);

        self.thinking.insert(
            part_index,
            ThinkingState {
                generation,
                text_prefix: text.chars().take(80).collect(),
            },
        );

        let step_id = format!("think-{part_index}-{generation}");
        let step_seq = self.next_seq;
        self.next_seq += 1;
        self.steps.insert(step_id.clone(), step_seq);
        StepIdentity { step_id, step_seq }
    }

    /// Assign identity for a tool call started event.
    /// Idempotent — repeated calls for the same `call_id` (e.g. Pending then
    /// Running updates from `OpenCode`) reuse the original identity.
    pub fn tool_started(&mut self, call_id: &str) -> StepIdentity {
        let step_id = format!("tool-{call_id}");
        if let Some(&step_seq) = self.steps.get(&step_id) {
            return StepIdentity { step_id, step_seq };
        }
        let step_seq = self.next_seq;
        self.next_seq += 1;
        self.steps.insert(step_id.clone(), step_seq);
        StepIdentity { step_id, step_seq }
    }

    /// Assign identity for a tool call completed event.
    /// Reuses the `step_seq` from the started event if it exists.
    pub fn tool_completed(&mut self, call_id: &str) -> StepIdentity {
        let step_id = format!("tool-{call_id}");
        if let Some(&step_seq) = self.steps.get(&step_id) {
            StepIdentity { step_id, step_seq }
        } else {
            // Started event was missed — assign new seq.
            let step_seq = self.next_seq;
            self.next_seq += 1;
            self.steps.insert(step_id.clone(), step_seq);
            StepIdentity { step_id, step_seq }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_thinking_block_gets_identity() {
        let mut reg = StepRegistry::new();
        let id = reg.thinking_step(0, "I think we should");
        assert_eq!(id.step_id, "think-0-0");
        assert_eq!(id.step_seq, 0);
    }

    #[test]
    fn cumulative_thinking_reuses_identity() {
        let mut reg = StepRegistry::new();
        let id1 = reg.thinking_step(0, "I think");
        let id2 = reg.thinking_step(0, "I think we should");
        assert_eq!(id1.step_id, id2.step_id);
        assert_eq!(id1.step_seq, id2.step_seq);
    }

    #[test]
    fn recycled_part_index_gets_new_generation() {
        let mut reg = StepRegistry::new();
        let id1 = reg.thinking_step(0, "First thought");
        // Tool call between thinking blocks advances seq.
        let _ = reg.tool_started("tool-1");
        let id2 = reg.thinking_step(0, "Completely different thought");
        assert_eq!(id1.step_id, "think-0-0");
        assert_eq!(id2.step_id, "think-0-1");
        assert_ne!(id1.step_seq, id2.step_seq);
    }

    #[test]
    fn tool_started_and_completed_share_identity() {
        let mut reg = StepRegistry::new();
        let started = reg.tool_started("abc-123");
        let completed = reg.tool_completed("abc-123");
        assert_eq!(started.step_id, completed.step_id);
        assert_eq!(started.step_seq, completed.step_seq);
    }

    #[test]
    fn tool_started_is_idempotent() {
        let mut reg = StepRegistry::new();
        let first = reg.tool_started("abc-123");
        let second = reg.tool_started("abc-123");
        assert_eq!(first.step_id, second.step_id);
        assert_eq!(first.step_seq, second.step_seq);
        // Next tool still gets the next seq.
        let other = reg.tool_started("def-456");
        assert_eq!(other.step_seq, first.step_seq + 1);
    }

    #[test]
    fn tool_completed_without_started_gets_new_seq() {
        let mut reg = StepRegistry::new();
        let completed = reg.tool_completed("orphan");
        assert_eq!(completed.step_id, "tool-orphan");
        assert_eq!(completed.step_seq, 0);
    }

    #[test]
    fn interleaved_thinking_and_tools_preserve_order() {
        let mut reg = StepRegistry::new();
        let t0 = reg.thinking_step(0, "Thinking about query");
        let tool1 = reg.tool_started("call-1");
        let t0_update = reg.thinking_step(0, "Thinking about query structure");
        let tool1_done = reg.tool_completed("call-1");
        let t1 = reg.thinking_step(1, "Now analyzing results");

        // Thinking update keeps original seq.
        assert_eq!(t0.step_seq, t0_update.step_seq);
        // Tool keeps its seq between started/completed.
        assert_eq!(tool1.step_seq, tool1_done.step_seq);
        // Order: think-0 (0) < tool-1 (1) < think-1 (2).
        assert!(t0.step_seq < tool1.step_seq);
        assert!(tool1.step_seq < t1.step_seq);
    }

    #[test]
    fn seq_is_monotonically_increasing() {
        let mut reg = StepRegistry::new();
        let ids: Vec<i32> = (0..5)
            .map(|i| reg.tool_started(&format!("call-{i}")).step_seq)
            .collect();
        for window in ids.windows(2) {
            assert!(window[0] < window[1]);
        }
    }
}
