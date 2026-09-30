use super::RdfDocument;
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{Error, KnowledgeLifecycleOperation, Result};
use uuid::Uuid;

pub(crate) const SHAPES: &str = include_str!("shapes.ttl");
const QUALIFY_VALID: &str = include_str!("qualify-valid.ttl");

pub(crate) async fn native_publish(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    event: Uuid,
    operation: KnowledgeLifecycleOperation,
    document: &RdfDocument,
) -> Result<String> {
    if operation == KnowledgeLifecycleOperation::Erase || document.payload.is_empty() {
        return Err(Error::InvalidArguments);
    }
    sqlx::query_scalar("SELECT public.tect_dk2_native_publish($1,$2,$3,$4,$5,$6)")
        .bind(tenant)
        .bind(workspace)
        .bind(event)
        .bind(operation_name(operation))
        .bind(&document.payload)
        .bind(&document.stable_payload)
        .fetch_one(&mut **tx)
        .await
        .map_err(native_error)
}

pub(crate) async fn native_rows(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    event: Uuid,
    include_revision: bool,
) -> Result<Vec<serde_json::Value>> {
    sqlx::query_scalar("SELECT * FROM public.tect_dk2_native_read($1,$2,$3,$4,$5,$6)")
        .bind(tenant)
        .bind(workspace)
        .bind(unit)
        .bind(revision)
        .bind(event)
        .bind(include_revision)
        .fetch_all(&mut **tx)
        .await
        .map_err(native_error)
}

/// One independent scalar-read scope. Repeated and mixed scopes retain ordinals.
#[derive(Clone, serde::Serialize)]
pub(crate) struct NativeReadRequest {
    pub unit_id: Uuid,
    pub revision: i64,
    pub event_id: Uuid,
    pub include_revision: bool,
}

/// Candidate adapter: only the guarded public SQL surface is callable here.
/// Every returned group passes the existing typed RDF decoder and equality check.
#[allow(dead_code)] // Wired into consumers only after private scalar/batch qualification.
pub(crate) async fn native_rows_batch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    requests: &[(&NativeReadRequest, &RdfDocument)],
) -> Result<Vec<Vec<serde_json::Value>>> {
    type BatchRow = (i64, Uuid, i64, Uuid, bool, Option<serde_json::Value>);
    let payload = serde_json::to_value(
        requests
            .iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>(),
    )
    .map_err(storage_error)?;
    let rows: Vec<BatchRow> = sqlx::query_as(
        "SELECT request_ordinal,unit_id,revision,event_id,include_revision,triple FROM public.tect_dk2_native_read_batch($1,$2,$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(payload)
    .fetch_all(&mut **tx)
    .await
    .map_err(native_error)?;
    let mut groups = vec![Vec::new(); requests.len()];
    let mut seen = vec![false; requests.len()];
    let mut empty = vec![false; requests.len()];
    for (ordinal, unit, revision, event, include_revision, row) in rows {
        let index = ordinal
            .checked_sub(1)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value < requests.len())
            .ok_or(Error::InternalInvariant)?;
        let request = requests[index].0;
        if unit != request.unit_id
            || revision != request.revision
            || event != request.event_id
            || include_revision != request.include_revision
            || empty[index]
        {
            return Err(Error::InternalInvariant);
        }
        match row {
            Some(row) => groups[index].push(row),
            None => {
                if seen[index] {
                    return Err(Error::InternalInvariant);
                }
                empty[index] = true;
            }
        }
        seen[index] = true;
    }
    for (index, (_, document)) in requests.iter().enumerate() {
        if !seen[index] {
            return Err(Error::InternalInvariant);
        }
        super::validate_rows(&groups[index], document)?;
    }
    Ok(groups)
}

pub(crate) async fn qualify_native(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let identity: (String, String, String) = sqlx::query_as(
        "SELECT pgrdf.version(),pgrdf.build_id(),(SELECT extversion FROM pg_catalog.pg_extension WHERE extname='pgrdf')",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if identity != ("0.6.34".into(), "v0.6.34".into(), "0.6.34".into()) {
        return Err(Error::InvalidConfiguration);
    }
    let shapes = graph(tx, "urn:tect:dk:shapes:dk-2").await?;
    reset_graph(tx, shapes, SHAPES).await?;
    let valid = graph(tx, "urn:tect:dk:activation:dk-2:valid").await?;
    let invalid = graph(tx, "urn:tect:dk:activation:dk-2:missing-mandatory").await?;
    reset_graph(tx, valid, QUALIFY_VALID).await?;
    let invalid_payload = QUALIFY_VALID.replace(
        "<urn:tect:dk:v2:qualification:runbook> a v2:RunbookSection ; v2:purposeAndFit \"purpose\" ; v2:targetEnvironments <urn:tect:dk:v2:qualification:runbook-targets> ; v2:requiredAuthority \"owner\" ; v2:failureAndRecovery \"stop\" ; v2:proofStatus v2:proof-status:documented ; v2:proofEvidenceRefs <urn:tect:dk:v2:qualification:runbook-proof> ; v2:steps <urn:tect:dk:v2:qualification:steps> .\n",
        "",
    );
    reset_graph(tx, invalid, &invalid_payload).await?;
    let good: serde_json::Value = sqlx::query_scalar("SELECT pgrdf.validate($1,$2,'native',true)")
        .bind(valid)
        .bind(shapes)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let bad: serde_json::Value = sqlx::query_scalar("SELECT pgrdf.validate($1,$2,'native',true)")
        .bind(invalid)
        .bind(shapes)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let typed_targets: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pgrdf.construct('CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <urn:tect:dk:activation:dk-2:valid> { ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:tect:dk:v2:KnowledgeRevision> ; ?p ?o } }')",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if good.get("conforms").and_then(serde_json::Value::as_bool) != Some(true)
        || bad.get("conforms").and_then(serde_json::Value::as_bool) != Some(false)
        || typed_targets == 0
    {
        return Err(Error::InvalidConfiguration);
    }
    for id in [valid, invalid] {
        sqlx::query("SELECT pgrdf.drop_graph($1,true)")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

async fn graph(tx: &mut Transaction<'_, Postgres>, iri: &str) -> Result<i64> {
    sqlx::query_scalar("SELECT pgrdf.add_graph($1)")
        .bind(iri)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}

async fn reset_graph(tx: &mut Transaction<'_, Postgres>, id: i64, payload: &str) -> Result<()> {
    sqlx::query("SELECT pgrdf.clear_graph($1)")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
        .bind(payload)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    Ok(())
}

fn operation_name(value: KnowledgeLifecycleOperation) -> &'static str {
    match value {
        KnowledgeLifecycleOperation::Create => "create",
        KnowledgeLifecycleOperation::Revise => "revise",
        KnowledgeLifecycleOperation::Revalidate => "revalidate",
        KnowledgeLifecycleOperation::Supersede => "supersede",
        KnowledgeLifecycleOperation::Retract => "retract",
        KnowledgeLifecycleOperation::Erase => "erase",
    }
}

fn native_error(error: sqlx::Error) -> Error {
    match error
        .as_database_error()
        .and_then(|value| value.code())
        .as_deref()
    {
        Some("23514") | Some("22023") => Error::InvalidArguments,
        Some("42501") => Error::KnowledgeUnavailable,
        _ => Error::StorageUnavailable,
    }
}
