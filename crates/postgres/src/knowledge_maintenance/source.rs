use super::*;

pub(super) async fn verified_logical_uri(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    source_iri: &str,
    accepted_digest: &str,
) -> Result<Option<String>> {
    let row: Option<(String, Uuid, bool)> = sqlx::query_as(
        "SELECT contract_version,publication_event_id,payload_erased \
         FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 \
          AND unit_id=$3 AND revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((contract, event, erased)) = row else {
        return Ok(None);
    };
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    match contract.as_str() {
        "dk-2" => {
            let verified = crate::knowledge_lifecycle::verify_publication_event(
                tx, tenant, workspace, unit, revision, event, true,
            )
            .await?;
            Ok(verified
                .input
                .resolved_sources
                .into_iter()
                .find(|source| {
                    source.pin.source_iri == source_iri && source.pin.digest == accepted_digest
                })
                .map(|source| source.uri))
        }
        "dk-1" => {
            let value = crate::durable_knowledge::context::load_revision(
                tx,
                tenant,
                workspace,
                unit,
                Some(revision),
                true,
            )
            .await?
            .ok_or(Error::InternalInvariant)?;
            Ok(
                (value.source_iri == source_iri && value.source_sha256 == accepted_digest)
                    .then_some(value.constraint.source.uri),
            )
        }
        _ => Err(Error::InternalInvariant),
    }
}
