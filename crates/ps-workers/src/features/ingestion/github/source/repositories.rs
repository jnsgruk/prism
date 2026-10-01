//! Repository bounds shared by person discovery and ordinary member search.
use super::super::types::GraphQLSearchRepo;
use super::is_valid_github_username;

pub(super) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// Unknown archive status leaves coverage incomplete when archived repos are excluded.
pub(super) fn eligible(
    repo: &GraphQLSearchRepo,
    orgs: &[String],
    exclusions: &[String],
    exclude_archived: bool,
) -> Result<bool, ps_core::Error> {
    let owner = &repo.owner.login;
    let name = &repo.name;
    let full_name = format!("{owner}/{name}");
    if !orgs.iter().any(|org| owner.eq_ignore_ascii_case(org))
        || !is_valid_github_username(owner)
        || !valid_name(name)
        || exclusions.iter().any(|excluded| {
            excluded.eq_ignore_ascii_case(name) || excluded.eq_ignore_ascii_case(&full_name)
        })
        || (exclude_archived && repo.is_archived == Some(true))
    {
        return Ok(false);
    }
    if exclude_archived && repo.is_archived.is_none() {
        return Err(ps_core::Error::Validation(
            "repository archive status missing".into(),
        ));
    }
    Ok(true)
}
