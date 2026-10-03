use super::*;
use crate::knowledge_lifecycle::{CandidateProofError, PublicationProofScope};

#[allow(clippy::too_many_arguments)]
pub(super) async fn selected(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    rows: &[BindingRow],
    projection: &Option<crate::durable_knowledge::manifest::inquiry::Projection>,
    definition: &PipelineDefinitionSnapshot,
    definition_version: &str,
    definition_digest: &str,
    owner: bool,
    proof_scope: &mut PublicationProofScope,
) -> std::result::Result<(), CandidateProofError> {
    let typed_rows = rows
        .iter()
        .filter(|row| {
            let projection_allows = projection
                .as_ref()
                .is_none_or(|value| value.allows_binding(&row.binding_kind));
            let pin_matches = row.binding_kind != "slice_phase"
                || (row.definition_kind.as_deref() == Some(definition.kind.as_str())
                    && row.definition_version.as_deref() == Some(definition_version)
                    && row.definition_digest.as_deref() == Some(definition_digest));
            let inaccessible = (row.head_access == "owners_only"
                || row.revision_access.as_deref() == Some("owners_only"))
                && !owner;
            projection_allows
                && row.revision_contract.as_deref() == Some("dk-2")
                && !inaccessible
                && row.binding_active
                && row.head_active
                && !row.head_payload_erased
                && row.revision_payload_erased == Some(false)
                && row.lifecycle == "active"
                && pin_matches
                && row.event_id.is_some()
        })
        .collect::<Vec<_>>();
    let mut keys = typed_rows
        .iter()
        .map(|row| {
            Ok(crate::knowledge_lifecycle::PublicationProofKey {
                unit_id: row.unit_id,
                revision: row.revision,
                event_id: row
                    .event_id
                    .ok_or_else(|| CandidateProofError::refusal(Error::InternalInvariant))?,
                include_revision: true,
            })
        })
        .collect::<std::result::Result<Vec<_>, CandidateProofError>>()?;
    proof_scope.preload_candidate(tx, &keys).await?;
    keys.clear();
    let mut requested = BTreeSet::new();
    for row in &typed_rows {
        let verified = proof_scope
            .verify_candidate(
                tx,
                crate::knowledge_lifecycle::PublicationProofKey {
                    unit_id: row.unit_id,
                    revision: row.revision,
                    event_id: row
                        .event_id
                        .ok_or_else(|| CandidateProofError::refusal(Error::InternalInvariant))?,
                    include_revision: true,
                },
            )
            .await?;
        // Preserve scalar same-resource refusal order: creation proof,
        // selected revision digest, then latest revalidation proof.
        resource::verify_revision_digest(row.rdf_digest.as_deref(), &verified.rdf_digest)
            .map_err(CandidateProofError::proof_refusal)?;
        let Some(document) = verified.input.planned.document.as_ref() else {
            continue;
        };
        let purpose: KnowledgeBindingPurpose =
            decode(serde_json::Value::String(row.purpose.clone()))
                .map_err(CandidateProofError::proof_refusal)?;
        let selection = projection
            .as_ref()
            .map(|value| value.select(document, purpose))
            .unwrap_or(super::super::inquiry::BriefSelection::Full);
        if !matches!(
            selection,
            super::super::inquiry::BriefSelection::Omit { .. }
        ) {
            requested.insert((row.unit_id, row.revision));
        }
    }
    if !requested.is_empty() {
        // Revalidation is read only for documents rendered by the current
        // projection, matching typed()'s early return for omitted resources.
        // Consumers still re-read metadata and check exact source pins.
        let units = requested.iter().map(|(unit, _)| *unit).collect::<Vec<_>>();
        let revisions = requested
            .iter()
            .map(|(_, revision)| *revision)
            .collect::<Vec<_>>();
        let latest: Vec<(Uuid, i64, Uuid)> = sqlx::query_as(
                "SELECT requested.unit_id,requested.revision,latest.id FROM unnest($3::uuid[],$4::bigint[]) AS requested(unit_id,revision) CROSS JOIN LATERAL (SELECT v.id FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=requested.unit_id AND v.unit_revision=requested.revision AND NOT v.payload_erased ORDER BY v.created_at DESC,v.id DESC LIMIT 1) latest",
            ).bind(tenant).bind(workspace).bind(&units).bind(&revisions).fetch_all(&mut **tx).await.map_err(CandidateProofError::storage)?;
        let mut seen = BTreeSet::new();
        for (unit, revision, event) in latest {
            if !requested.contains(&(unit, revision)) || !seen.insert((unit, revision)) {
                return Err(CandidateProofError::refusal(Error::InternalInvariant));
            }
            keys.push(crate::knowledge_lifecycle::PublicationProofKey {
                unit_id: unit,
                revision,
                event_id: event,
                include_revision: false,
            });
        }
    }
    proof_scope.preload_candidate(tx, &keys).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn ordinary(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    rows: &[BindingRow],
    projection: &Option<crate::durable_knowledge::manifest::inquiry::Projection>,
    definition: &PipelineDefinitionSnapshot,
    definition_version: &str,
    definition_digest: &str,
    owner: bool,
    proof_scope: &mut PublicationProofScope,
) -> Result<()> {
    // Bound before any speculative query or candidate allocation. Conservatively
    // skip even duplicate/filtered binding rows above the candidate key budget.
    if rows.len() > 64 {
        tect_application::request_diagnostics::count("proof.candidate_cap_keys", 1);
        return Ok(());
    }
    tect_application::request_diagnostics::count("proof.candidate_attempts", 1);
    let mut candidate = proof_scope.candidate();
    sqlx::query("SAVEPOINT ordinary_context_proof_candidate")
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    match selected(
        tx,
        tenant,
        workspace,
        rows,
        projection,
        definition,
        definition_version,
        definition_digest,
        owner,
        &mut candidate,
    )
    .await
    {
        Ok(()) => {
            sqlx::query("RELEASE SAVEPOINT ordinary_context_proof_candidate")
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
            tect_application::request_diagnostics::count("proof.candidate_adopted", 1);
            proof_scope.adopt_candidate(candidate);
        }
        Err(CandidateProofError::Refusal(_)) => {
            candidate.record_candidate_budget();
            tect_application::request_diagnostics::count("proof.candidate_fallbacks", 1);
            // Candidate contains reads only; recover transaction before the
            // unchanged ordinary walk selects its original first refusal.
            sqlx::query("ROLLBACK TO SAVEPOINT ordinary_context_proof_candidate")
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
            sqlx::query("RELEASE SAVEPOINT ordinary_context_proof_candidate")
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
        }
        Err(CandidateProofError::Terminal(error)) => {
            candidate.record_candidate_budget();
            // Unknown storage/cancellation may precede a latent semantic error.
            // Fail closed without retry; only known proof refusals promise
            // the ordinary first-error order in a usable transaction.
            tect_application::request_diagnostics::count("proof.candidate_terminal", 1);
            return Err(error);
        }
    }
    Ok(())
}
