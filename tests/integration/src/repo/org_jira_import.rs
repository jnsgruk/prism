use crate::common::db::RepoTestContext;
use ps_core::directory::JiraUserRecord;
use ps_core::repo::org::CreatePersonParams;

#[tokio::test]
async fn jira_csv_skips_ambiguous_batch_claims_and_preserves_unrelated_rows() {
    let ctx = RepoTestContext::new().await;

    for name in ["alice", "bob", "carol", "valid"] {
        ctx.repos
            .org
            .create_person(CreatePersonParams {
                name: name.into(),
                email: Some(format!("{name}@example.com")),
                level: None,
                team_id: None,
                identities: vec![],
            })
            .await
            .unwrap();
    }

    let records: Vec<_> = [
        ("alice", "first"),
        ("alice", "second"),
        ("bob", "shared"),
        ("carol", "shared"),
        ("valid", "MixedCase:ID"),
        ("valid", "MixedCase:ID"),
    ]
    .into_iter()
    .map(|(name, id)| JiraUserRecord {
        display_name: name.into(),
        email: format!("{name}@example.com"),
        account_id: id.into(),
    })
    .collect();
    let (mapped, unmatched, warnings) = ctx.repos.org.import_jira_users(&records).await.unwrap();

    assert_eq!(mapped, 1);
    assert_eq!(unmatched, 4);
    assert_eq!(warnings.len(), 4);

    let export = ctx.repos.org.export_org().await.unwrap();

    for person in export.people {
        if person.name == "valid" {
            assert_eq!(
                person.identities[0].platform_user_id.as_deref(),
                Some("MixedCase:ID")
            );
        } else {
            assert!(person.identities.is_empty());
        }
    }

    ctx.teardown().await;
}

fn jira_record(email: &str, account_id: &str) -> JiraUserRecord {
    JiraUserRecord {
        display_name: "Jira user".into(),
        email: email.into(),
        account_id: account_id.into(),
    }
}

#[tokio::test]
async fn jira_reimport_preserves_identity_uuid_and_matches_changed_email_by_account_id() {
    use super::org_manual_import::manual;

    let ctx = RepoTestContext::new().await;
    let person = ctx
        .repos
        .org
        .create_person(manual("Owner", Some("old@example.com")))
        .await
        .unwrap()
        .person;
    let original = jira_record(" OLD@example.com ", "MixedCase:ID");
    for _ in 0..2 {
        let result = ctx
            .repos
            .org
            .import_jira_users(std::slice::from_ref(&original))
            .await
            .unwrap();
        assert_eq!(result, (1, 0, vec![]));
    }
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    let identity_id = identities[0].id;

    // The CSV email may change independently of the canonical directory email.
    for email in ["new@example.com", "NEW@example.com"] {
        let result = ctx
            .repos
            .org
            .import_jira_users(&[jira_record(email, "MixedCase:ID")])
            .await
            .unwrap();
        assert_eq!(result, (1, 0, vec![]));
        let identities = ctx
            .repos
            .org
            .get_identities_for_people(&[person.id])
            .await
            .unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].id, identity_id);
        assert_eq!(identities[0].platform_username, "new@example.com");
        assert_eq!(
            identities[0].platform_user_id.as_deref(),
            Some("MixedCase:ID")
        );
    }
    let saved = ctx.repos.org.get_person(person.id).await.unwrap().unwrap();
    assert_eq!(saved.email.as_deref(), Some("old@example.com"));

    // Stable accounts also work when the canonical person has no email.
    ctx.repos
        .org
        .update_person(person.id, None, Some(""), None)
        .await
        .unwrap();
    assert_eq!(
        ctx.repos
            .org
            .import_jira_users(&[jira_record("latest@example.com", "MixedCase:ID")])
            .await
            .unwrap(),
        (1, 0, vec![])
    );
    assert_eq!(ctx.repos.org.list_people(false).await.unwrap().len(), 1);
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_reimport_skips_conflicting_emails_and_inactive_account_owners() {
    use super::org_manual_import::manual;

    let ctx = RepoTestContext::new().await;
    let owner = ctx
        .repos
        .org
        .create_person(manual("Owner", Some("owner@example.com")))
        .await
        .unwrap()
        .person;
    let other = ctx
        .repos
        .org
        .create_person(manual("Other", Some("other@example.com")))
        .await
        .unwrap()
        .person;
    assert_eq!(
        ctx.repos
            .org
            .import_jira_users(&[jira_record("owner@example.com", "account")])
            .await
            .unwrap(),
        (1, 0, vec![])
    );
    let original = ctx
        .repos
        .org
        .get_identities_for_people(&[owner.id])
        .await
        .unwrap()
        .remove(0);

    let (mapped, unmatched, warnings) = ctx
        .repos
        .org
        .import_jira_users(&[jira_record("other@example.com", "account")])
        .await
        .unwrap();
    assert_eq!((mapped, unmatched), (0, 1));
    assert!(warnings[0].contains("conflicting ownership"));
    assert!(
        ctx.repos
            .org
            .get_identities_for_people(&[other.id])
            .await
            .unwrap()
            .is_empty()
    );

    ctx.repos.org.deactivate_person(owner.id).await.unwrap();
    let (mapped, unmatched, warnings) = ctx
        .repos
        .org
        .import_jira_users(&[
            jira_record("new@example.com", "account"),
            jira_record("owner@example.com", "new-account"),
        ])
        .await
        .unwrap();
    assert_eq!((mapped, unmatched), (0, 2));
    assert!(warnings.iter().all(|warning| warning.contains("inactive")));
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[owner.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].id, original.id);
    assert_eq!(identities[0].platform_username, original.platform_username);
    assert!(
        !ctx.repos
            .org
            .get_person(owner.id)
            .await
            .unwrap()
            .unwrap()
            .active
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_email_fallback_rejects_ambiguous_matches_including_inactive_people() {
    use super::org_manual_import::manual;

    let ctx = RepoTestContext::new().await;
    ctx.repos
        .org
        .create_person(manual("First", Some("same@example.com")))
        .await
        .unwrap();
    let second = ctx
        .repos
        .org
        .create_person(manual("Second", Some("SAME@example.com")))
        .await
        .unwrap()
        .person;
    for deactivate in [false, true] {
        if deactivate {
            ctx.repos.org.deactivate_person(second.id).await.unwrap();
        }
        let (mapped, unmatched, warnings) = ctx
            .repos
            .org
            .import_jira_users(&[jira_record(" same@example.com ", "account")])
            .await
            .unwrap();
        assert_eq!((mapped, unmatched), (0, 1));
        assert!(warnings[0].contains("ambiguous email"));
    }
    let export = ctx.repos.org.export_org().await.unwrap();
    assert_eq!(export.people.len(), 2);
    assert!(
        export
            .people
            .iter()
            .all(|person| person.identities.is_empty())
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn jira_reimport_promotes_legacy_identity_in_place_and_preserves_manual_accounts() {
    use super::org_manual_import::{directory, manual};
    use ps_core::models::Platform;
    use ps_core::repo::org::{IdentityInput, ImportIdentity};

    let ctx = RepoTestContext::new().await;
    let mut legacy = directory("Legacy", Some("legacy@example.com"), Some("directory-id"));
    legacy.identities.push(ImportIdentity {
        platform: Platform::Jira.to_string(),
        username: "legacy@example.com".into(),
    });
    ctx.repos
        .org
        .import_records(&[legacy], false)
        .await
        .unwrap();
    let legacy_person = ctx.repos.org.list_people(false).await.unwrap().remove(0);
    let legacy_identity = ctx
        .repos
        .org
        .get_identities_for_people(&[legacy_person.id])
        .await
        .unwrap()
        .remove(0);
    assert_eq!(legacy_identity.platform_user_id, None);
    assert_eq!(
        ctx.repos
            .org
            .import_jira_users(&[jira_record("legacy@example.com", "legacy-account")])
            .await
            .unwrap(),
        (1, 0, vec![])
    );
    let promoted = ctx
        .repos
        .org
        .get_identities_for_people(&[legacy_person.id])
        .await
        .unwrap();
    assert_eq!(promoted.len(), 1);
    assert_eq!(promoted[0].id, legacy_identity.id);
    assert_eq!(
        promoted[0].platform_user_id.as_deref(),
        Some("legacy-account")
    );

    let mut params = manual("Manual", Some("manual@example.com"));
    params.identities.push(IdentityInput {
        platform: Platform::Jira,
        username: "manual@example.com".into(),
        platform_user_id: Some("manual-account".into()),
    });
    let manual_person = ctx.repos.org.create_person(params).await.unwrap();
    let (mapped, unmatched, warnings) = ctx
        .repos
        .org
        .import_jira_users(&[jira_record("changed@example.com", "manual-account")])
        .await
        .unwrap();
    assert_eq!((mapped, unmatched), (0, 1));
    assert!(warnings[0].contains("protected accounts"));
    let identities = ctx
        .repos
        .org
        .get_identities_for_people(&[manual_person.person.id])
        .await
        .unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].id, manual_person.identities[0].id);
    assert_eq!(identities[0].platform_username, "manual@example.com");
    ctx.teardown().await;
}
