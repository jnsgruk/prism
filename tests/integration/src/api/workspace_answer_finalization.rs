//! Completed answers are verified before persistence, emission, and resume.
use crate::common::{fixtures::create_admin_user, server::ApiTestContext};
use ps_core::repo::{Repos, reasoning::CreateConversationParams};
use ps_proto::canonical::prism::v1::{
    ResumeStreamRequest, ask_question_response, reasoning_service_client::ReasoningServiceClient,
    resume_stream_response,
};
use ps_server::features::reasoning::completion::{EventLoopResult, finalize_query};
use tonic::Request;

#[tokio::test]
async fn workspace_answer_finalization_persists_and_emits_verified_timeout_answer() {
    let ctx = ApiTestContext::new().await;
    let (user, token) = create_admin_user(&ctx.server.pool).await;
    let repos = Repos::new(ctx.server.pool.clone());
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            id: None,
            user_id: user,
            title: Some("file answer"),
            model_name: "fixture/model",
        })
        .await
        .expect("conversation");
    let cid = conv.id.to_string();
    let directory = ctx.server.workspaces_dir.path().join(&cid);
    std::fs::create_dir_all(&directory).expect("workspace");
    std::fs::write(directory.join("report.pdf"), b"%PDF-test").expect("report");
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let result = EventLoopResult {
        answer_text: "Ready: [report](/workspace/report.pdf). [missing](/workspace/no.pdf)".into(),
        tool_calls: 2,
        total_input: 10,
        total_output: 20,
        timed_out: true,
    };
    // Start a resume while the query is running; completion reads persisted content.
    repos
        .reasoning
        .update_query_status(conv.id, ps_core::models::QueryStatus::Running)
        .await
        .expect("running");
    let mut client = ReasoningServiceClient::new(ctx.server.channel.clone());
    let mut request = Request::new(ResumeStreamRequest {
        conversation_id: cid.clone(),
        last_event_id: 0,
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("token"),
    );
    let mut resumed = client
        .resume_stream(request)
        .await
        .expect("resume")
        .into_inner();
    finalize_query(
        &repos,
        conv.id,
        &cid,
        "fixture/model",
        "create report",
        &result,
        &tx,
        Some(ctx.server.workspaces_dir.path()),
    )
    .await
    .expect("finalize");
    let frame = rx.recv().await.expect("emission").expect("frame");
    let Some(ask_question_response::Event::FinalAnswer(final_answer)) = frame.event else {
        panic!("final answer")
    };
    assert!(
        final_answer
            .answer
            .contains(&format!("/ask/{cid}/files/report.pdf"))
    );
    assert!(final_answer.answer.contains("missing (file unavailable)"));
    let persisted = repos
        .reasoning
        .list_messages(conv.id)
        .await
        .expect("history");
    assert_eq!(
        persisted.last().expect("assistant").content,
        final_answer.answer
    );
    assert_eq!(persisted.last().expect("assistant").prompt_tokens, 10);
    let frame = resumed
        .message()
        .await
        .expect("resume frame")
        .expect("resume completion");
    let Some(resume_stream_response::Event::FinalAnswer(answer)) = frame.event else {
        panic!("resumed answer")
    };
    assert_eq!(answer.answer, final_answer.answer);
    ctx.teardown().await;
}
