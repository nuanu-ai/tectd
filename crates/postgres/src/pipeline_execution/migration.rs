use super::{digest, enum_text, json, storage_error};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::{
    Error, PipelineDefinitionSnapshot, PipelineRunMigrationCommand, PipelineRunMigrationOutcome,
    PipelineRunMigrationRequest, Result,
};
use uuid::Uuid;

/// Infrastructure hashes the domain's canonical typed snapshot bytes.
struct MigrationDefinitionDigest;
impl tect_domain::PipelineDefinitionDigestPort for MigrationDefinitionDigest {
    fn sha256(&self, canonical_json: &[u8]) -> [u8; 32] {
        Sha256::digest(canonical_json).into()
    }
}

#[cfg(test)]
mod hash_tests;

pub(crate) async fn migration_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &PipelineRunMigrationCommand,
) -> Result<Option<PipelineRunMigrationOutcome>> {
    let payload = json(request)?;
    let row: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT request_payload,result_payload FROM slice_pipeline_run_migrations WHERE tenant_id=$1 AND workspace_id=$2 AND idempotency_key=$3")
        .bind(tenant).bind(workspace).bind(&request.idempotency_key)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((stored, result)) = row else {
        return Ok(None);
    };
    if stored != payload {
        return Err(Error::InputConflict);
    }
    let mut outcome: PipelineRunMigrationOutcome = super::decode(result)?;
    outcome.status = "replayed".into();
    Ok(Some(outcome))
}

/// Migrate one immutable legacy run to a distinct pinned successor in the same
/// transaction. The predecessor definition and all caller-visible history are
/// read from storage; the command only supplies the explicit mapping.
pub(crate) async fn migrate_run(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &PipelineRunMigrationCommand,
    definition: &PipelineDefinitionSnapshot,
) -> Result<PipelineRunMigrationOutcome> {
    request.validate()?;
    let principal = super::session_principal(tx, session).await?;
    super::context::load_context_without_delivery_receipt(
        tx,
        tenant,
        workspace,
        principal,
        request.predecessor_run_id,
    )
    .await?
    .ok_or(Error::NotFound)?;
    let request_payload = json(request)?;

    if let Some(replay) = migration_replay(tx, tenant, workspace, request).await? {
        return Ok(replay);
    }

    let predecessor = sqlx::query_as::<_, (Uuid, Uuid, i64, i64, String, String, String, serde_json::Value, String, Option<String>, Option<i32>, String)>(
        "SELECT scope_id, slice_id, slice_revision, revision, definition_kind, definition_version, definition_digest, definition, delivery_mode, current_phase_id, current_phase_ordinal, status FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.predecessor_run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;

    // Serialize concurrent same-key migrations on the predecessor, then recheck
    // the immutable receipt before inspecting the advanced predecessor revision.
    if let Some(replay) = migration_replay(tx, tenant, workspace, request).await? {
        return Ok(replay);
    }
    tect_domain::ensure_pipeline_definition_selectable(definition)
        .map_err(tect_domain::migration_successor_retirement_error)?;
    if request.successor_definition_version != definition.version {
        return Err(Error::InvalidArguments);
    }
    let predecessor_definition: PipelineDefinitionSnapshot = super::decode(predecessor.7.clone())?;
    if !matches!(
        predecessor.11.as_str(),
        "active" | "waiting_input" | "blocked"
    ) {
        return Err(Error::Forbidden);
    }
    if tect_domain::is_retired_lightweight(&predecessor_definition)
        && !tect_domain::is_current_lightweight_retirement_successor(
            definition,
            &MigrationDefinitionDigest,
        )
    {
        return Err(tect_domain::lightweight_retirement_error(
            "arguments.params.successor_definition_version",
            definition.version.clone(),
            "get_current_context_and_use_exact_migration_action",
        ));
    }
    if predecessor.3 != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    if predecessor.5 == request.successor_definition_version {
        return Err(Error::refused_at(
            tect_domain::RefusalCode::LegacyMigrationRequired,
            "WP6-MIGRATION-VERSION-02",
            "arguments.params.successor_definition_version",
            "a definition version different from the predecessor",
            predecessor.5.clone(),
            "select_successor_definition",
            "new_definition_version",
        ));
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM slice_pipeline_run_migrations WHERE tenant_id=$1 AND workspace_id=$2 AND predecessor_run_id=$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.predecessor_run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?
    {
        return Err(tect_domain::Error::refused_at(
            tect_domain::RefusalCode::LegacyMigrationRequired,
            "WP6-MIGRATION-IDEMPOTENCY-01",
            "arguments.params.idempotency_key",
            "unused key or exact replay of the same successor mapping",
            "key already bound to a different migration",
            "reuse_migration_idempotency_key",
            "explicit_successor_mapping",
        ));
    }

    let predecessor_request = PipelineRunMigrationRequest {
        request_id: request.request_id,
        predecessor_run_id: request.predecessor_run_id,
        predecessor_definition_version: predecessor.5.clone(),
        predecessor_definition_digest: predecessor.6.clone(),
        successor_definition_version: definition.version.clone(),
        successor_definition_digest: definition.digest.clone(),
        mappings: request.mappings.clone(),
    };
    predecessor_request.validate_retirement_restart(
        &predecessor_definition,
        definition,
        &MigrationDefinitionDigest,
    )?;
    let first = definition.phases.first().ok_or(Error::InternalInvariant)?;
    let successor_run_id = Uuid::new_v4();
    let successor_request_id = request.request_id;
    let mode = enum_text(&definition.default_mode)?;
    sqlx::query("INSERT INTO slice_pipeline_runs(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,definition_kind,definition_version,definition_digest,definition,delivery_mode,qualification_reason,status,current_phase_id,current_phase_ordinal,origin_request_id,origin_payload,inquiry,source_checkpoint_id,source_checkpoint_digest) VALUES($1,$2,$3,$4,$5,$6,1,$7,$8,$9,$10,$11,$12,'active',$13,$14,$15,$16,NULL,NULL,NULL)")
        .bind(successor_run_id)
        .bind(tenant)
        .bind(workspace)
        .bind(predecessor.0)
        .bind(predecessor.1)
        .bind(predecessor.2)
        .bind(definition.kind.as_str())
        .bind(&definition.version)
        .bind(&definition.digest)
        .bind(json(definition)?)
        .bind(mode)
        .bind(format!("Migrated from predecessor run {}", request.predecessor_run_id))
        .bind(&first.id)
        .bind(first.ordinal as i32)
        .bind(successor_request_id)
        .bind(&request_payload)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;

    let migration_id = Uuid::new_v4();
    let mappings_digest = digest(&request.mappings)?;
    let outcome = PipelineRunMigrationOutcome {
        migration_id,
        predecessor_run_id: request.predecessor_run_id,
        successor_run_id,
        predecessor_revision: predecessor.3,
        successor_definition_version: definition.version.clone(),
        successor_definition_digest: definition.digest.clone(),
        status: "committed".into(),
    };
    let outcome_payload = json(&outcome)?;
    sqlx::query("UPDATE slice_pipeline_runs SET status='superseded', revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND revision=$4")
        .bind(tenant)
        .bind(workspace)
        .bind(request.predecessor_run_id)
        .bind(request.expected_revision)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_pipeline_run_migrations(tenant_id,workspace_id,migration_id,idempotency_key,request_payload,predecessor_run_id,successor_run_id,predecessor_revision,expected_revision,predecessor_definition_version,predecessor_definition_digest,successor_definition_version,successor_definition_digest,mappings,mappings_digest,result_payload,status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,'committed')")
        .bind(tenant)
        .bind(workspace)
        .bind(migration_id)
        .bind(&request.idempotency_key)
        .bind(&request_payload)
        .bind(request.predecessor_run_id)
        .bind(successor_run_id)
        .bind(predecessor.3)
        .bind(request.expected_revision)
        .bind(&predecessor.5)
        .bind(&predecessor.6)
        .bind(&definition.version)
        .bind(&definition.digest)
        .bind(serde_json::to_value(&request.mappings).map_err(storage_error)?)
        .bind(mappings_digest)
        .bind(outcome_payload)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    Ok(outcome)
}
