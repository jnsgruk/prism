//! Discourse REST API client.
//!
//! Each Discourse instance has its own API key and username, stored as
//! encrypted secrets.  Authentication is via the `Api-Key` and `Api-Username`
//! headers.

use serde::Deserialize;
use tracing::debug;

/// Discourse REST API client.
#[derive(Clone)]
pub struct DiscourseClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    api_username: String,
}

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
    pub post_stream: Option<PostStream>,
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

// ---------------------------------------------------------------------------
// Client implementation
// ---------------------------------------------------------------------------

impl DiscourseClient {
    pub fn new(http: reqwest::Client, base_url: &str, api_key: &str, api_username: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            api_username: if api_username.is_empty() {
                "system".to_string()
            } else {
                api_username.to_string()
            },
        }
    }

    /// Apply auth headers to a request builder, if an API key is configured.
    ///
    /// Discourse public endpoints work without authentication (with stricter
    /// rate limits), so we skip the headers when no key is set.
    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            builder
        } else {
            builder
                .header("Api-Key", &self.api_key)
                .header("Api-Username", &self.api_username)
        }
    }

    /// Fetch the latest topics page.
    ///
    /// `page` is 0-indexed.  Returns topics sorted by latest activity.
    pub async fn latest(&self, page: u32) -> Result<LatestResponse, ps_core::Error> {
        let url = format!("{}/latest.json", self.base_url);

        debug!(page, "discourse latest request");

        let req = self
            .http
            .get(&url)
            .query(&[("page", page.to_string()), ("order", "activity".into())])
            .timeout(std::time::Duration::from_secs(30));

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse latest request failed: {e}"))
        })?;

        Self::handle_rate_limit(&resp)?;
        Self::require_success(&resp)?;

        resp.json()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("discourse latest parse error: {e}")))
    }

    /// Fetch full topic detail including posts.
    pub async fn topic(&self, topic_id: i64) -> Result<TopicDetailResponse, ps_core::Error> {
        let url = format!("{}/t/{}.json", self.base_url, topic_id);

        debug!(topic_id, "discourse topic request");

        let req = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(30));

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse topic request failed: {e}"))
        })?;

        Self::handle_rate_limit(&resp)?;
        Self::require_success(&resp)?;

        resp.json()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("discourse topic parse error: {e}")))
    }

    /// Fetch the users who liked a specific post.
    ///
    /// Uses `post_action_type_id=2` (like) on the post-actions endpoint.
    pub async fn post_likers(&self, post_id: i64) -> Result<Vec<PostActionUser>, ps_core::Error> {
        let url = format!("{}/post_action_users.json", self.base_url);

        debug!(post_id, "discourse post_likers request");

        let req = self
            .http
            .get(&url)
            .query(&[
                ("id", post_id.to_string()),
                ("post_action_type_id", "2".to_string()),
            ])
            .timeout(std::time::Duration::from_secs(30));

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse post_likers request failed: {e}"))
        })?;

        Self::handle_rate_limit(&resp)?;
        Self::require_success(&resp)?;

        let body: PostActionUsersResponse = resp.json().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse post_likers parse error: {e}"))
        })?;

        Ok(body.post_action_users)
    }

    /// Fetch all categories for mapping category IDs to names.
    pub async fn categories(&self) -> Result<Vec<Category>, ps_core::Error> {
        let url = format!("{}/categories.json", self.base_url);

        let req = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(30));

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse categories request failed: {e}"))
        })?;

        Self::handle_rate_limit(&resp)?;
        Self::require_success(&resp)?;

        let cats: CategoriesResponse = resp
            .json()
            .await
            .map_err(|e| ps_core::Error::Internal(format!("discourse categories parse: {e}")))?;

        Ok(cats.category_list.categories)
    }

    /// Test the connection by fetching `/about.json`.
    pub async fn test_connection(&self) -> Result<String, ps_core::Error> {
        let url = format!("{}/about.json", self.base_url);

        let req = self.http.get(&url);

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse connection test failed: {e}"))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ps_core::Error::Internal(format!(
                "discourse connection test returned {status}: {body}"
            )));
        }

        let about: AboutResponse = resp.json().await.unwrap_or(AboutResponse {
            about: AboutInfo {
                title: None,
                version: None,
            },
        });

        let title = about.about.title.as_deref().unwrap_or("unknown");
        let version = about.about.version.as_deref().unwrap_or("unknown");

        Ok(format!("Connected to {title} (Discourse {version})"))
    }

    /// Search for a user by email via the admin API.
    ///
    /// Requires an admin-scoped API key. Returns the username if an exact
    /// email match is found.
    pub async fn admin_user_search(&self, email: &str) -> Result<Option<String>, ps_core::Error> {
        let url = format!("{}/admin/users/list/active.json", self.base_url);

        let req = self
            .http
            .get(&url)
            .query(&[("filter", email), ("show_emails", "true")])
            .timeout(std::time::Duration::from_secs(30));

        let resp = self.auth(req).send().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse admin user search failed: {e}"))
        })?;

        Self::handle_rate_limit(&resp)?;

        // 403 means the API key lacks admin scope, 404 means the admin
        // endpoint is not reachable (common on instances with no API key
        // where the route simply doesn't exist for anonymous users).
        // Either way this strategy is unavailable — fall through.
        if matches!(
            resp.status(),
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::NOT_FOUND
        ) {
            return Ok(None);
        }

        Self::require_success(&resp)?;

        let users: Vec<AdminUser> = resp.json().await.map_err(|e| {
            ps_core::Error::Internal(format!("discourse admin user search parse: {e}"))
        })?;

        let email_lower = email.to_lowercase();
        let matched = users.into_iter().find(|u| {
            u.email
                .as_deref()
                .is_some_and(|e| e.to_lowercase() == email_lower)
        });

        Ok(matched.map(|u| u.username))
    }

    /// Look up a user by username via the public endpoint.
    ///
    /// Returns `true` if the user exists (200 response), `false` on 404.
    pub async fn user_exists(&self, username: &str) -> Result<bool, ps_core::Error> {
        let url = format!("{}/u/{}.json", self.base_url, username);

        let req = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(30));

        let resp =
            self.auth(req).send().await.map_err(|e| {
                ps_core::Error::Internal(format!("discourse user lookup failed: {e}"))
            })?;

        Self::handle_rate_limit(&resp)?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }

        Self::require_success(&resp)?;
        Ok(true)
    }

    /// Check for 429 and return a rate-limit error.
    fn handle_rate_limit(resp: &reqwest::Response) -> Result<(), ps_core::Error> {
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            return Err(ps_core::Error::RateLimit {
                retry_after_secs: retry_after,
            });
        }
        Ok(())
    }

    /// Return an error if the response status is not successful.
    ///
    /// Must be called *before* deserializing the body.
    fn require_success(resp: &reqwest::Response) -> Result<(), ps_core::Error> {
        let status = resp.status();
        if !status.is_success() {
            // We can't consume the response here because we don't own it.
            // The caller will get a parse error which is fine — this catches
            // the rate-limit case above.  For other errors, the JSON parse
            // will produce a descriptive error.
            return Err(ps_core::Error::Internal(format!(
                "discourse API returned {status}"
            )));
        }
        Ok(())
    }
}
