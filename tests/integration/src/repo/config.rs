use crate::define_repo_test;
use ps_core::models::{AiModel, AiProvider, Platform};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Source CRUD
// ---------------------------------------------------------------------------

define_repo_test!(create_source_and_get, |repos, _pool| async move {
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
});

define_repo_test!(
    create_source_conflict_returns_error,
    |repos, _pool| async move {
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
    }
);

define_repo_test!(list_sources_ordered_by_name, |repos, _pool| async move {
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
});

define_repo_test!(get_enabled_source_by_name, |repos, _pool| async move {
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
});

define_repo_test!(get_enabled_source_by_type, |repos, _pool| async move {
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
});

define_repo_test!(update_source_settings, |repos, _pool| async move {
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
});

define_repo_test!(update_source_schedule, |repos, _pool| async move {
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
});

define_repo_test!(delete_source, |repos, _pool| async move {
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
});

define_repo_test!(source_exists, |repos, _pool| async move {
    let id = Uuid::now_v7();
    assert!(!repos.config.source_exists(id).await.unwrap());

    let settings = serde_json::json!({});
    repos
        .config
        .create_source(id, "github", "GH", &settings, None)
        .await
        .unwrap();
    assert!(repos.config.source_exists(id).await.unwrap());
});

define_repo_test!(discourse_source_type_roundtrip, |repos, _pool| async move {
    let settings = serde_json::json!({});
    let id = Uuid::now_v7();
    repos
        .config
        .create_source(id, "discourse-ubuntu", "Ubuntu Discourse", &settings, None)
        .await
        .unwrap();

    let fetched = repos.config.get_source(id).await.unwrap().unwrap();
    assert_eq!(fetched.source_type, Platform::Discourse("ubuntu".into()));
});

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

define_repo_test!(store_and_retrieve_secret, |repos, _pool| async move {
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
});

define_repo_test!(
    secret_status_shows_which_keys_set,
    |repos, _pool| async move {
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
    }
);

define_repo_test!(list_all_secret_keys_grouped, |repos, _pool| async move {
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
});

define_repo_test!(upsert_secret_overwrites, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// Global secrets
// ---------------------------------------------------------------------------

define_repo_test!(global_secret_crud, |repos, _pool| async move {
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
        .upsert_global_secret(Uuid::now_v7(), "openrouter_key", b"enc")
        .await
        .unwrap();

    let keys = repos.config.list_global_secret_keys().await.unwrap();
    assert_eq!(keys, vec!["openrouter_key"]);

    let val = repos
        .config
        .get_global_secret("openrouter_key")
        .await
        .unwrap();
    assert_eq!(val.as_deref(), Some(b"enc".as_slice()));

    assert!(
        repos
            .config
            .delete_global_secret("openrouter_key")
            .await
            .unwrap()
    );
    assert!(
        !repos
            .config
            .delete_global_secret("openrouter_key")
            .await
            .unwrap()
    );
});

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

define_repo_test!(global_settings_crud, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// AI model catalogue
// ---------------------------------------------------------------------------

define_repo_test!(replace_and_list_ai_models, |repos, _pool| async move {
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
});

// ---------------------------------------------------------------------------
// Backup helpers
// ---------------------------------------------------------------------------

define_repo_test!(count_and_export_sources, |repos, _pool| async move {
    assert_eq!(repos.config.count_sources().await.unwrap(), 0);

    let settings = serde_json::json!({"org": "test"});
    repos
        .config
        .create_source(Uuid::now_v7(), "github", "GH", &settings, None)
        .await
        .unwrap();

    assert_eq!(repos.config.count_sources().await.unwrap(), 1);
    let exported = repos.config.export_sources().await.unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0]["name"], "GH");
});
