//! Rejected API requests must not reserve admission or send work to Restate.
use ps_core::models::Platform;
use ps_core::repo::Repos;
use ps_core::repo::org::{CreatePersonParams, IdentityInput};
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersService;
use ps_proto::canonical::prism::v1::{PersonBackfillScope, TriggerPipelineRequest};
use ps_server::features::dispatch::HandlersServiceImpl;
use ps_server::interceptor::AuthContext;
use tonic::{Code, Request};
use uuid::Uuid;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

use crate::common::server::ApiTestContext;

#[tokio::test]
async fn rejected_person_preflight_and_missing_auth_dispatch_nothing() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller_id, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let person = repos
        .org
        .create_person(CreatePersonParams {
            name: "Preflight Person".into(),
            email: None,
            level: None,
            team_id: None,
            identities: vec![
                IdentityInput {
                    platform: Platform::Github,
                    username: "saved-user".into(),
                    platform_user_id: None,
                },
                IdentityInput {
                    platform: Platform::Jira,
                    username: "Saved Display Name".into(),
                    platform_user_id: Some("opaque-cloud-account".into()),
                },
                IdentityInput {
                    platform: Platform::Discourse("other".into()),
                    username: "saved-for-other-instance".into(),
                    platform_user_id: None,
                },
            ],
        })
        .await
        .unwrap();
    let github_id = Uuid::now_v7();
    let jira_id = Uuid::now_v7();
    let discourse_id = Uuid::now_v7();
    repos
        .config
        .create_source(
            github_id,
            &Platform::Github.to_string(),
            "Preflight GitHub",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    repos
        .config
        .create_source(
            jira_id,
            &Platform::Jira.to_string(),
            "Preflight Jira",
            &serde_json::json!({"api_mode":false}),
            None,
        )
        .await
        .unwrap();
    repos
        .config
        .create_source(
            discourse_id,
            &Platform::Discourse("exact".into()).to_string(),
            "Preflight Discourse",
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    let restate = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202))
        .expect(0)
        .mount(&restate)
        .await;
    let service = HandlersServiceImpl::new(repos.clone(), restate.uri(), restate.uri());
    let missing_auth = service
        .trigger_pipeline(Request::new(TriggerPipelineRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(missing_auth.code(), Code::Unauthenticated);

    let authenticated = |request| {
        let mut request = Request::new(request);
        request.extensions_mut().insert(AuthContext {
            user_id: caller_id,
            username: "admin".into(),
            display_name: "Test Admin".into(),
            role: ps_core::models::Role::Admin,
            session_id: Uuid::now_v7(),
        });
        request
    };
    for (source_id, since_date, code, reason) in [
        (
            github_id,
            Some("2020-01-01"),
            Code::FailedPrecondition,
            "#30",
        ),
        (
            jira_id,
            Some("2020-01-01"),
            Code::FailedPrecondition,
            "Cloud",
        ),
        (
            discourse_id,
            Some("2020-01-01"),
            Code::FailedPrecondition,
            "saved identity",
        ),
        (github_id, None, Code::InvalidArgument, "since_date"),
        (
            github_id,
            Some("2026-02-30"),
            Code::InvalidArgument,
            "since_date",
        ),
        (
            github_id,
            Some("9999-01-01"),
            Code::InvalidArgument,
            "run boundary",
        ),
    ] {
        let request = TriggerPipelineRequest {
            scope: Some(PersonBackfillScope {
                person_id: person.person.id.to_string(),
                source_ids: vec![source_id.to_string()],
            }),
            since_date: since_date.map(String::from),
            ..Default::default()
        };
        let error = service
            .trigger_pipeline(authenticated(request))
            .await
            .unwrap_err();
        assert_eq!(error.code(), code);
        assert!(error.message().contains(reason), "{}", error.message());
    }
    for (source_ids, code) in [
        (Vec::new(), Code::InvalidArgument),
        (
            vec![github_id.to_string(), github_id.to_string()],
            Code::InvalidArgument,
        ),
        (vec![Uuid::now_v7().to_string()], Code::FailedPrecondition),
    ] {
        let request = TriggerPipelineRequest {
            scope: Some(PersonBackfillScope {
                person_id: person.person.id.to_string(),
                source_ids,
            }),
            since_date: Some("2020-01-01".into()),
            ..Default::default()
        };
        assert_eq!(
            service
                .trigger_pipeline(authenticated(request))
                .await
                .unwrap_err()
                .code(),
            code
        );
    }
    repos
        .config
        .update_source_enabled(github_id, false)
        .await
        .unwrap();
    let disabled_request = TriggerPipelineRequest {
        scope: Some(PersonBackfillScope {
            person_id: person.person.id.to_string(),
            source_ids: vec![github_id.to_string()],
        }),
        since_date: Some("2020-01-01".into()),
        ..Default::default()
    };
    assert_eq!(
        service
            .trigger_pipeline(authenticated(disabled_request))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    assert!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .is_empty()
    );
    restate.verify().await;
    assert!(restate.received_requests().await.unwrap().is_empty());
    ctx.teardown().await;
}

#[tokio::test]
async fn legacy_status_and_cancel_apis_preserve_owned_scoped_runs() {
    use ps_core::models::{HandlerMethod, HandlerName, SourceName};
    use ps_core::repo::activity::PipelineRunParams;
    use ps_proto::canonical::prism::v1::{
        CancelHandlerRunRequest, CancelRunRequest, GetStatusRequest, ListHandlersRequest,
        SourceState,
    };
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let source_name = "Owned GitHub";
    repos
        .config
        .create_source(
            Uuid::now_v7(),
            &Platform::Github.to_string(),
            source_name,
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    let pipeline_id = Uuid::now_v7();
    repos
        .activity
        .create_pipeline(pipeline_id, Some("inv_root"))
        .await
        .unwrap();
    let source_run = Uuid::now_v7();
    let metric_run = Uuid::now_v7();
    for (run_id, source, handler) in [
        (source_run, source_name, "GithubIngestionHandler"),
        (metric_run, "_metrics", "MetricsComputeHandler"),
    ] {
        assert!(
            repos
                .activity
                .create_pipeline_run(PipelineRunParams {
                    run_id,
                    source_name: &SourceName::from(source),
                    handler_name: &HandlerName::from(handler),
                    method: &HandlerMethod::from("run_scoped"),
                    pipeline_id,
                    invocation_id: &format!("inv_{run_id}"),
                })
                .await
                .unwrap()
        );
    }
    let restate = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(202))
        .expect(0)
        .mount(&restate)
        .await;
    let service = HandlersServiceImpl::new(repos.clone(), restate.uri(), restate.uri());
    let mut request = Request::new(GetStatusRequest::default());
    authenticate(&mut request, caller);
    let statuses = service.get_status(request).await.unwrap().into_inner();
    assert_eq!(
        statuses.sources.first().unwrap().state,
        i32::from(SourceState::Collecting)
    );
    let mut request = Request::new(ListHandlersRequest::default());
    authenticate(&mut request, caller);
    let handlers = service.list_handlers(request).await.unwrap().into_inner();
    assert!(
        handlers
            .handlers
            .iter()
            .find(|handler| handler.name == "MetricsComputeHandler")
            .unwrap()
            .active_run
            .is_some()
    );
    let mut request = Request::new(CancelRunRequest {
        source_name: source_name.into(),
    });
    authenticate(&mut request, caller);
    assert_eq!(
        service.cancel_run(request).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    let mut request = Request::new(CancelHandlerRunRequest {
        run_id: metric_run.to_string(),
    });
    authenticate(&mut request, caller);
    assert_eq!(
        service
            .cancel_handler_run(request)
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    for id in [source_run, metric_run] {
        assert_eq!(
            repos.activity.get_run(id).await.unwrap().unwrap().status,
            ps_core::models::IngestionStatus::Running
        );
    }
    restate.verify().await;
    ctx.teardown().await;
}

fn authenticate<T>(request: &mut Request<T>, caller: Uuid) {
    request.extensions_mut().insert(AuthContext {
        user_id: caller,
        username: "admin".into(),
        display_name: "Test Admin".into(),
        role: ps_core::models::Role::Admin,
        session_id: Uuid::now_v7(),
    });
}

#[tokio::test]
async fn stored_queued_scoped_invocation_is_never_cancelled_by_legacy_source_api() {
    use ps_proto::canonical::prism::v1::CancelRunRequest;
    use wiremock::matchers::{body_partial_json, body_string_contains, path};
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let source_name = "Queued GitHub";
    repos
        .config
        .create_source(
            Uuid::now_v7(),
            &Platform::Github.to_string(),
            source_name,
            &serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    repos
        .activity
        .set_current_invocation_id(source_name, "inv_queued_owned")
        .await
        .unwrap();
    let restate = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/query"))
        .and(body_string_contains("target_handler_name != 'run_scoped'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows":[]})))
        .expect(1)
        .mount(&restate)
        .await;
    let query = "SELECT id, target_handler_name, target_service_name FROM sys_invocation WHERE id = 'inv_queued_owned'";
    Mock::given(method("POST")).and(path("/query"))
        .and(body_partial_json(serde_json::json!({"query":query})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows":[{
            "id":"inv_queued_owned", "target_handler_name":"run_scoped", "target_service_name":"GithubIngestionHandler"
        }]}))).expect(1).mount(&restate).await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&restate)
        .await;
    let service = HandlersServiceImpl::new(repos.clone(), restate.uri(), restate.uri());
    let mut request = Request::new(CancelRunRequest {
        source_name: source_name.into(),
    });
    authenticate(&mut request, caller);
    assert_eq!(
        service.cancel_run(request).await.unwrap_err().code(),
        Code::FailedPrecondition
    );
    assert_eq!(
        repos
            .activity
            .get_current_invocation_id(source_name)
            .await
            .unwrap()
            .as_deref(),
        Some("inv_queued_owned")
    );
    restate.verify().await;
    ctx.teardown().await;
}
