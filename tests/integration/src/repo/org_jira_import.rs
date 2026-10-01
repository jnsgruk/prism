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
