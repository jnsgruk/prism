//! Manual people/account writes against real PostgreSQL.

use crate::common::db::RepoTestContext;
use ps_core::{
    Error,
    models::{Management, Platform, TeamType},
    repo::org::{CreatePersonParams, IdentityInput, UpdateIdentityParams},
};
use uuid::Uuid;

pub(super) fn account(platform: Platform, username: &str, id: Option<&str>) -> IdentityInput {
    IdentityInput {
        platform,
        username: username.into(),
        platform_user_id: id.map(Into::into),
    }
}

pub(super) fn params(name: &str, identities: Vec<IdentityInput>) -> CreatePersonParams {
    CreatePersonParams {
        name: name.into(),
        email: Some(format!("{}@example.com", name.to_lowercase())),
        level: None,
        team_id: None,
        identities,
    }
}

#[tokio::test]
async fn manual_people_create_complete_with_optional_team() {
    let ctx = RepoTestContext::new().await;

    let person = ctx
        .repos
        .org
        .create_person(params(
            "Alice",
            vec![
                account(Platform::Github, "AliceGH", None),
                account(Platform::Jira, "Alice@example.com", Some("AbC:123")),
            ],
        ))
        .await
        .unwrap();

    assert!(person.person.active);
    assert!(person.person.team_id.is_none());
    assert_eq!(person.identities.len(), 2);
    assert!(
        person
            .identities
            .iter()
            .all(|i| i.management == Management::Manual)
    );
    assert!(
        person
            .identities
            .iter()
            .any(|i| i.platform_user_id.as_deref() == Some("AbC:123"))
    );

    let row = sqlx::query!(
        "SELECT directory_id,last_import_at,membership_management FROM org.people WHERE id=$1",
        person.person.id
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    assert!(row.directory_id.is_none());
    assert!(row.last_import_at.is_none());
    assert_eq!(row.membership_management, "manual");

    let team = ctx
        .repos
        .org
        .create_team("Team", "Org", TeamType::Team, None, None)
        .await
        .unwrap();

    let mut input = params("Bob", vec![]);
    input.team_id = Some(team.id.into());
    let bob = ctx.repos.org.create_person(input).await.unwrap();

    assert_eq!(bob.person.team_id, Some(team.id));
    assert_eq!(bob.person.team_name.as_deref(), Some("Team"));

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_creation_rolls_back_missing_team_and_duplicate_accounts() {
    let ctx = RepoTestContext::new().await;

    let mut input = params("Alice", vec![account(Platform::Github, "alice", None)]);
    input.team_id = Some(Uuid::now_v7().into());

    assert!(matches!(
        ctx.repos.org.create_person(input).await,
        Err(Error::NotFound(_))
    ));

    let input = params(
        "Alice",
        vec![
            account(Platform::Github, "alice", None),
            account(Platform::Github, "ALICE", None),
        ],
    );

    assert!(matches!(
        ctx.repos.org.create_person(input).await,
        Err(Error::Conflict(_))
    ));

    let input = params(
        "Alice",
        vec![
            account(Platform::Jira, "first", Some("ID")),
            account(Platform::Jira, "second", Some("ID")),
        ],
    );

    assert!(matches!(
        ctx.repos.org.create_person(input).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM org.people")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM org.platform_identities")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM org.team_memberships")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(0)
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_validation_rejects_blank_invalid_and_missing_instance() {
    let ctx = RepoTestContext::new().await;

    for input in [
        params(" ", vec![]),
        params("Alice", vec![account(Platform::Github, "a/b", None)]),
        params(
            "Alice",
            vec![account(Platform::Discourse("".into()), "alice", None)],
        ),
        params(
            "Alice",
            vec![account(
                Platform::Discourse("Bad/instance".into()),
                "alice",
                None,
            )],
        ),
        params("Alice", vec![account(Platform::Jira, "alice", None)]),
        params("Alice", vec![account(Platform::Jira, "alice", Some(" "))]),
    ] {
        assert!(matches!(
            ctx.repos.org.create_person(input).await,
            Err(Error::Validation(_))
        ));
    }

    let mut input = params("Alice", vec![]);
    input.email = Some("bad-email".into());

    assert!(matches!(
        ctx.repos.org.create_person(input).await,
        Err(Error::Validation(_))
    ));

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_disparate_discourse_instances_and_case_safe_jira_ids() {
    let ctx = RepoTestContext::new().await;

    let alice = ctx
        .repos
        .org
        .create_person(params(
            "Alice",
            vec![
                account(Platform::Discourse("ubuntu".into()), "SameUser", None),
                account(Platform::Discourse("snapcraft".into()), "SameUser", None),
                account(Platform::Jira, "Alice Smith", Some("MixedCase")),
            ],
        ))
        .await
        .unwrap();

    assert_eq!(alice.identities.len(), 3);
    assert!(
        alice
            .identities
            .iter()
            .filter(|i| i.platform.starts_with("discourse-"))
            .all(|i| i.platform_username == "sameuser")
    );
    assert!(matches!(
        ctx.repos
            .org
            .create_person(params(
                "Bob",
                vec![account(Platform::Jira, "different", Some("MixedCase"))]
            ))
            .await,
        Err(Error::Conflict(_))
    ));

    let bob = ctx
        .repos
        .org
        .create_person(params(
            "Bob",
            vec![account(Platform::Jira, "different", Some("mixedcase"))],
        ))
        .await
        .unwrap();

    assert_ne!(alice.person.id, bob.person.id);

    ctx.teardown().await;
}

#[tokio::test]
async fn explicit_account_edits_are_owned_and_preserve_other_accounts() {
    let ctx = RepoTestContext::new().await;

    let alice = ctx
        .repos
        .org
        .create_person(params(
            "Alice",
            vec![
                account(Platform::Github, "first", None),
                account(Platform::Github, "second", None),
            ],
        ))
        .await
        .unwrap();

    let bob = ctx
        .repos
        .org
        .create_person(params("Bob", vec![]))
        .await
        .unwrap();

    let first = alice
        .identities
        .iter()
        .find(|i| i.platform_username == "first")
        .unwrap();

    let wrong = UpdateIdentityParams {
        person_id: bob.person.id.into(),
        identity_id: first.id,
        username: Some("stolen".into()),
        platform_user_id: None,
    };

    assert!(matches!(
        ctx.repos.org.update_person_identity(wrong).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        ctx.repos
            .org
            .remove_person_identity(bob.person.id.into(), first.id)
            .await,
        Err(Error::NotFound(_))
    ));

    let updated = ctx
        .repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: alice.person.id.into(),
            identity_id: first.id,
            username: Some("Changed".into()),
            platform_user_id: Some(Some("ABC".into())),
        })
        .await
        .unwrap();

    assert_eq!(updated.identities.len(), 2);
    assert!(updated.identities.iter().any(|i| i.id == first.id
        && i.platform_username == "changed"
        && i.platform_user_id.as_deref() == Some("ABC")));
    assert!(
        updated
            .identities
            .iter()
            .any(|i| i.platform_username == "second")
    );

    let preserved = ctx
        .repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: alice.person.id.into(),
            identity_id: first.id,
            username: None,
            platform_user_id: None,
        })
        .await
        .unwrap();

    assert!(
        preserved
            .identities
            .iter()
            .any(|i| i.id == first.id && i.platform_user_id.as_deref() == Some("ABC"))
    );

    let cleared = ctx
        .repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: alice.person.id.into(),
            identity_id: first.id,
            username: None,
            platform_user_id: Some(None),
        })
        .await
        .unwrap();

    assert!(
        cleared
            .identities
            .iter()
            .any(|i| i.id == first.id && i.platform_user_id.is_none())
    );
    assert!(matches!(
        ctx.repos
            .org
            .add_person_identity(
                bob.person.id.into(),
                account(Platform::Github, "CHANGED", None)
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        ctx.repos
            .org
            .add_person_identity(
                Uuid::now_v7().into(),
                account(Platform::Github, "missing", None)
            )
            .await,
        Err(Error::NotFound(_))
    ));

    ctx.teardown().await;
}

#[tokio::test]
async fn removing_account_preserves_attributed_activity_and_manual_resolution() {
    let ctx = RepoTestContext::new().await;

    let alice = ctx
        .repos
        .org
        .create_person(params(
            "Alice",
            vec![account(Platform::Github, "alice", None)],
        ))
        .await
        .unwrap();

    let contribution = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO activity.contributions
            (id, person_id, platform, platform_id, contribution_type, title, created_at)
        VALUES ($1, $2, 'github', 'manual-test', 'pull_request', 'Test', now())
        "#,
        contribution,
        alice.person.id
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    let removed = ctx
        .repos
        .org
        .remove_person_identity(alice.person.id.into(), alice.identities[0].id)
        .await
        .unwrap();

    assert!(removed.identities.is_empty());
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT person_id FROM activity.contributions WHERE id=$1",
            contribution
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        Some(alice.person.id)
    );
    assert_eq!(
        sqlx::query_scalar!(
            "SELECT status FROM org.identity_resolutions WHERE person_id=$1 AND platform='github'",
            alice.person.id
        )
        .fetch_one(&ctx.pool)
        .await
        .unwrap(),
        "manual"
    );
    assert!(
        ctx.repos
            .org
            .batch_resolve_person_ids(&Platform::Github, &["alice".into()])
            .await
            .unwrap()
            .is_empty()
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn manual_person_fields_validate_and_email_can_be_cleared() {
    let ctx = RepoTestContext::new().await;

    let alice = ctx
        .repos
        .org
        .create_person(params("Alice", vec![]))
        .await
        .unwrap();

    assert!(matches!(
        ctx.repos
            .org
            .update_person(alice.person.id, Some(" "), None, None)
            .await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        ctx.repos
            .org
            .update_person(alice.person.id, None, Some("invalid"), None)
            .await,
        Err(Error::Validation(_))
    ));

    let cleared = ctx
        .repos
        .org
        .update_person(alice.person.id, None, Some(""), None)
        .await
        .unwrap();

    assert!(cleared.email.is_none());

    ctx.teardown().await;
}
