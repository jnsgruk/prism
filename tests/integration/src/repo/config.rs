use crate::common::db::RepoTestContext;
use ps_core::models::{AiModel, AiProvider, Platform};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Source CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_source_and_get() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    let settings = serde_json::json!({"org": "canonical"});
    let source = repos
        .config
        .create_source(id, "github", "GitHub", &settings, Some("0 */6 * * *"))
        .await
        .unwrap();

    assert_eq!(source.id.into_inner(), id);
    assert_eq!(source.source_type, Platform::Github);
    assert_eq!(source.name, "GitHub");
    assert!(source.enabled);
    assert_eq!(source.schedule_cron.as_deref(), Some("0 */6 * * *"));

    let fetched = repos.config.get_source(id).await.unwrap().unwrap();
    assert_eq!(fetched.name, "GitHub");

    ctx.teardown().await;
}

#[tokio::test]
async fn create_source_conflict_returns_error() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "dupe", &settings, None)
        .await
        .unwrap();

    let err = repos
        .config
        .create_source(Uuid::now_v7(), "github", "dupe", &settings, None)
        .await;
    assert!(err.is_err());

    ctx.teardown().await;
}

#[tokio::test]
async fn list_sources_ordered_by_name() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(Uuid::now_v7(), "jira", "Zulu Jira", &settings, None)
        .await
        .unwrap();
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "Alpha GitHub", &settings, None)
        .await
        .unwrap();

    let sources = repos.config.list_sources().await.unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].name, "Alpha GitHub");
    assert_eq!(sources[1].name, "Zulu Jira");

    ctx.teardown().await;
}

#[tokio::test]
async fn get_enabled_source_by_name() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    let id = Uuid::now_v7();
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();

    // Enabled by default
    assert!(
        repos
            .config
            .get_enabled_source_by_name("GH")
            .await
            .unwrap()
            .is_some()
    );

    // Disable
    repos.config.update_source_enabled(id, false).await.unwrap();
    assert!(
        repos
            .config
            .get_enabled_source_by_name("GH")
            .await
            .unwrap()
            .is_none()
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn get_enabled_source_by_type() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "GH", &settings, None)
        .await
        .unwrap();

    assert!(
        repos
            .config
            .get_enabled_source_by_type("github")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        repos
            .config
            .get_enabled_source_by_type("jira")
            .await
            .unwrap()
            .is_none()
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn update_source_settings() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    let settings = serde_json::json!({"org": "old"});
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();

    let new_settings = serde_json::json!({"org": "new", "extra": true});
    repos
        .config
        .update_source_settings(id, &new_settings)
        .await
        .unwrap();

    let fetched = repos.config.get_source(id).await.unwrap().unwrap();
    assert_eq!(fetched.settings["org"], "new");
    assert_eq!(fetched.settings["extra"], true);

    ctx.teardown().await;
}

#[tokio::test]
async fn update_source_schedule() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();

    repos
        .config
        .update_source_schedule(id, "0 0 * * *")
        .await
        .unwrap();

    let fetched = repos.config.get_source(id).await.unwrap().unwrap();
    assert_eq!(fetched.schedule_cron.as_deref(), Some("0 0 * * *"));

    ctx.teardown().await;
}

#[tokio::test]
async fn delete_source() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();

    assert!(repos.config.delete_source(id).await.unwrap());
    assert!(repos.config.get_source(id).await.unwrap().is_none());
    // Second delete returns false
    assert!(!repos.config.delete_source(id).await.unwrap());

    ctx.teardown().await;
}

#[tokio::test]
async fn source_exists() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let id = Uuid::now_v7();
    assert!(!repos.config.source_exists(id).await.unwrap());

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();
    assert!(repos.config.source_exists(id).await.unwrap());

    ctx.teardown().await;
}

#[tokio::test]
async fn discourse_source_type_roundtrip() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    let id = Uuid::now_v7();
    repos
        .config
        .create_source(id, "discourse-ubuntu", "Ubuntu Discourse", &settings, None)
        .await
        .unwrap();

    let fetched = repos.config.get_source(id).await.unwrap().unwrap();
    assert_eq!(fetched.source_type, Platform::Discourse("ubuntu".into()));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_and_retrieve_secret() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let source_id = Uuid::now_v7();
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(source_id, "github", "GH", &settings, None)
        .await
        .unwrap();

    let encrypted = b"encrypted-token-data";
    repos
        .config
        .upsert_secret(Uuid::now_v7(), source_id, "api_token", encrypted)
        .await
        .unwrap();

    let retrieved = repos
        .config
        .get_encrypted_secret(source_id, "api_token")
        .await
        .unwrap();
    assert_eq!(retrieved.as_deref(), Some(encrypted.as_slice()));

    ctx.teardown().await;
}

#[tokio::test]
async fn secret_status_shows_which_keys_set() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let source_id = Uuid::now_v7();
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(source_id, "github", "GH", &settings, None)
        .await
        .unwrap();

    // No secrets yet
    let keys = repos.config.list_secret_keys(source_id).await.unwrap();
    assert!(keys.is_empty());

    // Add one
    repos
        .config
        .upsert_secret(Uuid::now_v7(), source_id, "api_token", b"enc")
        .await
        .unwrap();

    let keys = repos.config.list_secret_keys(source_id).await.unwrap();
    assert_eq!(keys, vec!["api_token"]);

    ctx.teardown().await;
}

#[tokio::test]
async fn list_all_secret_keys_grouped() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let settings = serde_json::json!({});
    let s1 = Uuid::now_v7();
    let s2 = Uuid::now_v7();
    repos
        .config
        .create_source(s1, "github", "GH1", &settings, None)
        .await
        .unwrap();
    repos
        .config
        .create_source(s2, "jira", "Jira1", &settings, None)
        .await
        .unwrap();

    repos
        .config
        .upsert_secret(Uuid::now_v7(), s1, "api_token", b"e1")
        .await
        .unwrap();
    repos
        .config
        .upsert_secret(Uuid::now_v7(), s2, "api_token", b"e2")
        .await
        .unwrap();
    repos
        .config
        .upsert_secret(Uuid::now_v7(), s2, "email", b"e3")
        .await
        .unwrap();

    let map = repos.config.list_all_secret_keys().await.unwrap();
    assert_eq!(map.get(&s1).map(Vec::len), Some(1));
    assert_eq!(map.get(&s2).map(Vec::len), Some(2));

    ctx.teardown().await;
}

#[tokio::test]
async fn upsert_secret_overwrites() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let source_id = Uuid::now_v7();
    let settings = serde_json::json!({});
    repos
        .config
        .create_source(source_id, "github", "GH", &settings, None)
        .await
        .unwrap();

    repos
        .config
        .upsert_secret(Uuid::now_v7(), source_id, "api_token", b"old")
        .await
        .unwrap();
    repos
        .config
        .upsert_secret(Uuid::now_v7(), source_id, "api_token", b"new")
        .await
        .unwrap();

    let val = repos
        .config
        .get_encrypted_secret(source_id, "api_token")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(val, b"new");

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Global secrets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn global_secret_crud() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    assert!(
        repos
            .config
            .list_global_secret_keys()
            .await
            .unwrap()
            .is_empty()
    );

    repos
        .config
        .upsert_global_secret(Uuid::now_v7(), "test_secret_key", b"enc")
        .await
        .unwrap();

    let keys = repos.config.list_global_secret_keys().await.unwrap();
    assert_eq!(keys, vec!["test_secret_key"]);

    let val = repos
        .config
        .get_global_secret("test_secret_key")
        .await
        .unwrap();
    assert_eq!(val.as_deref(), Some(b"enc".as_slice()));

    assert!(
        repos
            .config
            .delete_global_secret("test_secret_key")
            .await
            .unwrap()
    );
    assert!(
        !repos
            .config
            .delete_global_secret("test_secret_key")
            .await
            .unwrap()
    );

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn global_settings_crud() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    assert!(
        repos
            .config
            .get_global_setting("ai.budget")
            .await
            .unwrap()
            .is_none()
    );

    repos
        .config
        .set_global_setting("ai.budget", &serde_json::json!(10.0))
        .await
        .unwrap();
    repos
        .config
        .set_global_setting("ai.model", &serde_json::json!("gpt-4"))
        .await
        .unwrap();

    let setting = repos
        .config
        .get_global_setting("ai.budget")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(setting.value, serde_json::json!(10.0));

    let ai_settings = repos.config.list_global_settings("ai.").await.unwrap();
    assert_eq!(ai_settings.len(), 2);

    // Upsert overwrites
    repos
        .config
        .set_global_setting("ai.budget", &serde_json::json!(20.0))
        .await
        .unwrap();
    let updated = repos
        .config
        .get_global_setting("ai.budget")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.value, serde_json::json!(20.0));

    assert!(
        repos
            .config
            .delete_global_setting("ai.budget")
            .await
            .unwrap()
    );
    assert!(
        !repos
            .config
            .delete_global_setting("ai.budget")
            .await
            .unwrap()
    );

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// AI model catalogue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replace_and_list_ai_models() {
    let ctx = RepoTestContext::new().await;
    let repos = &ctx.repos;
    let _pool = &ctx.pool;

    let models = vec![
        AiModel {
            id: "google/gemini-pro".into(),
            provider: AiProvider::Google,
            display_name: "Gemini Pro".into(),
            description: Some("General-purpose".into()),
            context_length: Some(32768),
            input_price: Some(0.00025),
            output_price: Some(0.0005),
            capabilities: vec!["enrichment".into(), "insights".into()],
        },
        AiModel {
            id: "google/gemini-flash".into(),
            provider: AiProvider::Google,
            display_name: "Gemini Flash".into(),
            description: None,
            context_length: Some(1_000_000),
            input_price: Some(0.000075),
            output_price: Some(0.0003),
            capabilities: vec!["enrichment".into()],
        },
    ];

    repos
        .config
        .replace_ai_models("google", &models)
        .await
        .unwrap();

    // List all
    let all = repos.config.list_ai_models(None, None).await.unwrap();
    assert_eq!(all.len(), 2);

    // Filter by capability
    let insights = repos
        .config
        .list_ai_models(None, Some("insights"))
        .await
        .unwrap();
    assert_eq!(insights.len(), 1);
    assert_eq!(insights[0].display_name, "Gemini Pro");

    // Filter by provider
    let google = repos
        .config
        .list_ai_models(Some("google"), None)
        .await
        .unwrap();
    assert_eq!(google.len(), 2);

    // Replace clears old ones
    let updated = vec![AiModel {
        id: "google/gemini-2".into(),
        provider: AiProvider::Google,
        display_name: "Gemini 2".into(),
        description: None,
        context_length: None,
        input_price: None,
        output_price: None,
        capabilities: vec![],
    }];
    repos
        .config
        .replace_ai_models("google", &updated)
        .await
        .unwrap();
    let all = repos.config.list_ai_models(None, None).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].display_name, "Gemini 2");

    ctx.teardown().await;
}
