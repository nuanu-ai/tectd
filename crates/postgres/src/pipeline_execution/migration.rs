use super::{digest, enum_text, json, storage_error};
use sqlx::{Postgres, Transaction};
use tect_domain::{
    Error, PipelineDefinitionSnapshot, PipelineRunMigrationCommand, PipelineRunMigrationOutcome,
    PipelineRunMigrationRequest, Result,
};
use uuid::Uuid;

/// Migrate one immutable legacy run to a distinct pinned successor in the same
/// transaction. The predecessor definition and all caller-visible history are
/// read from storage; the command only supplies the explicit mapping.
pub(crate) async fn migrate_run(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    _session: Uuid,
    request: &PipelineRunMigrationCommand,
    definition: &PipelineDefinitionSnapshot,
) -> Result<PipelineRunMigrationOutcome> {
    request.validate()?;
    let request_payload = json(request)?;

    if let Some(existing) = sqlx::query_as::<_, (serde_json::Value, Uuid, Uuid, i64, String, String)>(
        "SELECT request_payload, migration_id, predecessor_run_id, predecessor_revision, successor_definition_version, successor_definition_digest FROM slice_pipeline_run_migrations WHERE tenant_id=$1 AND workspace_id=$2 AND idempotency_key=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(&request.idempotency_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    {
        if existing.0 != request_payload {
            return Err(Error::InputConflict);
        }
        let successor_run_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT successor_run_id FROM slice_pipeline_run_migrations WHERE tenant_id=$1 AND workspace_id=$2 AND migration_id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(existing.1)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
        return Ok(PipelineRunMigrationOutcome {
            migration_id: existing.1,
            predecessor_run_id: existing.2,
            successor_run_id,
            predecessor_revision: existing.3,
            successor_definition_version: existing.4,
            successor_definition_digest: existing.5,
            status: "replayed".into(),
        });
    }

    let predecessor = sqlx::query_as::<_, (Uuid, Uuid, i64, i64, String, String, String, serde_json::Value, String, Option<String>, Option<i32>)>(
        "SELECT scope_id, slice_id, slice_revision, revision, definition_kind, definition_version, definition_digest, definition, delivery_mode, current_phase_id, current_phase_ordinal FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.predecessor_run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;

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
    predecessor_request.validate()?;
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
