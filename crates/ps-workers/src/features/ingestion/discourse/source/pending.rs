use serde::{Deserialize, Serialize};

use super::super::client::Post;

/// Only metadata needed to resume a liker request; post bodies stay out of the cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LikePost {
    pub id: i64,
    pub topic_id: i64,
    pub username: String,
    pub post_number: i32,
    pub created_at: String,
}

impl From<&Post> for LikePost {
    fn from(post: &Post) -> Self {
        Self {
            id: post.id,
            topic_id: post.topic_id,
            username: post.username.clone(),
            post_number: post.post_number,
            created_at: post.created_at.clone(),
        }
    }
}
