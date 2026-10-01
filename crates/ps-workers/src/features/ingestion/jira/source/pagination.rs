//! Constant-memory detection of repeated opaque page tokens (Brent's method).

use ps_core::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TokenCycle {
    anchor: Option<String>,
    power: u64,
    distance: u64,
}

impl Default for TokenCycle {
    fn default() -> Self {
        Self {
            anchor: None,
            power: 1,
            distance: 0,
        }
    }
}

impl TokenCycle {
    pub(super) fn observe(&mut self, token: &str) -> Result<(), Error> {
        if self.anchor.as_deref() == Some(token) {
            return Err(Error::Validation(
                "Jira search repeated a pagination token".into(),
            ));
        }
        if self.anchor.is_none() {
            self.anchor = Some(token.into());
            return Ok(());
        }
        self.distance = self.distance.saturating_add(1);
        if self.distance >= self.power {
            self.anchor = Some(token.into());
            self.power = self.power.saturating_mul(2);
            self.distance = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_fail_with_constant_checkpoint_size() {
        let mut tracker = TokenCycle::default();
        for token in ["A", "B", "A"] {
            tracker.observe(token).unwrap();
        }
        assert!(tracker.observe("B").is_err());
        let mut tracker = TokenCycle::default();
        for token in ["A", "B", "C", "A", "B", "C"] {
            tracker.observe(token).unwrap();
        }
        assert!(tracker.observe("A").is_err());
    }

    #[test]
    fn unique_pages_do_not_accumulate_token_history() {
        let mut tracker = TokenCycle::default();
        for token in 0..10000 {
            tracker.observe(&token.to_string()).unwrap();
        }
        assert!(serde_json::to_string(&tracker).unwrap().len() < 80);
    }
}
