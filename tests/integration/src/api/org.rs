use crate::define_api_test;
use ps_proto::prism::v1::org_service_client::OrgServiceClient;
use ps_proto::prism::v1::{
    GetTeamRequest, ImportDirectoryRequest, ListPeopleRequest, ListTeamsRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

define_api_test!(list_teams_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = OrgServiceClient::new(server.channel.clone());

    let mut req = Request::new(ListTeamsRequest {
        parent_team_id: None,
    });
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );

    let resp = client
        .list_teams(req)
        .await
        .expect("list_teams")
        .into_inner();

    assert!(resp.teams.is_empty());
});

/// Helper: build a directory JSON payload from a slice of records.
fn directory_json(records: &[serde_json::Value]) -> Vec<u8> {
    serde_json::to_vec(records).expect("serialize directory JSON")
}

define_api_test!(
    import_directory_creates_people_and_teams,
    |server| async move {
        let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
        let mut client = OrgServiceClient::new(server.channel.clone());

        let payload = directory_json(&[
            serde_json::json!({
                "name": "Alice Smith",
                "email": "alice@example.com",
                "level": "Senior",
                "directory_id": "alice-1",
                "team": "Platform",
                "org": "Engineering",
                "identities": [
                    {"platform": "github", "username": "alicegh"},
                    {"platform": "jira", "username": "asmith"}
                ]
            }),
            serde_json::json!({
                "name": "Bob Jones",
                "email": "bob@example.com",
                "directory_id": "bob-1",
                "team": "Platform",
                "org": "Engineering",
                "identities": [
                    {"platform": "github", "username": "bobgh"}
                ]
            }),
        ]);

        let mut req = Request::new(ImportDirectoryRequest {
            file_content: payload,
        });
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
        );

        let resp = client
            .import_directory(req)
            .await
            .expect("import_directory")
            .into_inner();

        assert_eq!(resp.people_imported, 2);
        assert_eq!(resp.teams_created, 1);
        assert_eq!(resp.identities_mapped, 3);
        assert!(resp.warnings.is_empty());
    }
);

define_api_test!(list_people_after_import, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = OrgServiceClient::new(server.channel.clone());

    // Import two people
    let payload = directory_json(&[
        serde_json::json!({
            "name": "Alice Smith",
            "email": "alice@example.com",
            "directory_id": "alice-1",
            "identities": [{"platform": "github", "username": "alicegh"}]
        }),
        serde_json::json!({
            "name": "Bob Jones",
            "directory_id": "bob-1",
            "identities": []
        }),
    ]);

    let mut req = Request::new(ImportDirectoryRequest {
        file_content: payload,
    });
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
    client
        .import_directory(req)
        .await
        .expect("import_directory");

    // List people
    let mut req = Request::new(ListPeopleRequest {});
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );

    let resp = client
        .list_people(req)
        .await
        .expect("list_people")
        .into_inner();

    assert_eq!(resp.people.len(), 2);

    // People are ordered by name
    let alice = &resp.people[0];
    assert_eq!(alice.name, "Alice Smith");
    assert_eq!(alice.email.as_deref(), Some("alice@example.com"));
    assert_eq!(alice.identities.len(), 1);
    assert_eq!(alice.identities[0].platform, "github");
    assert_eq!(alice.identities[0].username, "alicegh");

    let bob = &resp.people[1];
    assert_eq!(bob.name, "Bob Jones");
    assert!(bob.identities.is_empty());
});

define_api_test!(get_team_returns_members, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = OrgServiceClient::new(server.channel.clone());

    // Import people with team assignments
    let payload = directory_json(&[
        serde_json::json!({
            "name": "Alice Smith",
            "directory_id": "alice-1",
            "team": "Platform",
            "org": "Engineering",
            "identities": [{"platform": "github", "username": "alicegh"}]
        }),
        serde_json::json!({
            "name": "Bob Jones",
            "directory_id": "bob-1",
            "team": "Platform",
            "org": "Engineering",
            "identities": []
        }),
    ]);

    let mut req = Request::new(ImportDirectoryRequest {
        file_content: payload,
    });
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
    client
        .import_directory(req)
        .await
        .expect("import_directory");

    // List teams to get the team ID
    let mut req = Request::new(ListTeamsRequest {
        parent_team_id: None,
    });
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
    let teams_resp = client
        .list_teams(req)
        .await
        .expect("list_teams")
        .into_inner();

    assert_eq!(teams_resp.teams.len(), 1);
    let team = &teams_resp.teams[0];
    assert_eq!(team.name, "Platform");
    assert_eq!(team.org_name, "Engineering");
    assert_eq!(team.member_count, 2);

    // Get team details with members
    let mut req = Request::new(GetTeamRequest {
        team_id: team.id.clone(),
    });
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
    let team_resp = client.get_team(req).await.expect("get_team").into_inner();

    let returned_team = team_resp.team.expect("team should be present");
    assert_eq!(returned_team.name, "Platform");
    assert_eq!(team_resp.members.len(), 2);

    // Members ordered by name
    assert_eq!(team_resp.members[0].name, "Alice Smith");
    assert_eq!(team_resp.members[0].identities.len(), 1);
    assert_eq!(team_resp.members[1].name, "Bob Jones");
});

define_api_test!(
    import_directory_upserts_by_directory_id,
    |server| async move {
        let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
        let mut client = OrgServiceClient::new(server.channel.clone());

        // First import
        let payload = directory_json(&[serde_json::json!({
            "name": "Alice Smith",
            "email": "alice@old.com",
            "directory_id": "alice-1",
            "team": "Platform",
            "org": "Engineering",
            "identities": [{"platform": "github", "username": "alicegh"}]
        })]);

        let mut req = Request::new(ImportDirectoryRequest {
            file_content: payload,
        });
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
        );
        let first = client
            .import_directory(req)
            .await
            .expect("first import")
            .into_inner();

        assert_eq!(first.people_imported, 1);
        assert_eq!(first.teams_created, 1);

        // Second import with same directory_id but updated email
        let payload = directory_json(&[serde_json::json!({
            "name": "Alice Smith-Updated",
            "email": "alice@new.com",
            "directory_id": "alice-1",
            "team": "Platform",
            "org": "Engineering",
            "identities": [{"platform": "github", "username": "alicegh"}]
        })]);

        let mut req = Request::new(ImportDirectoryRequest {
            file_content: payload,
        });
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
        );
        let second = client
            .import_directory(req)
            .await
            .expect("second import")
            .into_inner();

        // Should not create a new person (upsert by directory_id)
        assert_eq!(second.people_imported, 0);
        // Team already exists
        assert_eq!(second.teams_created, 0);

        // Verify only one person exists with the updated name
        let mut req = Request::new(ListPeopleRequest {});
        req.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
        );
        let people = client
            .list_people(req)
            .await
            .expect("list_people")
            .into_inner();

        assert_eq!(people.people.len(), 1);
        assert_eq!(people.people[0].name, "Alice Smith-Updated");
        assert_eq!(people.people[0].email.as_deref(), Some("alice@new.com"));
    }
);
