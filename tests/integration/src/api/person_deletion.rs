use crate::common::server::ApiTestContext;
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

#[tokio::test]
async fn deletion_requires_auth_valid_id_and_deactivation() {
    let ctx = ApiTestContext::new().await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut client = OrgServiceClient::new(ctx.server.channel.clone());
    let person = client
        .create_person(authed(
            proto::CreatePersonRequest {
                name: "Alice".into(),
                ..Default::default()
            },
            &token,
        ))
        .await
        .unwrap()
        .into_inner()
        .person
        .unwrap();
    let request = proto::DeletePersonRequest {
        person_id: person.id.clone(),
    };

    assert_eq!(
        client
            .delete_person(request.clone())
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert_eq!(
        client
            .delete_person(authed(
                proto::DeletePersonRequest {
                    person_id: "bad".into()
                },
                &token
            ))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        client
            .delete_person(authed(
                proto::DeletePersonRequest {
                    person_id: Uuid::now_v7().to_string()
                },
                &token
            ))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        client
            .delete_person(authed(request.clone(), &token))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );

    client
        .deactivate_person(authed(
            proto::DeactivatePersonRequest {
                person_id: person.id.clone(),
            },
            &token,
        ))
        .await
        .unwrap();
    client
        .reactivate_person(authed(
            proto::ReactivatePersonRequest {
                person_id: person.id.clone(),
            },
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(
        client
            .delete_person(authed(request.clone(), &token))
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    client
        .deactivate_person(authed(
            proto::DeactivatePersonRequest {
                person_id: person.id.clone(),
            },
            &token,
        ))
        .await
        .unwrap();
    client
        .delete_person(authed(request.clone(), &token))
        .await
        .unwrap();
    assert_eq!(
        client
            .delete_person(authed(request, &token))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert!(
        client
            .list_people(authed(proto::ListPeopleRequest::default(), &token))
            .await
            .unwrap()
            .into_inner()
            .people
            .is_empty()
    );
    ctx.teardown().await;
}
