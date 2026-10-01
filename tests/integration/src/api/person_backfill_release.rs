//! Release admission uses saved identities and remains queryable after edits.

use ps_core::models::{Platform, Role};
use ps_core::repo::Repos;
use ps_core::repo::org::{CreatePersonParams, IdentityInput, UpdateIdentityParams};
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersService;
use ps_proto::canonical::prism::v1::{
    GetStatusRequest, PersonBackfillScope, TriggerPipelineRequest,
};
use ps_server::features::dispatch::HandlersServiceImpl;
use ps_server::interceptor::AuthContext;
use tonic::{Code, Request};
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::common::server::ApiTestContext;

fn authenticate<T>(value: T, caller: Uuid, role: Role) -> Request<T> {
    let mut request = Request::new(value);
    request.extensions_mut().insert(AuthContext {
        user_id: caller,
        username: "admin".into(),
        display_name: "Test Admin".into(),
        role,
        session_id: Uuid::now_v7(),
    });
    request
}

#[tokio::test]
async fn person_admission_freezes_exact_sources_accounts_and_date_and_reuses_submission() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let identities = vec![
        IdentityInput {
            platform: Platform::Github,
            username: "selected".into(),
            platform_user_id: None,
        },
        IdentityInput {
            platform: Platform::Jira,
            username: "Selected Display Name".into(),
            platform_user_id: Some("opaque-cloud-account".into()),
        },
        IdentityInput {
            platform: Platform::Discourse("ubuntu".into()),
            username: "selected-ubuntu".into(),
            platform_user_id: None,
        },
        IdentityInput {
            platform: Platform::Discourse("snapcraft".into()),
            username: "selected-snapcraft".into(),
            platform_user_id: None,
        },
    ];
    let person = repos
        .org
        .create_person(CreatePersonParams {
            name: "Unassigned release person".into(),
            email: None,
            level: None,
            team_id: None,
            identities,
        })
        .await
        .unwrap();
    let mut sources = Vec::new();
    for (platform, name) in [
        (Platform::Github, "GitHub"),
        (Platform::Jira, "Jira"),
        (Platform::Discourse("ubuntu".into()), "Ubuntu"),
        (Platform::Discourse("snapcraft".into()), "Snapcraft"),
    ] {
        let id = Uuid::now_v7();
        repos
            .config
            .create_source(
                id,
                &platform.to_string(),
                name,
                &serde_json::json!({"api_mode":"cloud"}),
                None,
            )
            .await
            .unwrap();
        sources.push(id);
    }
    let server = MockServer::start().await;
    let service = HandlersServiceImpl::new(repos.clone(), server.uri(), server.uri());
    let status = service
        .get_status(authenticate(
            GetStatusRequest::default(),
            caller,
            Role::Admin,
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(status.person_backfill_capabilities.unwrap().enabled);
    let submission = Uuid::now_v7();
    Mock::given(method("POST"))
        .and(path(format!(
            "/ScopedIngestionPipelineWorkflow/{submission}/run/send"
        )))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_json(serde_json::json!({"invocationId":"inv_release"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let trigger = TriggerPipelineRequest {
        scope: Some(PersonBackfillScope {
            person_id: person.person.id.to_string(),
            source_ids: sources.iter().rev().map(ToString::to_string).collect(),
        }),
        since_date: Some("2020-01-01".into()),
        submission_id: Some(submission.to_string()),
    };
    assert_eq!(
        service
            .trigger_pipeline(Request::new(trigger.clone()))
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    let admitted = service
        .trigger_pipeline(authenticate(trigger.clone(), caller, Role::Admin))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(admitted.pipeline_id, submission.to_string());
    let before = repos
        .activity
        .get_pipeline(submission)
        .await
        .unwrap()
        .unwrap();
    let snapshot: ps_core::ingestion::PipelineRequest =
        serde_json::from_value(before.request_snapshot.clone()).unwrap();
    assert_eq!(
        snapshot.scope.person_id().unwrap().into_inner(),
        person.person.id
    );
    assert_eq!(snapshot.since_date.as_deref(), Some("2020-01-01"));
    assert_eq!(snapshot.sources.len(), 4);
    assert_eq!(
        snapshot
            .sources
            .iter()
            .find(|source| source.platform == Platform::Jira)
            .unwrap()
            .identity
            .as_ref()
            .unwrap()
            .platform_user_id
            .as_deref(),
        Some("opaque-cloud-account")
    );
    for source in &snapshot.sources {
        assert!(sources.contains(&source.source_id.into_inner()));
        assert_eq!(source.identity.as_ref().unwrap().platform, source.platform);
    }
    let account = person
        .identities
        .iter()
        .find(|identity| identity.platform == Platform::Github.to_string())
        .unwrap();
    repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: person.person.id.into(),
            identity_id: account.id,
            username: Some("corrected".into()),
            platform_user_id: None,
        })
        .await
        .unwrap();
    repos
        .config
        .update_source_settings(sources[0], &serde_json::json!({"orgs":["updated"]}))
        .await
        .unwrap();
    let retry = service
        .trigger_pipeline(authenticate(trigger, caller, Role::Admin))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(retry.pipeline_id, admitted.pipeline_id);
    assert_eq!(
        repos
            .activity
            .get_pipeline(submission)
            .await
            .unwrap()
            .unwrap()
            .request_snapshot,
        before.request_snapshot
    );
    assert_eq!(
        snapshot
            .sources
            .iter()
            .find(|source| source.platform == Platform::Github)
            .unwrap()
            .identity
            .as_ref()
            .unwrap()
            .username
            .as_str(),
        "selected"
    );
    for _ in 0..100 {
        if repos
            .activity
            .get_pipeline(submission)
            .await
            .unwrap()
            .unwrap()
            .dispatch_acknowledged
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        repos
            .activity
            .get_pipeline(submission)
            .await
            .unwrap()
            .unwrap()
            .current_invocation_id
            .as_deref(),
        Some("inv_release")
    );
    let requests = server.received_requests().await.unwrap();
    let dispatched: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(dispatched, before.request_snapshot);
    server.verify().await;
    ctx.teardown().await;
}
