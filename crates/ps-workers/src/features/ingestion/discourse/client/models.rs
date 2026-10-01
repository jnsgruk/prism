//! Typed Discourse topic, post, category and user response payloads.
use serde::Deserialize;

// ---------------------------------------------------------------------------
// /latest.json response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LatestResponse {
    pub topic_list: TopicList,
}

#[derive(Debug, Deserialize)]
pub struct TopicList {
    pub topics: Vec<TopicSummary>,
    /// Present when there are more pages.  Absent on the last page.
    pub more_topics_url: Option<String>,
}

/// Lightweight topic metadata returned by `/latest.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct TopicSummary {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub posts_count: i32,
    pub views: i32,
    pub category_id: Option<i64>,
    pub created_at: String,
    pub bumped_at: Option<String>,
    pub last_posted_at: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub has_accepted_answer: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// /t/{id}.json response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TopicDetailResponse {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub posts_count: i32,
    pub views: i32,
    pub category_id: Option<i64>,
    pub created_at: String,
    pub bumped_at: Option<String>,
    #[serde(default)]
    pub has_accepted_answer: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub post_stream: Option<PostStream>,
    #[serde(default)]
    pub archetype: Option<String>,
    #[serde(default)]
    pub visible: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PostStream {
    pub posts: Vec<Post>,
}

/// A single post within a topic.
#[derive(Debug, Clone, Deserialize)]
pub struct Post {
    pub id: i64,
    pub topic_id: i64,
    pub username: String,
    pub name: Option<String>,
    pub post_number: i32,
    pub reply_count: i32,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub deleted_at: Option<String>,
    #[serde(default)]
    like_count: Option<i32>,
    /// `actions_summary` contains per-action-type counts; action type 2 = like.
    #[serde(default)]
    actions_summary: Vec<ActionSummary>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub raw: Option<String>,
    /// If this post is a reply, the post number it replies to.
    #[serde(default)]
    pub reply_to_post_number: Option<i32>,
}

impl Post {
    /// Effective like count: prefer `like_count` when present, fall back to
    /// `actions_summary` type-2 count (some Discourse instances return
    /// `like_count: null`).
    pub fn likes(&self) -> i32 {
        self.like_count.filter(|&c| c > 0).unwrap_or_else(|| {
            self.actions_summary
                .iter()
                .find(|a| a.id == 2)
                .map_or(0, |a| a.count)
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ActionSummary {
    id: i32,
    #[serde(default)]
    count: i32,
}

// ---------------------------------------------------------------------------
// /post_action_users.json response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PostActionUsersResponse {
    pub post_action_users: Vec<PostActionUser>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostActionUser {
    pub id: i64,
    pub username: String,
    pub name: Option<String>,
}

// ---------------------------------------------------------------------------
// /categories.json response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CategoriesResponse {
    pub category_list: CategoryList,
}

#[derive(Debug, Deserialize)]
pub struct CategoryList {
    pub categories: Vec<Category>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub slug: String,
}

// ---------------------------------------------------------------------------
// /about.json response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AboutResponse {
    pub about: AboutInfo,
}

#[derive(Debug, Deserialize)]
pub struct AboutInfo {
    pub title: Option<String>,
    pub version: Option<String>,
}

// ---------------------------------------------------------------------------
// Admin API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AdminUser {
    pub username: String,
    pub email: Option<String>,
}
