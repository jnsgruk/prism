//! Exercise startup persistence through the real API and PostgreSQL.

use std::time::Duration;

use ps_core::repo::{Repos, reasoning::CreateConversationParams};
use ps_proto::canonical::prism::v1::reasoning_service_client::ReasoningServiceClient;
use ps_proto::canonical::prism::v1::{AskQuestionRequest, ask_question_response};
use serde_json::json;
use tonic::Request;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::server::ApiTestContext;

struct StartupContext {
    api: ApiTestContext,
    agent: MockServer,
    repos: Repos,
    token: String,
    conversation: Uuid,
    _restate: MockServer,
}

impl StartupContext {
    async fn new(existing: Option<&str>, pod_uid: &str) -> Self {
        // OpenCode's port is fixed. A distinct loopback address isolates each test.
        let bytes = *Uuid::new_v4().as_bytes();
        let ip = format!("127.{}.{}.{}", bytes[0], bytes[1], bytes[2].max(1));
        let listener = std::net::TcpListener::bind(format!("{ip}:4096")).unwrap();
        let agent = MockServer::builder().listener(listener).start().await;
        let restate = MockServer::start().await;
        let api = ApiTestContext::with_restate_url(&restate.uri()).await;
        let repos = Repos::new(api.server.pool.clone());
        let (user, token) = crate::common::fixtures::create_admin_user(&api.server.pool).await;
        let conversation = repos
            .reasoning
            .create_conversation(&CreateConversationParams {
                id: None,
                user_id: user,
                title: Some("startup"),
                model_name: "google/gemini-2.5-flash",
            })
            .await
            .unwrap()
            .id;
        repos
            .reasoning
            .update_container_status(
                conversation,
                Some("test-pod"),
                "active",
                existing,
                Some(&ip),
            )
            .await
            .unwrap();
        Mock::given(method("POST"))
            .and(path(format!(
                "/AgenticQueryHandler/{conversation}/prepare_query"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "pod_ip": ip, "pod_name": "test-pod", "pod_uid": pod_uid,
            })))
            .mount(&restate)
            .await;
        Mock::given(method("GET")).and(path("/event"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(concat!(
                "data: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
                "data: {\"type\":\"message.part.updated\",\"properties\":{\"part\":{\"type\":\"text\",\"text\":\"A useful answer from the agent.\"}}}\n\n",
                "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_test\"}}\n\n",
            ), "text/event-stream")).mount(&agent).await;
        Mock::given(method("POST"))
            .and(path("/session/ses_test/prompt_async"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&agent)
            .await;
        // MockServer must live for the whole query.
        Self {
            api,
            agent,
            repos,
            token,
            conversation,
            _restate: restate,
        }
    }

    async fn healthy(&self) {
        Mock::given(method("GET"))
            .and(path("/global/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"healthy": true})))
            .mount(&self.agent)
            .await;
    }

    async fn ask(&self) -> Vec<ask_question_response::Event> {
        let mut client = ReasoningServiceClient::new(self.api.server.channel.clone());
        let mut request = Request::new(AskQuestionRequest {
            question: "Summarise the engineering data".into(),
            conversation_id: Some(self.conversation.to_string()),
            ..Default::default()
        });
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        let mut stream = client.ask_question(request).await.unwrap().into_inner();
        let mut events = Vec::new();
        while let Some(response) = tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await
            .unwrap()
            .unwrap()
        {
            let event = response.event.unwrap();
            let terminal = matches!(
                event,
                ask_question_response::Event::FinalAnswer(_)
                    | ask_question_response::Event::Error(_)
            );
            events.push(event);
            if terminal {
                break;
            }
        }
        events
    }
}

#[tokio::test]
async fn delayed_health_then_ambiguous_create_is_persisted_and_reused() {
    let ctx = StartupContext::new(None, "pod-one").await;
    ctx.healthy().await;
    Mock::given(path("/global/health"))
        .respond_with(ResponseTemplate::new(503))
        .with_priority(1)
        .up_to_n_times(2)
        .expect(2)
        .mount(&ctx.agent)
        .await;
    let title = format!("Prism conversation {}", ctx.conversation);
    Mock::given(method("GET"))
        .and(path("/session"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([{ "id": "ses_test", "title": title }])),
        )
        .mount(&ctx.agent)
        .await;
    Mock::given(method("GET"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&ctx.agent)
        .await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_string("lost response"))
        .expect(1)
        .mount(&ctx.agent)
        .await;
    Mock::given(method("GET"))
        .and(path("/session/ses_test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "ses_test"})))
        .mount(&ctx.agent)
        .await;
    Mock::given(method("GET"))
        .and(path("/session/ses_test"))
        .respond_with(ResponseTemplate::new(503))
        .with_priority(1)
        .up_to_n_times(1)
        .mount(&ctx.agent)
        .await;

    let events = ctx.ask().await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ask_question_response::Event::FinalAnswer(_))),
        "{events:?}"
    );
    let conv = ctx
        .repos
        .reasoning
        .get_conversation(ctx.conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conv.opencode_session_id.as_deref(), Some("ses_test"));
    // FinalAnswer is sent just before completion persistence; wait for the CAS release.
    for _ in 0..100 {
        if ctx
            .repos
            .reasoning
            .get_conversation(ctx.conversation)
            .await
            .unwrap()
            .unwrap()
            .query_status
            == "completed"
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let events = ctx.ask().await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ask_question_response::Event::FinalAnswer(_))),
        "{events:?}"
    );
    let requests = ctx.agent.received_requests().await.unwrap();
    let first_session = requests
        .iter()
        .position(|r| r.url.path() == "/session")
        .unwrap();
    assert_eq!(
        requests[..first_session]
            .iter()
            .filter(|r| r.url.path() == "/global/health")
            .count(),
        3
    );
    ctx.agent.verify().await;
    ctx.api.teardown().await;
}

#[tokio::test]
async fn pending_creation_on_expired_pod_can_recover() {
    let ctx = StartupContext::new(Some("pending:old-pod"), "replacement-pod").await;
    ctx.healthy().await;
    Mock::given(method("GET"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&ctx.agent)
        .await;
    Mock::given(method("POST"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "ses_test"})))
        .expect(1)
        .mount(&ctx.agent)
        .await;
    let events = ctx.ask().await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ask_question_response::Event::FinalAnswer(_))),
        "{events:?}"
    );
    assert_eq!(
        ctx.repos
            .reasoning
            .get_conversation(ctx.conversation)
            .await
            .unwrap()
            .unwrap()
            .opencode_session_id
            .as_deref(),
        Some("ses_test")
    );
    ctx.agent.verify().await;
    ctx.api.teardown().await;
}

#[tokio::test]
async fn browser_reconnect_and_cancel_preserve_pending_creation_without_duplicate_post() {
    use ps_proto::canonical::prism::v1::{
        CancelQueryRequest, ResumeStreamRequest, resume_stream_response,
    };

    let ctx = StartupContext::new(Some("pending:pod-one"), "pod-one").await;
    ctx.healthy().await;
    Mock::given(method("GET"))
        .and(path("/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&ctx.agent)
        .await;
    let mut client = ReasoningServiceClient::new(ctx.api.server.channel.clone());
    let mut request = Request::new(AskQuestionRequest {
        question: "Continue the analysis".into(),
        conversation_id: Some(ctx.conversation.to_string()),
        ..Default::default()
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", ctx.token).parse().unwrap(),
    );
    let mut stream = client.ask_question(request).await.unwrap().into_inner();
    loop {
        let response = tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if matches!(response.event, Some(ask_question_response::Event::ContainerStatus(ref s)) if s.message == "Resolving agent session...")
        {
            break;
        }
    }
    drop(stream);
    let mut request = Request::new(ResumeStreamRequest {
        conversation_id: ctx.conversation.to_string(),
        last_event_id: 0,
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", ctx.token).parse().unwrap(),
    );
    let mut resumed = client.resume_stream(request).await.unwrap().into_inner();
    loop {
        let response = tokio::time::timeout(Duration::from_secs(5), resumed.message())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if matches!(response.event, Some(resume_stream_response::Event::ContainerStatus(ref s)) if s.message == "Resolving agent session...")
        {
            break;
        }
    }
    let mut request = Request::new(CancelQueryRequest {
        conversation_id: ctx.conversation.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", ctx.token).parse().unwrap(),
    );
    client.cancel_query(request).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let conv = ctx
        .repos
        .reasoning
        .get_conversation(ctx.conversation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conv.opencode_session_id.as_deref(), Some("pending:pod-one"));
    assert_eq!(conv.query_status, "idle");
    assert!(
        ctx.agent
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| r.method != "POST")
    );
    ctx.api.teardown().await;
}
