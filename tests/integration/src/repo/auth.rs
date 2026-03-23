use crate::define_repo_test;
use ps_core::auth::{generate_token, hash_password, hash_token};
use ps_core::models::Role;
use time::OffsetDateTime;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// User CRUD
// ---------------------------------------------------------------------------

define_repo_test!(create_user_and_find_by_username, |repos, pool| async move {
    let user_id = Uuid::now_v7();
    let hash = hash_password("secret123").unwrap();
    repos
        .auth
        .create_user(user_id, "alice", "Alice A", &hash, Role::Admin)
        .await
        .unwrap();

    let creds = repos.auth.find_user_by_username("alice").await.unwrap();
    assert!(creds.is_some());
    let creds = creds.unwrap();
    assert_eq!(creds.id, user_id);
    assert!(creds.is_active);
});

define_repo_test!(
    find_user_by_username_returns_none_for_unknown,
    |repos, _pool| async move {
        let creds = repos.auth.find_user_by_username("nobody").await.unwrap();
        assert!(creds.is_none());
    }
);

define_repo_test!(any_users_exist_false_initially, |repos, _pool| async move {
    assert!(!repos.auth.any_users_exist().await.unwrap());
});

define_repo_test!(
    any_users_exist_true_after_create,
    |repos, _pool| async move {
        let hash = hash_password("pw").unwrap();
        repos
            .auth
            .create_user(Uuid::now_v7(), "bob", "Bob B", &hash, Role::Admin)
            .await
            .unwrap();

        assert!(repos.auth.any_users_exist().await.unwrap());
    }
);

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

define_repo_test!(create_session_and_validate, |repos, _pool| async move {
    let user_id = Uuid::now_v7();
    let hash = hash_password("pw").unwrap();
    repos
        .auth
        .create_user(user_id, "carol", "Carol C", &hash, Role::Admin)
        .await
        .unwrap();

    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);
    let session_id = Uuid::now_v7();
    let expires = OffsetDateTime::now_utc() + time::Duration::days(7);

    repos
        .auth
        .create_session(
            session_id,
            user_id,
            &token_hash,
            "browser",
            Some(expires),
            None,
        )
        .await
        .unwrap();

    let session = repos.auth.validate_session(&token_hash).await.unwrap();
    assert!(session.is_some());
    let session = session.unwrap();
    assert_eq!(session.user_id, user_id);
    assert_eq!(session.username, "carol");
    assert_eq!(session.role, Role::Admin);
    assert!(session.is_active);
});

define_repo_test!(
    validate_session_returns_none_for_unknown_hash,
    |repos, _pool| async move {
        let session = repos.auth.validate_session("no-such-hash").await.unwrap();
        assert!(session.is_none());
    }
);

define_repo_test!(delete_session_removes_it, |repos, _pool| async move {
    let user_id = Uuid::now_v7();
    let hash = hash_password("pw").unwrap();
    repos
        .auth
        .create_user(user_id, "dave", "Dave D", &hash, Role::Admin)
        .await
        .unwrap();

    let raw_token = generate_token();
    let token_hash = hash_token(&raw_token);
    let session_id = Uuid::now_v7();

    repos
        .auth
        .create_session(session_id, user_id, &token_hash, "browser", None, None)
        .await
        .unwrap();

    // Validate exists
    assert!(
        repos
            .auth
            .validate_session(&token_hash)
            .await
            .unwrap()
            .is_some()
    );

    // Delete and verify gone
    repos.auth.delete_session(session_id).await.unwrap();
    assert!(
        repos
            .auth
            .validate_session(&token_hash)
            .await
            .unwrap()
            .is_none()
    );
});

define_repo_test!(
    touch_session_updates_last_active,
    |repos, _pool| async move {
        let user_id = Uuid::now_v7();
        let hash = hash_password("pw").unwrap();
        repos
            .auth
            .create_user(user_id, "eve", "Eve E", &hash, Role::Admin)
            .await
            .unwrap();

        let raw_token = generate_token();
        let token_hash = hash_token(&raw_token);
        let session_id = Uuid::now_v7();

        repos
            .auth
            .create_session(session_id, user_id, &token_hash, "browser", None, None)
            .await
            .unwrap();

        // touch should not error
        repos.auth.touch_session(session_id).await.unwrap();
    }
);

// ---------------------------------------------------------------------------
// API tokens
// ---------------------------------------------------------------------------

define_repo_test!(create_api_token_and_list, |repos, _pool| async move {
    let user_id = Uuid::now_v7();
    let hash = hash_password("pw").unwrap();
    repos
        .auth
        .create_user(user_id, "frank", "Frank F", &hash, Role::Admin)
        .await
        .unwrap();

    // Create two API tokens
    let token1_hash = hash_token(&generate_token());
    let token1_id = Uuid::now_v7();
    repos
        .auth
        .create_session(
            token1_id,
            user_id,
            &token1_hash,
            "api_token",
            None,
            Some("CI token"),
        )
        .await
        .unwrap();

    let token2_hash = hash_token(&generate_token());
    let token2_id = Uuid::now_v7();
    repos
        .auth
        .create_session(
            token2_id,
            user_id,
            &token2_hash,
            "api_token",
            None,
            Some("Deploy token"),
        )
        .await
        .unwrap();

    let tokens = repos.auth.list_api_tokens(user_id).await.unwrap();
    assert_eq!(tokens.len(), 2);
    // Ordered by created_at DESC
    assert_eq!(tokens[0].token_name.as_deref(), Some("Deploy token"));
    assert_eq!(tokens[1].token_name.as_deref(), Some("CI token"));
});

define_repo_test!(revoke_api_token, |repos, _pool| async move {
    let user_id = Uuid::now_v7();
    let hash = hash_password("pw").unwrap();
    repos
        .auth
        .create_user(user_id, "grace", "Grace G", &hash, Role::Admin)
        .await
        .unwrap();

    let token_hash = hash_token(&generate_token());
    let token_id = Uuid::now_v7();
    repos
        .auth
        .create_session(
            token_id,
            user_id,
            &token_hash,
            "api_token",
            None,
            Some("temp"),
        )
        .await
        .unwrap();

    assert!(
        repos
            .auth
            .delete_api_token(token_id, user_id)
            .await
            .unwrap()
    );
    // Second delete returns false
    assert!(
        !repos
            .auth
            .delete_api_token(token_id, user_id)
            .await
            .unwrap()
    );
    // List is empty
    assert!(
        repos
            .auth
            .list_api_tokens(user_id)
            .await
            .unwrap()
            .is_empty()
    );
});

define_repo_test!(
    delete_api_token_wrong_user_returns_false,
    |repos, _pool| async move {
        let user_id = Uuid::now_v7();
        let other_user_id = Uuid::now_v7();
        let hash = hash_password("pw").unwrap();
        repos
            .auth
            .create_user(user_id, "heidi", "Heidi H", &hash, Role::Admin)
            .await
            .unwrap();
        repos
            .auth
            .create_user(other_user_id, "ivan", "Ivan I", &hash, Role::Admin)
            .await
            .unwrap();

        let token_hash = hash_token(&generate_token());
        let token_id = Uuid::now_v7();
        repos
            .auth
            .create_session(token_id, user_id, &token_hash, "api_token", None, None)
            .await
            .unwrap();

        // Wrong user cannot delete
        assert!(
            !repos
                .auth
                .delete_api_token(token_id, other_user_id)
                .await
                .unwrap()
        );
        // Still present for the correct user
        assert_eq!(repos.auth.list_api_tokens(user_id).await.unwrap().len(), 1);
    }
);

// ---------------------------------------------------------------------------
// Backup helpers
// ---------------------------------------------------------------------------

define_repo_test!(count_users_and_export, |repos, _pool| async move {
    assert_eq!(repos.auth.count_users().await.unwrap(), 0);
    assert!(repos.auth.export_users().await.unwrap().is_empty());

    let hash = hash_password("pw").unwrap();
    repos
        .auth
        .create_user(Uuid::now_v7(), "jan", "Jan J", &hash, Role::Admin)
        .await
        .unwrap();

    assert_eq!(repos.auth.count_users().await.unwrap(), 1);
    let exported = repos.auth.export_users().await.unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0]["username"], "jan");
    // Ensure password hash is NOT in export
    assert!(exported[0].get("password_hash").is_none());
});
