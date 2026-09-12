use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::{CandidateSnapshotMaterial, Error, Result};
use uuid::Uuid;

pub(super) async fn insert(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    planning_latest_input: i64,
    material: &CandidateSnapshotMaterial,
) -> Result<Uuid> {
    let program_body = serde_json::to_string(&material.program).map_err(storage_error)?;
    let program_digest = content(transaction, tenant_id, workspace_id, &program_body).await?;
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(sequence),0)+1 FROM scope_candidate_snapshots \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let selected_ids: Vec<Uuid> = material
        .selected_worktrees
        .iter()
        .map(|value| value.id)
        .collect();
    let rules = serde_json::to_value(&material.rules).map_err(storage_error)?;
    let snapshot_id: Uuid = sqlx::query_scalar(
        "INSERT INTO scope_candidate_snapshots \
             (tenant_id,workspace_id,candidate_set_id,sequence,program_revision,\
              program_latest_input,planning_latest_input,program_body_digest,\
              selected_worktree_ids,selected_sources_digest,method_id,method_revision,\
              method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING id",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(sequence)
    .bind(material.program.revision)
    .bind(material.program.latest_input)
    .bind(planning_latest_input)
    .bind(&program_digest)
    .bind(&selected_ids)
    .bind(&material.selected_sources_digest)
    .bind(&material.method.id)
    .bind(&material.method.revision)
    .bind(&material.method.digest)
    .bind(&material.method.body)
    .bind(serde_json::to_value(&material.method.origin_refs).map_err(storage_error)?)
    .bind(&material.registry_revision)
    .bind(&material.registry_digest)
    .bind(rules)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    for (field, body) in [
        ("name", material.program.name.as_deref()),
        ("intent", material.program.intent.as_deref()),
        ("basis", material.program.basis.as_deref()),
        ("boundaries", material.program.boundaries.as_deref()),
        ("constraints", material.program.constraints.as_deref()),
        ("success", material.program.success.as_deref()),
    ] {
        let Some(body) = body.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        insert_source(
            transaction,
            tenant_id,
            workspace_id,
            candidate_set_id,
            snapshot_id,
            if field == "success" {
                "program_success"
            } else {
                "program_field"
            },
            None,
            Some(field),
            body,
            &format!("Captured Program {field}"),
        )
        .await?;
    }
    let inputs: Vec<(i64, String)> = sqlx::query_as(
        "SELECT sequence,input FROM scope_candidate_inputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND sequence<=$4 \
         ORDER BY sequence",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(planning_latest_input)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    for (sequence, input) in inputs {
        insert_source(
            transaction,
            tenant_id,
            workspace_id,
            candidate_set_id,
            snapshot_id,
            "planning_input",
            Some(sequence),
            None,
            &input,
            &format!("Planning request input {sequence}"),
        )
        .await?;
    }
    Ok(snapshot_id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_source(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    snapshot_id: Uuid,
    kind: &str,
    input_sequence: Option<i64>,
    program_field: Option<&str>,
    body: &str,
    label: &str,
) -> Result<()> {
    let digest = content(transaction, tenant_id, workspace_id, body).await?;
    sqlx::query(
        "INSERT INTO scope_candidate_source_refs \
             (tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,input_sequence,program_field,\
              body_digest,label) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .bind(kind)
    .bind(input_sequence)
    .bind(program_field)
    .bind(digest)
    .bind(label)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn content(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    body: &str,
) -> Result<String> {
    let digest = hex(&Sha256::digest(body.as_bytes()));
    sqlx::query(
        "INSERT INTO scope_candidate_contents (tenant_id,workspace_id,digest,body) \
         VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(&digest)
    .bind(body)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let matches: bool = sqlx::query_scalar(
        "SELECT body=$4 FROM scope_candidate_contents \
         WHERE tenant_id=$1 AND workspace_id=$2 AND digest=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(&digest)
    .bind(body)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if matches {
        Ok(digest)
    } else {
        Err(Error::InternalInvariant)
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    value
}
