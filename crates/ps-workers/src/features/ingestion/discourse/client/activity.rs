//! User activity types are separate from the post-action API's type-2 likes.
use serde::{Deserialize, Deserializer};

use super::{DiscourseClient, Post};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserActionType {
    LikeGiven,
    TopicCreated,
    PostAuthored,
    Other,
}

impl<'de> Deserialize<'de> for UserActionType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match i32::deserialize(deserializer)? {
            1 => Self::LikeGiven,
            4 => Self::TopicCreated,
            5 => Self::PostAuthored,
            _ => Self::Other,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserAction {
    #[serde(default)]
    pub id: Option<i64>,
    pub action_type: UserActionType,
    pub created_at: String,
    pub acting_username: String,
    pub acting_name: Option<String>,
    pub topic_id: i64,
    pub post_id: Option<i64>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub hidden: Option<bool>,
}

#[derive(Deserialize)]
struct UserActionsResponse {
    user_actions: Vec<UserAction>,
}

impl UserAction {
    /// Official Discourse activity responses omit the internal action ID.
    /// This composite uses only immutable event evidence returned by the API.
    pub fn key(&self) -> String {
        let kind = match self.action_type {
            UserActionType::LikeGiven => 1,
            UserActionType::TopicCreated => 4,
            UserActionType::PostAuthored => 5,
            UserActionType::Other => 0,
        };
        format!(
            "{kind}:{}:{}:{}:{}",
            self.topic_id,
            self.post_id.unwrap_or_default(),
            self.acting_username.to_lowercase(),
            self.created_at
        )
    }
    /// Cursor anchors need event identity, without persisting provider strings.
    pub fn fingerprint(&self) -> String {
        ps_core::repo::reasoning::content_hash(&serde_json::Value::String(self.key()))
    }
}

impl DiscourseClient {
    /// Public visibility is controlled by the existing API credentials. A hidden
    /// or unavailable profile is an error, never an empty activity history.
    pub async fn user_actions(
        &self,
        username: &str,
        offset: u64,
        limit: usize,
        fetch_likes: bool,
    ) -> Result<Vec<UserAction>, ps_core::Error> {
        super::validate_discourse_username(username)?;
        let req = self
            .http
            .get(format!("{}/user_actions.json", self.base_url))
            .query(&[
                ("username", username.to_string()),
                ("acting_username", username.to_string()),
                ("filter", if fetch_likes { "1,4,5" } else { "4,5" }.into()),
                ("offset", offset.to_string()),
                ("limit", limit.to_string()),
            ])
            .timeout(std::time::Duration::from_secs(30));
        let response = self
            .auth(req)
            .send()
            .await
            .map_err(|e| Self::request_error("discourse activity request", e))?;
        Self::handle_rate_limit(&response)?;
        Self::require_success(&response)?;
        let body: UserActionsResponse = response
            .json()
            .await
            .map_err(|e| Self::request_error("discourse activity parse", e))?;
        if body.user_actions.len() > limit {
            return Err(ps_core::Error::Validation(
                "Discourse activity response exceeded requested page bound".into(),
            ));
        }
        Ok(body.user_actions)
    }

    /// Fetch a specific post even when it is absent from the topic's initial stream.
    pub async fn post(&self, post_id: i64) -> Result<Post, ps_core::Error> {
        if post_id <= 0 {
            return Err(ps_core::Error::Validation(
                "invalid Discourse post ID".into(),
            ));
        }
        let req = self
            .http
            .get(format!("{}/posts/{post_id}.json", self.base_url))
            .query(&[("include_raw", "1")])
            .timeout(std::time::Duration::from_secs(30));
        let response = self
            .auth(req)
            .send()
            .await
            .map_err(|e| Self::request_error("discourse post request", e))?;
        Self::handle_rate_limit(&response)?;
        Self::require_success(&response)?;
        response
            .json()
            .await
            .map_err(|e| Self::request_error("discourse post parse", e))
    }
}

#[cfg(test)]
mod tests {
    use super::UserActionType;

    #[test]
    fn user_activity_types_do_not_confuse_received_likes_or_notifications_with_authorship() {
        for (number, expected) in [
            (1, UserActionType::LikeGiven),
            (4, UserActionType::TopicCreated),
            (5, UserActionType::PostAuthored),
            (2, UserActionType::Other),
            (6, UserActionType::Other),
            (12, UserActionType::Other),
            (13, UserActionType::Other),
            (11, UserActionType::Other),
        ] {
            assert_eq!(
                serde_json::from_value::<UserActionType>(number.into()).unwrap(),
                expected
            );
        }
    }
}
