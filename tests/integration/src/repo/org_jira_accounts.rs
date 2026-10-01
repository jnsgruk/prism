//! Jira labels can repeat; opaque IDs remain the unique ownership key.

use super::org_manual::{account, params};
use crate::common::db::RepoTestContext;
use ps_core::{Error, models::Platform, repo::org::UpdateIdentityParams};

#[tokio::test]
async fn jira_duplicate_display_names_preserve_distinct_owners() {
    let ctx = RepoTestContext::new().await;

    let alice = ctx
        .repos
        .org
        .create_person(params(
            "Alice",
            vec![account(Platform::Jira, "Alex Smith", Some("Opaque:A"))],
        ))
        .await
        .unwrap();
    let bob = ctx
        .repos
        .org
        .create_person(params(
            "Bob",
            vec![account(Platform::Jira, "Alex Smith", Some("Opaque:B"))],
        ))
        .await
        .unwrap();

    let owners = ctx
        .repos
        .org
        .batch_resolve_by_user_id(&Platform::Jira, &["Opaque:A".into(), "Opaque:B".into()])
        .await
        .unwrap();
    assert_eq!(owners["Opaque:A"], alice.person.id);
    assert_eq!(owners["Opaque:B"], bob.person.id);
    assert!(
        ctx.repos
            .org
            .batch_resolve_person_ids(&Platform::Jira, &["Alex Smith".into()])
            .await
            .unwrap()
            .is_empty()
    );

    let conflict = ctx
        .repos
        .org
        .update_person_identity(UpdateIdentityParams {
            person_id: bob.person.id.into(),
            identity_id: bob.identities[0].id,
            username: None,
            platform_user_id: Some(Some("Opaque:A".into())),
        })
        .await;
    assert!(matches!(conflict, Err(Error::Conflict(_))));

    let export = ctx.repos.org.export_org().await.unwrap();
    ctx.repos.org.import_org(&export, true).await.unwrap();
    let restored = ctx
        .repos
        .org
        .batch_resolve_by_user_id(&Platform::Jira, &["Opaque:A".into(), "Opaque:B".into()])
        .await
        .unwrap();
    assert_eq!(restored, owners);

    ctx.teardown().await;
}

#[tokio::test]
async fn jira_csv_promotes_legacy_account_and_reimports_by_opaque_id() {
    let ctx = RepoTestContext::new().await;
    let person = ctx
        .repos
        .org
        .create_person(params("Alice", vec![]))
        .await
        .unwrap();
    let identity_id = uuid::Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
        VALUES ($1, $2, 'jira', 'alice@example.com')
        "#,
        identity_id,
        person.person.id,
    )
    .execute(&ctx.pool)
    .await
    .unwrap();

    let record = ps_core::directory::JiraUserRecord {
        email: "alice@example.com".into(),
        display_name: "Alice".into(),
        account_id: "Exact:ID".into(),
    };
    for _ in 0..2 {
        let (mapped, unmatched, warnings) = ctx
            .repos
            .org
            .import_jira_users(std::slice::from_ref(&record))
            .await
            .unwrap();
        assert_eq!(mapped, 1);
        assert_eq!(unmatched, 0);
        assert!(warnings.is_empty());
    }

    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].id, identity_id);
    assert_eq!(identities[0].platform_user_id.as_deref(), Some("Exact:ID"));

    // Labels can change without creating another row for the same Cloud account.
    ctx.repos
        .org
        .update_person(person.person.id, None, Some("renamed@example.com"), None)
        .await
        .unwrap();
    let renamed = ps_core::directory::JiraUserRecord {
        email: "renamed@example.com".into(),
        ..record
    };
    assert_eq!(
        ctx.repos.org.import_jira_users(&[renamed]).await.unwrap().0,
        1
    );
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].id, identity_id);
    assert_eq!(identities[0].platform_username, "renamed@example.com");

    ctx.teardown().await;
}
