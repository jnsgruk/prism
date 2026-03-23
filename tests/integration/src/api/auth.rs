use crate::define_api_test;
use ps_proto::canonical::prism::v1::auth_service_client::AuthServiceClient;
use ps_proto::canonical::prism::v1::{
    CompleteSetupRequest, GetCurrentUserRequest, GetSetupStatusRequest, LoginRequest, LogoutRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

define_api_test!(setup_status_returns_false_initially, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    let resp = client
        .get_setup_status(GetSetupStatusRequest {})
        .await
        .expect("get_setup_status")
        .into_inner();

    assert!(!resp.setup_complete);
});

define_api_test!(
    complete_setup_creates_admin_and_session,
    |server| async move {
        let mut client = AuthServiceClient::new(server.channel.clone());

        let resp = client
            .complete_setup(CompleteSetupRequest {
                username: "admin".into(),
                display_name: "Test Admin".into(),
                password: "secure-password-123".into(),
            })
            .await
            .expect("complete_setup")
            .into_inner();

        assert!(!resp.session_token.is_empty());

        // Setup status should now be true
        let status = client
            .get_setup_status(GetSetupStatusRequest {})
            .await
            .expect("get_setup_status")
            .into_inner();

        assert!(status.setup_complete);
    }
);

define_api_test!(complete_setup_rejects_second_call, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    client
        .complete_setup(CompleteSetupRequest {
            username: "admin".into(),
            display_name: "Test Admin".into(),
            password: "secure-password-123".into(),
        })
        .await
        .expect("first setup");

    let err = client
        .complete_setup(CompleteSetupRequest {
            username: "admin2".into(),
            display_name: "Another Admin".into(),
            password: "another-password".into(),
        })
        .await
        .expect_err("second setup should fail");

    assert_eq!(err.code(), tonic::Code::FailedPrecondition);
});

define_api_test!(login_and_get_current_user, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    // Setup first
    client
        .complete_setup(CompleteSetupRequest {
            username: "admin".into(),
            display_name: "Test Admin".into(),
            password: "my-password".into(),
        })
        .await
        .expect("complete_setup");

    // Login
    let login_resp = client
        .login(LoginRequest {
            username: "admin".into(),
            password: "my-password".into(),
        })
        .await
        .expect("login")
        .into_inner();

    assert!(!login_resp.session_token.is_empty());
    assert!(login_resp.expires_at.is_some());

    // Use the token to get current user
    let token = login_resp.session_token;
    let mut req = Request::new(GetCurrentUserRequest {});
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );

    let user = client
        .get_current_user(req)
        .await
        .expect("get_current_user")
        .into_inner();

    assert_eq!(user.username, "admin");
    assert_eq!(user.display_name, "Test Admin");
    assert_eq!(user.role, "admin");
});

define_api_test!(login_with_wrong_password_fails, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    client
        .complete_setup(CompleteSetupRequest {
            username: "admin".into(),
            display_name: "Test Admin".into(),
            password: "correct-password".into(),
        })
        .await
        .expect("complete_setup");

    let err = client
        .login(LoginRequest {
            username: "admin".into(),
            password: "wrong-password".into(),
        })
        .await
        .expect_err("login should fail");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});

define_api_test!(logout_invalidates_session, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    let setup_resp = client
        .complete_setup(CompleteSetupRequest {
            username: "admin".into(),
            display_name: "Test Admin".into(),
            password: "my-password".into(),
        })
        .await
        .expect("complete_setup")
        .into_inner();

    let token = setup_resp.session_token;

    // Logout with the token
    let mut req = Request::new(LogoutRequest {});
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );

    client.logout(req).await.expect("logout");

    // Token should no longer work
    let mut req = Request::new(GetCurrentUserRequest {});
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );

    let err = client
        .get_current_user(req)
        .await
        .expect_err("should be unauthenticated");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});

define_api_test!(protected_rpc_without_token_fails, |server| async move {
    let mut client = AuthServiceClient::new(server.channel.clone());

    let err = client
        .get_current_user(GetCurrentUserRequest {})
        .await
        .expect_err("should require auth");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});
