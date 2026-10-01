use crate::common::server::ApiTestContext;
use ps_proto::canonical::prism::v1::handlers_service_client::HandlersServiceClient;
use ps_proto::canonical::prism::v1::{GetPipelineStatusRequest, TriggerPipelineRequest};
use tonic::Request;
use tonic::metadata::MetadataValue;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

#[tokio::test]
async fn pipeline_reservation_serializes_global_admission_and_is_idempotent() {
    let ctx = ApiTestContext::new().await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let snapshot = serde_json::json!({
        "scope": {"kind": "all"}, "sources": [], "since_date": null,
        "run_started_at": time::OffsetDateTime::now_utc(), "processing": {"kind": "all"}
    });
    let mut person_snapshot = snapshot.clone();
    person_snapshot["scope"] =
        serde_json::json!({"kind":"person", "person_id":uuid::Uuid::now_v7()});
    let first_id = uuid::Uuid::now_v7();
    let second_id = uuid::Uuid::now_v7();
    let (first, second) = tokio::join!(
        repos
            .activity
            .reserve_pipeline(first_id, &snapshot, caller, "admin"),
        repos
            .activity
            .reserve_pipeline(second_id, &person_snapshot, caller, "admin"),
    );
    let admitted = match (first, second) {
        (Ok(pipeline), Err(ps_core::Error::Conflict(_)))
        | (Err(ps_core::Error::Conflict(_)), Ok(pipeline)) => pipeline,
        _ => panic!("exactly one reservation must succeed"),
    };
    assert_eq!(admitted.status, "pending");
    assert!(admitted.request_snapshot == snapshot || admitted.request_snapshot == person_snapshot);
    let initialized = repos
        .activity
        .create_pipeline(admitted.id, Some("inv_original"))
        .await
        .unwrap();
    let replayed = repos
        .activity
        .create_pipeline(admitted.id, Some("inv_original"))
        .await
        .unwrap();
    assert_eq!(initialized.id, replayed.id);
    assert_eq!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .len(),
        1
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn trigger_pipeline_returns_queryable_id_and_reuses_submission() {
    let ctx = ApiTestContext::new().await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = HandlersServiceClient::new(ctx.server.channel.clone());
    let submission_id = uuid::Uuid::now_v7().to_string();
    for _ in 0..2 {
        let mut request = Request::new(TriggerPipelineRequest {
            submission_id: Some(submission_id.clone()),
            ..Default::default()
        });
        auth(&mut request, &token);
        let response = client.trigger_pipeline(request).await.unwrap().into_inner();
        assert_eq!(response.pipeline_id, submission_id);
    }
    let mut request = Request::new(GetPipelineStatusRequest {
        pipeline_id: Some(submission_id.clone()),
        ..Default::default()
    });
    auth(&mut request, &token);
    let response = client
        .get_pipeline_status(request)
        .await
        .unwrap()
        .into_inner();
    let current = response.current.unwrap();
    assert_eq!(current.id, submission_id);
    assert_eq!(current.status, "pending");
    assert_eq!(current.scope_kind, "all");
    ctx.teardown().await;
}

#[tokio::test]
async fn invalid_dates_and_identifiers_never_reserve_pipeline() {
    let ctx = ApiTestContext::new().await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = HandlersServiceClient::new(ctx.server.channel.clone());
    for request in [
        TriggerPipelineRequest {
            since_date: Some("2026-02-30".into()),
            ..Default::default()
        },
        TriggerPipelineRequest {
            since_date: Some("9999-01-01".into()),
            ..Default::default()
        },
        TriggerPipelineRequest {
            submission_id: Some("invalid".into()),
            ..Default::default()
        },
        TriggerPipelineRequest {
            scope: Some(ps_proto::canonical::prism::v1::PersonBackfillScope {
                person_id: "invalid".into(),
                source_ids: vec![],
            }),
            ..Default::default()
        },
    ] {
        let mut request = Request::new(request);
        auth(&mut request, &token);
        assert_eq!(
            client.trigger_pipeline(request).await.unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    assert!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .is_empty()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn durable_dispatch_retries_same_workflow_after_response_loss() {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let ctx = ApiTestContext::new().await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let server = MockServer::start().await;
    let id = uuid::Uuid::now_v7();
    let snapshot = serde_json::json!({
        "scope": {"kind": "all"}, "sources": [], "since_date": null,
        "run_started_at": time::OffsetDateTime::now_utc(), "processing": {"kind": "all"}
    });
    repos
        .activity
        .reserve_pipeline(id, &snapshot, caller, "admin")
        .await
        .unwrap();
    let workflow_path = format!("/ScopedIngestionPipelineWorkflow/{id}/run/send");
    Mock::given(method("POST"))
        .and(path(&workflow_path))
        .respond_with(ResponseTemplate::new(202).set_body_string("lost response"))
        .expect(1)
        .mount(&server)
        .await;
    let service = ps_server::features::dispatch::HandlersServiceImpl::new(
        repos.clone(),
        server.uri(),
        server.uri(),
    );
    service.recover_pipeline_dispatch().await.unwrap();
    let pending = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pending.status, "pending");
    assert!(!pending.dispatch_acknowledged);
    server.verify().await;
    server.reset().await;
    Mock::given(method("POST"))
        .and(path(&workflow_path))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(serde_json::json!({"invocationId": "inv_original"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    // PostgreSQL owns the durable lease clock, so let it expire in real time.
    tokio::time::sleep(std::time::Duration::from_secs(31)).await;
    // A new service instance represents recovery after a server restart.
    let restarted = ps_server::features::dispatch::HandlersServiceImpl::new(
        repos.clone(),
        server.uri(),
        server.uri(),
    );
    restarted.recover_pipeline_dispatch().await.unwrap();
    let recovered = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert!(recovered.dispatch_acknowledged);
    assert_eq!(recovered.request_snapshot, snapshot);
    assert_eq!(
        recovered.current_invocation_id.as_deref(),
        Some("inv_original")
    );
    assert_eq!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .len(),
        1
    );
    server.verify().await;
    ctx.teardown().await;
}

#[tokio::test]
async fn definitive_dispatch_rejection_releases_admission() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
    let ctx = ApiTestContext::new().await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let snapshot = serde_json::json!({"scope": {"kind":"all"}});
    let id = uuid::Uuid::now_v7();
    repos
        .activity
        .reserve_pipeline(id, &snapshot, caller, "admin")
        .await
        .unwrap();
    let service = ps_server::features::dispatch::HandlersServiceImpl::new(
        repos.clone(),
        server.uri(),
        server.uri(),
    );
    service.recover_pipeline_dispatch().await.unwrap();
    assert_eq!(
        repos
            .activity
            .get_pipeline(id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
    repos
        .activity
        .reserve_pipeline(uuid::Uuid::now_v7(), &snapshot, caller, "admin")
        .await
        .unwrap();
    server.verify().await;
    ctx.teardown().await;
}

#[tokio::test]
async fn person_backfill_preflight_rejects_invalid_bindings_and_safe_capability_gate() {
    use ps_core::models::Platform;
    use ps_proto::canonical::prism::v1::PersonBackfillScope;
    let ctx = ApiTestContext::new().await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let person_id = crate::common::fixtures::create_person_with_identity(
        &ctx.server.pool,
        "Saved Person",
        &Platform::Github,
        "saved-user",
    )
    .await;
    let github = uuid::Uuid::now_v7();
    let jira = uuid::Uuid::now_v7();
    let discourse = uuid::Uuid::now_v7();
    repos
        .config
        .create_source(
            github,
            &Platform::Github.to_string(),
            "GitHub",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    repos
        .config
        .create_source(
            jira,
            &Platform::Jira.to_string(),
            "Jira",
            &serde_json::json!({"api_mode":"server"}),
            None,
        )
        .await
        .unwrap();
    repos
        .config
        .create_source(
            discourse,
            &Platform::Discourse("exact".into()).to_string(),
            "Discourse",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    let mut client = HandlersServiceClient::new(ctx.server.channel.clone());
    for (person, source_ids, expected_code, expected_message) in [
        (
            uuid::Uuid::now_v7(),
            vec![github.to_string()],
            tonic::Code::FailedPrecondition,
            "active",
        ),
        (
            person_id,
            vec![],
            tonic::Code::InvalidArgument,
            "at least one",
        ),
        (
            person_id,
            vec![github.to_string(), github.to_string()],
            tonic::Code::InvalidArgument,
            "duplicate",
        ),
        (
            person_id,
            vec!["invalid".into()],
            tonic::Code::InvalidArgument,
            "source_id",
        ),
        (
            person_id,
            vec![uuid::Uuid::now_v7().to_string()],
            tonic::Code::FailedPrecondition,
            "enabled",
        ),
        (
            person_id,
            vec![jira.to_string()],
            tonic::Code::FailedPrecondition,
            "Cloud",
        ),
        (
            person_id,
            vec![discourse.to_string()],
            tonic::Code::FailedPrecondition,
            "saved identity",
        ),
        (
            person_id,
            vec![github.to_string()],
            tonic::Code::FailedPrecondition,
            "#30",
        ),
    ] {
        let mut request = Request::new(TriggerPipelineRequest {
            scope: Some(PersonBackfillScope {
                person_id: person.to_string(),
                source_ids,
            }),
            since_date: Some("2020-01-01".into()),
            ..Default::default()
        });
        auth(&mut request, &token);
        let error = client.trigger_pipeline(request).await.unwrap_err();
        assert_eq!(error.code(), expected_code);
        assert!(
            error.message().contains(expected_message),
            "{}",
            error.message()
        );
    }
    for mode in [serde_json::json!(false), serde_json::json!("cloud")] {
        repos
            .config
            .update_source_settings(jira, &serde_json::json!({"api_mode":mode}))
            .await
            .unwrap();
        let mut request = Request::new(TriggerPipelineRequest {
            scope: Some(PersonBackfillScope {
                person_id: person_id.to_string(),
                source_ids: vec![jira.to_string()],
            }),
            since_date: Some("2020-01-01".into()),
            ..Default::default()
        });
        auth(&mut request, &token);
        let error = client.trigger_pipeline(request).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    }
    repos
        .config
        .update_source_enabled(github, false)
        .await
        .unwrap();
    let mut disabled = Request::new(TriggerPipelineRequest {
        scope: Some(PersonBackfillScope {
            person_id: person_id.to_string(),
            source_ids: vec![github.to_string()],
        }),
        since_date: Some("2020-01-01".into()),
        ..Default::default()
    });
    auth(&mut disabled, &token);
    assert_eq!(
        client.trigger_pipeline(disabled).await.unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );
    let unauthenticated = client
        .trigger_pipeline(TriggerPipelineRequest::default())
        .await
        .unwrap_err();
    assert_eq!(unauthenticated.code(), tonic::Code::Unauthenticated);
    assert!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .is_empty()
    );
    repos.org.deactivate_person(person_id).await.unwrap();
    let mut inactive = Request::new(TriggerPipelineRequest {
        scope: Some(PersonBackfillScope {
            person_id: person_id.to_string(),
            source_ids: vec![github.to_string()],
        }),
        since_date: Some("2020-01-01".into()),
        ..Default::default()
    });
    auth(&mut inactive, &token);
    let error = client.trigger_pipeline(inactive).await.unwrap_err();
    assert!(error.message().contains("active"));
    let legacy_jira_person = crate::common::fixtures::create_person_with_identity(
        &ctx.server.pool,
        "Legacy Jira Person",
        &Platform::Jira,
        "legacy-display-name",
    )
    .await;
    let mut missing_jira_id = Request::new(TriggerPipelineRequest {
        scope: Some(PersonBackfillScope {
            person_id: legacy_jira_person.to_string(),
            source_ids: vec![jira.to_string()],
        }),
        since_date: Some("2020-01-01".into()),
        ..Default::default()
    });
    auth(&mut missing_jira_id, &token);
    let error = client.trigger_pipeline(missing_jira_id).await.unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    assert!(error.message().contains("account ID"));
    ctx.teardown().await;
}
