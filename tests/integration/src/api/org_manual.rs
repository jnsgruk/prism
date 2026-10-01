//! Admin-only manual people/account API contracts.

use crate::common::server::ApiTestContext;
use ps_core::auth::{generate_token, hash_token};
use ps_proto::canonical::prism::v1::{self as proto, org_service_client::OrgServiceClient};
use tonic::{Code, Request};
use uuid::Uuid;

fn authed<T>(payload: T, token: &str) -> Request<T> {
    let mut request = Request::new(payload);
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());

    request
}

fn identity(username: &str) -> proto::IdentityInput {
    proto::IdentityInput {
        platform: proto::Platform::Github as i32,
        username: username.into(),
        platform_instance: None,
        platform_user_id: None,
    }
}

fn create(name: &str) -> proto::CreatePersonRequest {
    proto::CreatePersonRequest {
        name: name.into(),
        email: None,
        level: None,
        team_id: None,
        identities: vec![identity("alice")],
    }
}

#[tokio::test]
async fn manual_jira_accounts_with_duplicate_display_names_save_distinct_ids() {
    let ctx = ApiTestContext::new().await;
    let (_, admin) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let mut request = create("Alex");
    request.identities = ["Opaque:A", "Opaque:B"]
        .into_iter()
        .map(|id| proto::IdentityInput {
            platform: proto::Platform::Jira as i32,
            username: "Alex Smith".into(),
            platform_instance: None,
            platform_user_id: Some(id.into()),
        })
        .collect();

    let saved = client
        .create_person(authed(request, &admin))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();
    assert_eq!(saved.identities.len(), 2);
    assert_ne!(saved.identities[0].id, saved.identities[1].id);
    for id in ["Opaque:A", "Opaque:B"] {
        assert!(
            saved
                .identities
                .iter()
                .any(|identity| identity.platform_user_id.as_deref() == Some(id))
        );
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_rpcs_require_admin_and_return_complete_saved_person() {
    let ctx = ApiTestContext::new().await;

    let (_, admin) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;

    // Unsupported non-admin role is rejected by the existing fail-closed auth decoder.

    // The domain currently supports only Admin; no additional role is introduced.
    let viewer_id = Uuid::now_v7();
    let session_id = Uuid::now_v7();
    let token = generate_token();
    let digest = hash_token(&token);
    sqlx::query!(
        r#"
        INSERT INTO auth.users (id, username, display_name, password_hash, role)
        VALUES ($1, 'viewer', 'Viewer', 'unused', 'viewer')
        "#,
        viewer_id
    )
    .execute(&ctx.server.pool)
    .await
    .unwrap();

    sqlx::query!(
        r#"
        INSERT INTO auth.sessions (id, user_id, token_hash, session_type, expires_at)
        VALUES ($1, $2, $3, 'browser', now() + interval '1 day')
        "#,
        session_id,
        viewer_id,
        digest
    )
    .execute(&ctx.server.pool)
    .await
    .unwrap();

    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let saved = client
        .create_person(authed(create("Alice"), &admin))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    assert!(saved.active);
    assert_eq!(saved.identities.len(), 1);
    assert!(!saved.identities[0].id.is_empty());
    assert_eq!(
        saved.identities[0].management,
        proto::Management::Manual as i32
    );

    for role in [None, Some(token.as_str())] {
        let expected = Code::Unauthenticated;
        let req = create("Unauthorized");
        let req = match role {
            Some(t) => authed(req, t),
            None => Request::new(req),
        };

        assert_eq!(
            client.create_person(req).await.unwrap_err().code(),
            expected
        );

        let req = proto::AddPersonIdentityRequest {
            person_id: saved.id.clone(),
            identity: Some(identity("bob")),
        };
        let req = match role {
            Some(t) => authed(req, t),
            None => Request::new(req),
        };

        assert_eq!(
            client.add_person_identity(req).await.unwrap_err().code(),
            expected
        );

        let req = proto::UpdatePersonIdentityRequest {
            person_id: saved.id.clone(),
            identity_id: saved.identities[0].id.clone(),
            username: Some("new".into()),
            platform_user_id_change: None,
        };
        let req = match role {
            Some(t) => authed(req, t),
            None => Request::new(req),
        };

        assert_eq!(
            client.update_person_identity(req).await.unwrap_err().code(),
            expected
        );

        let req = proto::RemovePersonIdentityRequest {
            person_id: saved.id.clone(),
            identity_id: saved.identities[0].id.clone(),
        };
        let req = match role {
            Some(t) => authed(req, t),
            None => Request::new(req),
        };

        assert_eq!(
            client.remove_person_identity(req).await.unwrap_err().code(),
            expected
        );
    }

    let added = client
        .add_person_identity(authed(
            proto::AddPersonIdentityRequest {
                person_id: saved.id.clone(),
                identity: Some(identity("second")),
            },
            &admin,
        ))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    assert_eq!(added.identities.len(), 2);

    let updated = client
        .update_person_identity(authed(
            proto::UpdatePersonIdentityRequest {
                person_id: saved.id.clone(),
                identity_id: saved.identities[0].id.clone(),
                username: Some("changed".into()),
                platform_user_id_change: None,
            },
            &admin,
        ))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    assert!(updated.identities.iter().any(|i| i.username == "changed"));
    assert_eq!(updated.identities.len(), 2);

    let removed = client
        .remove_person_identity(authed(
            proto::RemovePersonIdentityRequest {
                person_id: saved.id.clone(),
                identity_id: saved.identities[0].id.clone(),
            },
            &admin,
        ))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    assert_eq!(removed.identities.len(), 1);
    assert_eq!(removed.identities[0].username, "second");

    let jira = client
        .add_person_identity(authed(
            proto::AddPersonIdentityRequest {
                person_id: saved.id.clone(),
                identity: Some(proto::IdentityInput {
                    platform: proto::Platform::Jira as i32,
                    username: "Alice Smith".into(),
                    platform_instance: None,
                    platform_user_id: Some("Opaque:CaseID".into()),
                }),
            },
            &admin,
        ))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    let jira_id = jira
        .identities
        .iter()
        .find(|i| i.platform == proto::Platform::Jira as i32)
        .unwrap();

    assert_eq!(jira_id.platform_user_id.as_deref(), Some("Opaque:CaseID"));

    let listed = client
        .list_people(authed(proto::ListPeopleRequest::default(), &admin))
        .await
        .unwrap()
        .into_inner()
        .people;
    let read = listed.iter().find(|p| p.id == saved.id).unwrap();

    assert!(
        read.identities
            .iter()
            .any(|i| i.id == jira_id.id && i.platform_user_id.as_deref() == Some("Opaque:CaseID"))
    );

    let invalid = client
        .update_person(authed(
            proto::UpdatePersonRequest {
                person_id: saved.id.clone(),
                name: Some(" ".into()),
                email: None,
                level: None,
            },
            &admin,
        ))
        .await
        .unwrap_err();

    assert_eq!(invalid.code(), Code::InvalidArgument);

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_errors_are_explicit_and_do_not_expose_database_details() {
    let ctx = ApiTestContext::new().await;

    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let saved = client
        .create_person(authed(create("Alice"), &token))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();

    let conflict = client
        .create_person(authed(create("Bob"), &token))
        .await
        .unwrap_err();

    assert_eq!(conflict.code(), Code::AlreadyExists);
    assert_eq!(conflict.message(), "account is already assigned");

    let missing = client
        .remove_person_identity(authed(
            proto::RemovePersonIdentityRequest {
                person_id: saved.id.clone(),
                identity_id: Uuid::now_v7().to_string(),
            },
            &token,
        ))
        .await
        .unwrap_err();

    assert_eq!(missing.code(), Code::NotFound);

    let mut invalid = create("Bad");
    invalid.team_id = Some("not-a-uuid".into());

    assert_eq!(
        client
            .create_person(authed(invalid, &token))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let mut invalid = create("Bad");
    invalid.identities = vec![proto::IdentityInput {
        platform: proto::Platform::Discourse as i32,
        username: "valid".into(),
        platform_instance: None,
        platform_user_id: None,
    }];

    assert_eq!(
        client
            .create_person(authed(invalid, &token))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    // Genuine unexpected database error: fixed public text, logged details.

    sqlx::query!("DROP TABLE org.identity_resolutions")
        .execute(&ctx.server.pool)
        .await
        .unwrap();

    let internal = client
        .add_person_identity(authed(
            proto::AddPersonIdentityRequest {
                person_id: saved.id,
                identity: Some(identity("third")),
            },
            &token,
        ))
        .await
        .unwrap_err();

    assert_eq!(internal.code(), Code::Internal);
    assert_eq!(internal.message(), "internal error");

    ctx.teardown().await;
}
