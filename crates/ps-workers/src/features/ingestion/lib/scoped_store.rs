use ps_core::{
    Error,
    ingestion::{ContributionInput, IngestionContext},
};

/// The shared, fail-closed persistence path for every person adapter.
pub async fn store_person_batch(
    ctx: &IngestionContext,
    items: &[ContributionInput],
) -> Result<Option<usize>, Error> {
    let Some(request) = ctx.person_request()? else {
        return Ok(None);
    };
    let run_id = ctx
        .run_id
        .ok_or_else(|| Error::Validation("scoped ingestion run ID is required".into()))?;
    let stored = ctx
        .repos
        .activity
        .store_person_contributions(request, run_id, items)
        .await?;
    Ok(Some(stored))
}
