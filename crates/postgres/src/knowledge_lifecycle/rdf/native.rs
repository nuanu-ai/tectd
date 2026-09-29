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

const CANONICAL_BATCH_LIMIT: usize = 512;

fn group_canonical_rows(
    count: usize,
    rows: Vec<(i64, serde_json::Value)>,
) -> Result<Vec<Vec<serde_json::Value>>> {
    let mut grouped = vec![Vec::new(); count];
    for (ordinal, triple) in rows {
        let index = usize::try_from(ordinal)
            .ok()
            .and_then(|value| value.checked_sub(1))
            .filter(|index| *index < count)
            .ok_or(Error::InternalInvariant)?;
        grouped[index].push(triple);
    }
    Ok(grouped)
}

pub(crate) async fn canonical_native_rows(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    keys: &[(Uuid, i64, Uuid)],
) -> Result<Vec<Vec<serde_json::Value>>> {
    let mut grouped = Vec::with_capacity(keys.len());
    for chunk in keys.chunks(CANONICAL_BATCH_LIMIT) {
        let units: Vec<Uuid> = chunk.iter().map(|key| key.0).collect();
        let revisions: Vec<i64> = chunk.iter().map(|key| key.1).collect();
        let events: Vec<Uuid> = chunk.iter().map(|key| key.2).collect();
        let include_revisions = vec![true; chunk.len()];
        let rows: Vec<(i64, serde_json::Value)> = sqlx::query_as(
            "SELECT input_ordinal,triple FROM public.tect_dk2_canonical_native_rows($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(principal)
        .bind(units)
        .bind(revisions)
        .bind(events)
        .bind(include_revisions)
        .fetch_all(&mut **tx)
        .await
        .map_err(native_error)?;
        grouped.extend(group_canonical_rows(chunk.len(), rows)?);
    }
    Ok(grouped)
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

#[cfg(test)]
mod canonical_tests {
    use super::*;

    #[test]
    fn groups_by_one_based_ordinal_without_deduplicating_rows() {
        let triple = serde_json::json!({"subject": "same"});
        let grouped = group_canonical_rows(
            3,
            vec![
                (2, triple.clone()),
                (1, triple.clone()),
                (2, triple.clone()),
            ],
        )
        .unwrap();
        assert_eq!(
            grouped,
            vec![vec![triple.clone()], vec![triple.clone(), triple], vec![]]
        );
    }

    #[test]
    fn rejects_invalid_ordinals() {
        for ordinal in [0, -1, 3, i64::MAX] {
            assert!(matches!(
                group_canonical_rows(2, vec![(ordinal, serde_json::Value::Null)]),
                Err(Error::InternalInvariant)
            ));
        }
    }

    #[test]
    fn leaves_malformed_triples_for_strict_validation() {
        assert_eq!(
            group_canonical_rows(1, vec![(1, serde_json::Value::Null)]).unwrap(),
            vec![vec![serde_json::Value::Null]]
        );
    }
}
