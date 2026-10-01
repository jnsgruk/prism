//! Account ownership and delayed-resolution concurrency regression tests.
use crate::common::db::RepoTestContext;
use ps_core::{
    Error,
    models::{Platform, ResolutionStatus},
    repo::org::{CreatePersonParams, IdentityInput},
};

fn person(name: &str) -> CreatePersonParams {
    CreatePersonParams {
        name: name.into(),
        email: Some(format!("{name}@example.com")),
        level: None,
        team_id: None,
        identities: vec![],
    }
}
fn jira(username: &str) -> IdentityInput {
    IdentityInput {
        platform: Platform::Jira,
        username: username.into(),
        platform_user_id: Some("Shared:OpaqueID".into()),
    }
}

#[tokio::test]
async fn concurrent_existing_people_claiming_jira_id_have_one_stable_owner() {
    let ctx = RepoTestContext::new().await;
    let alice = ctx.repos.org.create_person(person("alice")).await.unwrap();
    let bob = ctx.repos.org.create_person(person("bob")).await.unwrap();
    let (alice_claim, bob_claim) = tokio::join!(
        ctx.repos
            .org
            .add_person_identity(alice.person.id.into(), jira("Alice Smith")),
        ctx.repos
            .org
            .add_person_identity(bob.person.id.into(), jira("Bob Jones"))
    );
    assert_ne!(alice_claim.is_ok(), bob_claim.is_ok());
    let (winner, loser) = if let Ok(result) = alice_claim {
        (result, bob_claim)
    } else {
        (bob_claim.unwrap(), alice_claim)
    };
    assert!(matches!(loser, Err(Error::Conflict(_))));
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[alice.person.id, bob.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].person_id, winner.person.id);
    assert_eq!(
        identities[0].platform_user_id.as_deref(),
        Some("Shared:OpaqueID")
    );
    assert!(identities[0].platform_username.contains(' '));
    ctx.teardown().await;
}

#[tokio::test]
async fn concurrent_jira_csv_and_manual_account_claims_never_transfer_owner() {
    let ctx = RepoTestContext::new().await;
    let alice = ctx.repos.org.create_person(person("alice")).await.unwrap();
    let bob = ctx.repos.org.create_person(person("bob")).await.unwrap();
    let csv = vec![ps_core::directory::JiraUserRecord {
        email: "bob@example.com".into(),
        display_name: "Bob Jones".into(),
        account_id: "Shared:OpaqueID".into(),
    }];
    let (manual, import) = tokio::join!(
        ctx.repos
            .org
            .add_person_identity(alice.person.id.into(), jira("Alice Smith")),
        ctx.repos.org.import_jira_users(&csv)
    );
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[alice.person.id, bob.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    if manual.is_ok() {
        assert_eq!(identities[0].person_id, alice.person.id);
        match import {
            Ok((count, _, _)) => assert_eq!(count, 0),
            Err(error) => assert!(matches!(error, Error::Conflict(_))),
        }
    } else {
        assert!(matches!(manual, Err(Error::Conflict(_))));
        assert_eq!(import.unwrap().0, 1);
        assert_eq!(identities[0].person_id, bob.person.id);
    }
    let owner = ctx
        .repos
        .org
        .batch_resolve_by_user_id(&Platform::Jira, &["Shared:OpaqueID".into()])
        .await
        .unwrap();
    assert_eq!(owner["Shared:OpaqueID"], identities[0].person_id);
    ctx.teardown().await;
}

#[tokio::test]
async fn delayed_unmatched_resolution_cannot_replace_manual_add_or_removal() {
    let ctx = RepoTestContext::new().await;
    let alice = ctx.repos.org.create_person(person("alice")).await.unwrap();
    let platform = Platform::Discourse("ubuntu".into());
    // Capture the pending row exactly as a worker would before remote lookup.
    sqlx::query!(
        "INSERT INTO org.identity_resolutions (person_id,platform) VALUES ($1,'discourse-ubuntu')",
        alice.person.id
    )
    .execute(&ctx.pool)
    .await
    .unwrap();
    let snapshot = ctx
        .repos
        .org
        .get_pending_resolutions(&platform.to_string())
        .await
        .unwrap();
    assert_eq!(snapshot.len(), 1);
    let saved = ctx
        .repos
        .org
        .add_person_identity(
            alice.person.id.into(),
            IdentityInput {
                platform: platform.clone(),
                username: "selected".into(),
                platform_user_id: None,
            },
        )
        .await
        .unwrap();
    for remove in [false, true] {
        if remove {
            ctx.repos
                .org
                .remove_person_identity(alice.person.id.into(), saved.identities[0].id)
                .await
                .unwrap();
        }
        ctx.repos
            .org
            .mark_unresolved(snapshot[0].person_id, &platform.to_string())
            .await
            .unwrap();
        let statuses = ctx
            .repos
            .org
            .get_resolution_statuses(alice.person.id)
            .await
            .unwrap();
        assert!(statuses.iter().any(
            |(key, status)| key == &platform.to_string() && *status == ResolutionStatus::Manual
        ));
        assert!(matches!(
            ctx.repos
                .org
                .resolve_identity(snapshot[0].person_id, &platform.to_string(), "outdated")
                .await,
            Err(Error::Conflict(_))
        ));
        let identities = ctx
            .repos
            .org
            .get_identities_for_people(&[alice.person.id])
            .await
            .unwrap();
        assert!(identities.iter().all(|i| i.platform_username != "outdated"));
        assert_eq!(identities.len(), usize::from(!remove));
    }
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_csv_waiting_on_manual_person_lock_rechecks_manual_choice() {
    let ctx = RepoTestContext::new().await;
    let alice = ctx.repos.org.create_person(person("alice")).await.unwrap();
    // Hold the same person lock as a manual write; its uncommitted account and
    // resolution marker must be observed when the waiting importer proceeds.
    let mut tx = ctx.pool.begin().await.unwrap();
    sqlx::query!(
        "SELECT id FROM org.people WHERE id=$1 FOR UPDATE",
        alice.person.id
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let account_id = uuid::Uuid::now_v7();
    sqlx::query!("INSERT INTO org.platform_identities (id,person_id,platform,platform_username,platform_user_id,management) VALUES ($1,$2,'jira','chosen display','ChosenID','manual')",account_id,alice.person.id).execute(&mut *tx).await.unwrap();
    sqlx::query!("INSERT INTO org.identity_resolutions (person_id,platform,status) VALUES ($1,'jira','manual')",alice.person.id).execute(&mut *tx).await.unwrap();
    let repo = ctx.repos.org.clone();
    let pending = tokio::spawn(async move {
        repo.import_jira_users(&[ps_core::directory::JiraUserRecord {
            email: "alice@example.com".into(),
            display_name: "Stale display".into(),
            account_id: "StaleID".into(),
        }])
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let blocked=sqlx::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND wait_event_type='Lock') AS "blocked!""#).fetch_one(&ctx.pool).await.unwrap();
            if blocked {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("importer should block on manual write lock");
    tx.commit().await.unwrap();
    let imported = pending.await.unwrap().unwrap();
    assert_eq!(imported.0, 0);
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[alice.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].platform_user_id.as_deref(), Some("ChosenID"));
    ctx.teardown().await;
}

#[tokio::test]
async fn database_rejects_case_collisions_and_identity_owner_changes() {
    let ctx = RepoTestContext::new().await;
    let alice = ctx.repos.org.create_person(person("alice")).await.unwrap();
    let bob = ctx.repos.org.create_person(person("bob")).await.unwrap();
    let identity_id = uuid::Uuid::now_v7();
    // A writer that bypasses normalization still cannot introduce another owner.
    sqlx::query!("INSERT INTO org.platform_identities (id,person_id,platform,platform_username) VALUES ($1,$2,'github','MixedCase')",identity_id,alice.person.id).execute(&ctx.pool).await.unwrap();
    let claimed = ctx
        .repos
        .org
        .add_person_identity(
            bob.person.id.into(),
            IdentityInput {
                platform: Platform::Github,
                username: "mixedcase".into(),
                platform_user_id: None,
            },
        )
        .await;
    assert!(matches!(claimed, Err(Error::Conflict(_))));
    let transferred = sqlx::query!(
        "UPDATE org.platform_identities SET person_id=$2 WHERE id=$1",
        identity_id,
        bob.person.id
    )
    .execute(&ctx.pool)
    .await
    .unwrap_err();
    assert!(
        transferred
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    );
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[alice.person.id, bob.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].person_id, alice.person.id);
    ctx.teardown().await;
}
