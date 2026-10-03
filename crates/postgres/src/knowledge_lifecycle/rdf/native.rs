use super::RdfDocument;
use crate::knowledge_lifecycle::{CandidateBudget, CandidateProofError};
type BatchRow = (i64, Uuid, i64, Uuid, bool, Option<serde_json::Value>);
type CandidateBatchRow = (
    i64,
    Uuid,
    i64,
    Uuid,
    bool,
    Option<serde_json::Value>,
    i64,
    bool,
);
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_application::request_diagnostics::{count, measure};
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
    let payload = serde_json::to_value(
        requests
            .iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>(),
    )
    .map_err(storage_error)?;
    count("proof.native_batch_calls", 1);
    count("proof.native_batch_requested_keys", requests.len());
    let rows: Vec<BatchRow> = measure("pg.native_batch_sql", sqlx::query_as(
        "SELECT request_ordinal,unit_id,revision,event_id,include_revision,triple FROM public.tect_dk2_native_read_batch($1,$2,$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(payload)
    .fetch_all(&mut **tx))
    .await
    .map_err(native_error)?;
    count(
        "proof.native_batch_returned_rows_including_sentinels",
        rows.len(),
    );
    decode_batch_rows(requests, rows)
}

fn decode_batch_rows(
    requests: &[(&NativeReadRequest, &RdfDocument)],
    rows: Vec<BatchRow>,
) -> Result<Vec<Vec<serde_json::Value>>> {
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
    if tect_application::request_diagnostics::enabled() {
        count(
            "proof.native_batch_validated_triples",
            groups.iter().map(Vec::len).sum(),
        );
    }
    Ok(groups)
}

/// Candidate only: bound transport rows and bytes without changing native SQL semantics.
pub(crate) async fn native_rows_batch_candidate(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    requests: &[(&NativeReadRequest, &RdfDocument)],
    budget: &mut CandidateBudget,
) -> std::result::Result<Vec<Vec<serde_json::Value>>, CandidateProofError> {
    let payload = serde_json::to_value(requests.iter().map(|(r, _)| r).collect::<Vec<_>>())
        .map_err(storage_error)
        .map_err(CandidateProofError::from)?;
    // LIMIT allows one overflow row. Byte overflow returns no caller RDF values.
    count("proof.native_batch_calls", 1);
    count("proof.native_batch_requested_keys", requests.len());
    let rows: Vec<CandidateBatchRow> = measure("pg.native_batch_sql", sqlx::query_as(
        "WITH bounded AS MATERIALIZED (SELECT request_ordinal,unit_id,revision,event_id,include_revision,triple FROM public.tect_dk2_native_read_batch($1,$2,$3) LIMIT $4 + 1), sized AS (SELECT *,COALESCE(sum(pg_catalog.octet_length(triple::text)) OVER(),0)::bigint AS bytes,count(*) OVER() AS rows FROM bounded) SELECT request_ordinal,unit_id,revision,event_id,include_revision,CASE WHEN bytes<=$5 AND rows<=$4 THEN triple ELSE NULL END,bytes,(bytes<=$5 AND rows<=$4) FROM sized"
    ).bind(tenant).bind(workspace).bind(payload).bind(budget.remaining_rows() as i64)
        .bind(budget.remaining_bytes() as i64).fetch_all(&mut **tx)).await.map_err(candidate_native_error)?;
    count(
        "proof.native_batch_returned_rows_including_sentinels",
        rows.len(),
    );
    if rows.iter().any(|row| !row.7) {
        if rows.len() > budget.remaining_rows() {
            count("proof.candidate_cap_rows", 1);
        }
        if rows
            .first()
            .is_some_and(|row| row.6 > budget.remaining_bytes() as i64)
        {
            count("proof.candidate_cap_bytes", 1);
        }
        return Err(CandidateProofError::refusal(Error::InternalInvariant));
    }
    let bytes = rows.first().map_or(0, |row| row.6);
    budget.reserve_bytes(
        usize::try_from(bytes)
            .map_err(|_| CandidateProofError::refusal(Error::InternalInvariant))?,
    )?;
    budget.returned_rows += rows.len();

    let rows = rows
        .into_iter()
        .map(|(a, b, c, d, e, f, _, _)| (a, b, c, d, e, f))
        .collect();
    decode_batch_rows(requests, rows).map_err(CandidateProofError::refusal)
}

fn candidate_native_error(error: sqlx::Error) -> CandidateProofError {
    let known = error.as_database_error().is_some_and(|database| {
        candidate_native_refusal(database.code().as_deref(), database.message())
    });
    let public = native_error(error);
    if known {
        CandidateProofError::Refusal(public)
    } else {
        CandidateProofError::Terminal(public)
    }
}
fn candidate_native_refusal(code: Option<&str>, message: &str) -> bool {
    // This exact server constant originates in migration0041. Unknown storage,
    // engine/recovery failures and57014 cancellation never authorize a retry.
    code == Some("55000") && message == "invalid durable knowledge native batch term"
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
mod candidate_tests {
    use super::*;
    #[test]
    fn only_verified_native_term_refusal_can_fall_back() {
        assert!(candidate_native_refusal(
            Some("55000"),
            "invalid durable knowledge native batch term"
        ));
        for code in [
            Some("57014"),
            Some("22023"),
            Some("23514"),
            Some("42501"),
            None,
        ] {
            assert!(!candidate_native_refusal(
                code,
                "invalid durable knowledge native batch term"
            ));
        }
        assert!(!candidate_native_refusal(
            Some("55000"),
            "unsupported durable knowledge native batch engine"
        ));
        assert!(!candidate_native_refusal(
            Some("55000"),
            "durable knowledge recovery required"
        ));
    }
}

#[cfg(test)]
mod batch_decoder_tests {
    use super::super::model::{Builder, RdfRefs};
    use super::*;
    fn document(nonempty: bool) -> RdfDocument {
        let mut b = Builder::new(RdfRefs {
            unit: "urn:u".into(),
            revision: "urn:r".into(),
            event: "urn:e".into(),
            event_content: "urn:c".into(),
        });
        if nonempty {
            b.iri("urn:s", "urn:p", "urn:o").unwrap();
        }
        b.finish(false).unwrap()
    }
    fn request() -> NativeReadRequest {
        NativeReadRequest {
            unit_id: Uuid::new_v4(),
            revision: 1,
            event_id: Uuid::new_v4(),
            include_revision: true,
        }
    }
    fn row(ordinal: i64, r: &NativeReadRequest, triple: Option<serde_json::Value>) -> BatchRow {
        (
            ordinal,
            r.unit_id,
            r.revision,
            r.event_id,
            r.include_revision,
            triple,
        )
    }
    #[test]
    fn ordinals_preserve_requested_groups_with_valid_empty_sentinels() {
        let a = request();
        let b = request();
        let d = document(false);
        let groups = decode_batch_rows(
            &[(&a, &d), (&b, &d)],
            vec![row(2, &b, None), row(1, &a, None)],
        )
        .unwrap();
        assert_eq!(groups, vec![Vec::<serde_json::Value>::new(); 2]);
        for rows in [
            vec![row(1, &a, None)],
            vec![row(3, &a, None)],
            vec![row(1, &a, None), row(1, &a, None)],
            vec![row(1, &b, None), row(2, &b, None)],
        ] {
            assert_eq!(
                decode_batch_rows(&[(&a, &d), (&b, &d)], rows),
                Err(Error::InternalInvariant)
            );
        }
    }
    #[test]
    fn exact_triple_groups_reject_duplicates_mixed_sentinel_and_missing_content() {
        let r = request();
        let d = document(true);
        let triple = serde_json::json!({"subject":{"type":"iri","value":"urn:s"},"predicate":{"type":"iri","value":"urn:p"},"object":{"type":"iri","value":"urn:o"}});
        assert_eq!(
            decode_batch_rows(&[(&r, &d)], vec![row(1, &r, Some(triple.clone()))]).unwrap(),
            vec![vec![triple.clone()]]
        );
        for rows in [
            vec![row(1, &r, None)],
            vec![row(1, &r, None), row(1, &r, Some(triple.clone()))],
            vec![
                row(1, &r, Some(triple.clone())),
                row(1, &r, Some(triple.clone())),
            ],
        ] {
            assert_eq!(
                decode_batch_rows(&[(&r, &d)], rows),
                Err(Error::InternalInvariant)
            );
        }
    }
}
