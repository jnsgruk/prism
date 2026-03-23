use crate::define_api_test;
use ps_proto::prism::v1::reasoning_service_client::ReasoningServiceClient;
use ps_proto::prism::v1::{
    AiTaskConfig, DeleteEnrichmentsByTypeRequest, FindSimilarRequest, GetAiSettingsRequest,
    GetCostSummaryRequest, GetEmbeddingStatusRequest, GetEnrichmentPipelineStatusRequest,
    GetEnrichmentsRequest, GetStorageHealthRequest, ListAiModelsRequest,
    RefreshModelCatalogueRequest, SetProviderSecretRequest, UpdateAiSettingsRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

// ---------------------------------------------------------------------------
// GetAiSettings — defaults
// ---------------------------------------------------------------------------

define_api_test!(get_ai_settings_defaults, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetAiSettingsRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_ai_settings(req)
        .await
        .expect("get_ai_settings")
        .into_inner();

    let settings = resp.settings.expect("settings present");
    // Default settings should have provider configs
    assert!(settings.enrichment.is_some());
    assert!(settings.insights.is_some());
    // No provider keys set initially
    assert!(!settings.provider_secret_status["google"]);
    assert!(!settings.provider_secret_status["openrouter"]);
});

// ---------------------------------------------------------------------------
// UpdateAiSettings — round-trip
// ---------------------------------------------------------------------------

define_api_test!(update_ai_settings_round_trip, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    // Update enrichment config
    let mut req = Request::new(UpdateAiSettingsRequest {
        enrichment: Some(AiTaskConfig {
            provider: "google".into(),
            model: "gemini-2.0-flash".into(),
        }),
        insights: None,
        agentic: None,
        embeddings: None,
        budget_cap_usd: Some(50.0),
    });
    auth(&mut req, &token);

    let resp = client
        .update_ai_settings(req)
        .await
        .expect("update_ai_settings")
        .into_inner();

    let settings = resp.settings.expect("settings present");
    let enrichment = settings.enrichment.expect("enrichment config");
    assert_eq!(enrichment.provider, "google");
    assert_eq!(enrichment.model, "gemini-2.0-flash");
    assert_eq!(settings.budget_cap_usd, Some(50.0));

    // Re-read to verify persistence
    let mut req = Request::new(GetAiSettingsRequest {});
    auth(&mut req, &token);
    let resp = client
        .get_ai_settings(req)
        .await
        .expect("get_ai_settings")
        .into_inner();

    let settings = resp.settings.expect("settings present");
    let enrichment = settings.enrichment.expect("enrichment config");
    assert_eq!(enrichment.provider, "google");
    assert_eq!(enrichment.model, "gemini-2.0-flash");
    assert_eq!(settings.budget_cap_usd, Some(50.0));
});

// ---------------------------------------------------------------------------
// SetProviderSecret — stores and reflects in status
// ---------------------------------------------------------------------------

define_api_test!(set_provider_secret_updates_status, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(SetProviderSecretRequest {
        provider: "google".into(),
        secret_value: "test-api-key-12345".into(),
    });
    auth(&mut req, &token);

    client
        .set_provider_secret(req)
        .await
        .expect("set_provider_secret");

    // Verify the status now shows google as configured
    let mut req = Request::new(GetAiSettingsRequest {});
    auth(&mut req, &token);
    let resp = client
        .get_ai_settings(req)
        .await
        .expect("get_ai_settings")
        .into_inner();

    let settings = resp.settings.expect("settings");
    assert!(settings.provider_secret_status["google"]);
    // openrouter still not set
    assert!(!settings.provider_secret_status["openrouter"]);
});

// ---------------------------------------------------------------------------
// SetProviderSecret — unknown provider
// ---------------------------------------------------------------------------

define_api_test!(set_provider_secret_unknown_provider, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(SetProviderSecretRequest {
        provider: "unknown-provider".into(),
        secret_value: "key".into(),
    });
    auth(&mut req, &token);

    let err = client
        .set_provider_secret(req)
        .await
        .expect_err("unknown provider");

    assert_eq!(err.code(), tonic::Code::InvalidArgument);
});

// ---------------------------------------------------------------------------
// SetProviderSecret — empty value rejected
// ---------------------------------------------------------------------------

define_api_test!(set_provider_secret_empty_value, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(SetProviderSecretRequest {
        provider: "google".into(),
        secret_value: String::new(),
    });
    auth(&mut req, &token);

    let err = client
        .set_provider_secret(req)
        .await
        .expect_err("empty secret");

    assert_eq!(err.code(), tonic::Code::InvalidArgument);
});

// ---------------------------------------------------------------------------
// GetEnrichmentPipelineStatus — empty
// ---------------------------------------------------------------------------

define_api_test!(get_enrichment_pipeline_status_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetEnrichmentPipelineStatusRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_enrichment_pipeline_status(req)
        .await
        .expect("get_enrichment_pipeline_status")
        .into_inner();

    assert_eq!(resp.pending_count, 0);
    assert_eq!(resp.total_enrichments, 0);
});

// ---------------------------------------------------------------------------
// GetEmbeddingStatus — empty
// ---------------------------------------------------------------------------

define_api_test!(get_embedding_status_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetEmbeddingStatusRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_embedding_status(req)
        .await
        .expect("get_embedding_status")
        .into_inner();

    assert_eq!(resp.queued_count, 0);
    assert_eq!(resp.embedded_count, 0);
    assert_eq!(resp.total_eligible, 0);
    assert_eq!(resp.coverage_percent, 0.0);
});

// ---------------------------------------------------------------------------
// ListAiModels — empty catalogue
// ---------------------------------------------------------------------------

define_api_test!(list_ai_models_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(ListAiModelsRequest {
        provider: String::new(),
        capability: String::new(),
    });
    auth(&mut req, &token);

    let resp = client
        .list_ai_models(req)
        .await
        .expect("list_ai_models")
        .into_inner();

    assert!(resp.models.is_empty());
});

// ---------------------------------------------------------------------------
// GetEnrichments — no enrichments for a random contribution
// ---------------------------------------------------------------------------

define_api_test!(get_enrichments_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetEnrichmentsRequest {
        contribution_id: uuid::Uuid::now_v7().to_string(),
    });
    auth(&mut req, &token);

    let resp = client
        .get_enrichments(req)
        .await
        .expect("get_enrichments")
        .into_inner();

    assert!(resp.enrichments.is_empty());
});

// ---------------------------------------------------------------------------
// GetCostSummary — empty
// ---------------------------------------------------------------------------

define_api_test!(get_cost_summary_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetCostSummaryRequest { days: 7 });
    auth(&mut req, &token);

    let resp = client
        .get_cost_summary(req)
        .await
        .expect("get_cost_summary")
        .into_inner();

    assert_eq!(resp.today_spend_usd, 0.0);
    assert!(resp.task_breakdown.is_empty());
    assert!(resp.model_breakdown.is_empty());
});

// ---------------------------------------------------------------------------
// GetStorageHealth — no artifact store configured
// ---------------------------------------------------------------------------

define_api_test!(get_storage_health_no_store, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetStorageHealthRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_storage_health(req)
        .await
        .expect("get_storage_health")
        .into_inner();

    assert!(!resp.healthy);
    assert!(!resp.error_message.is_empty());
});

// ---------------------------------------------------------------------------
// RefreshModelCatalogue — fires but doesn't crash (no Restate in tests)
// ---------------------------------------------------------------------------

define_api_test!(refresh_model_catalogue_returns, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(RefreshModelCatalogueRequest {});
    auth(&mut req, &token);

    let resp = client
        .refresh_model_catalogue(req)
        .await
        .expect("refresh_model_catalogue")
        .into_inner();

    // No Restate running → started should be false
    assert!(!resp.started);
});

// ---------------------------------------------------------------------------
// FindSimilar — empty (no embeddings)
// ---------------------------------------------------------------------------

define_api_test!(find_similar_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    // Create a contribution to search against
    let contribution_id = uuid::Uuid::now_v7();
    let item = ps_core::ingestion::ContributionInput {
        platform: ps_core::models::Platform::Github,
        contribution_type: ps_core::models::ContributionType::PullRequest,
        platform_id: "SIM-1".into(),
        platform_username: "user".into(),
        title: Some("Test PR".into()),
        url: None,
        state: Some(ps_core::models::ContributionState::Merged),
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    };

    repos
        .activity
        .upsert_contribution(contribution_id, None, &item)
        .await
        .unwrap();

    let mut req = Request::new(FindSimilarRequest {
        contribution_id: contribution_id.to_string(),
        limit: 5,
        platform: None,
    });
    auth(&mut req, &token);

    let resp = client
        .find_similar(req)
        .await
        .expect("find_similar")
        .into_inner();

    assert!(resp.items.is_empty());
});

// ---------------------------------------------------------------------------
// DeleteEnrichmentsByType — empty type rejected
// ---------------------------------------------------------------------------

define_api_test!(delete_enrichments_by_type_empty_type, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let mut req = Request::new(DeleteEnrichmentsByTypeRequest {
        enrichment_type: String::new(),
    });
    auth(&mut req, &token);

    let err = client
        .delete_enrichments_by_type(req)
        .await
        .expect_err("empty type should fail");

    assert_eq!(err.code(), tonic::Code::InvalidArgument);
});

// ---------------------------------------------------------------------------
// Requires auth
// ---------------------------------------------------------------------------

define_api_test!(reasoning_requires_auth, |server| async move {
    let mut client = ReasoningServiceClient::new(server.channel.clone());

    let err = client
        .get_ai_settings(GetAiSettingsRequest {})
        .await
        .expect_err("should require auth");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});
