use super::*;

#[derive(sqlx::FromRow)]
struct Dispatch {
    id: Uuid,
    attempt_number: i32,
    predecessor_dispatch_id: Option<Uuid>,
    retry_basis: String,
    payload_digest: String,
    request_payload: Vec<u8>,
    response_payload: Option<Vec<u8>>,
    state: String,
    send_certainty: String,
    outcome: Option<String>,
    raw_response_ref: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Raw {
    response_payload: Option<Vec<u8>>,
    response_sha256: Option<String>,
    response_complete: bool,
    original_transport_outcome: String,
    http_status: Option<i32>,
    original_transport_context: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct Consumption {
    calls: i64,
    retry_dispatches: i64,
    response_sha256: Option<String>,
    raw_response_ref: Option<String>,
    send_certainty: String,
    outcome: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    unknown_usage: bool,
}

pub(super) async fn assert_one_sealed_attempt(
    pool: &PgPool,
    workspace: Uuid,
    opportunity: Uuid,
    request_sha256: &str,
    reviewed: &[u8],
) {
    let rows: Vec<Dispatch> = sqlx::query_as(
        "SELECT id,attempt_number,predecessor_dispatch_id,retry_basis,payload_digest,request_payload, \
         response_payload,state,send_certainty,outcome,raw_response_ref FROM advisory_dispatch \
         WHERE workspace_id=$1 ORDER BY authorized_at",
    )
    .bind(workspace)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "one-use workspace has exactly one dispatch");
    let row = &rows[0];
    let linked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2 AND id=$3)",
    )
    .bind(workspace)
    .bind(opportunity)
    .bind(row.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(linked);
    assert_eq!(row.attempt_number, 1);
    assert!(row.predecessor_dispatch_id.is_none());
    assert_eq!(row.retry_basis, "initial");
    assert_eq!(row.payload_digest, request_sha256);
    assert_eq!(row.request_payload, reviewed);
    assert_eq!(row.state, "sealed", "raw transport must be durably sealed");
    let reservation: (i64, i64, String, i64) = sqlx::query_as(
        "SELECT reserved_calls,reserved_retry_dispatches,request_sha256,request_utf8_bytes \
         FROM advisory_budget_reservations WHERE workspace_id=$1 AND dispatch_id=$2",
    )
    .bind(workspace)
    .bind(row.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((reservation.0, reservation.1), (1, 0));
    assert_eq!(reservation.2, request_sha256);
    assert_eq!(reservation.3, reviewed.len() as i64);
    let raw: Raw = sqlx::query_as(
        "SELECT response_payload,response_sha256,response_complete,original_transport_outcome, \
         http_status,original_transport_context FROM advisory_provider_observations \
         WHERE workspace_id=$1 AND dispatch_id=$2",
    )
    .bind(workspace)
    .bind(row.id)
    .fetch_one(pool)
    .await
    .unwrap();
    let used: Consumption = sqlx::query_as(
        "SELECT calls,retry_dispatches,response_sha256,raw_response_ref,send_certainty,outcome, \
         input_tokens,output_tokens,unknown_usage FROM advisory_budget_consumptions \
         WHERE workspace_id=$1 AND dispatch_id=$2",
    )
    .bind(workspace)
    .bind(row.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((used.calls, used.retry_dispatches), (1, 0));
    assert_eq!(raw.response_payload, row.response_payload);
    let response_sha256 = row
        .response_payload
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    assert_eq!(raw.response_sha256, response_sha256);
    assert_eq!(used.response_sha256, response_sha256);
    assert_eq!(used.raw_response_ref, row.raw_response_ref);
    assert_eq!(used.send_certainty, row.send_certainty);
    assert_eq!(Some(used.outcome.as_str()), row.outcome.as_deref());
    if used.input_tokens.is_none() || used.output_tokens.is_none() {
        assert!(used.unknown_usage);
    }
    let transport = raw
        .original_transport_context
        .as_ref()
        .expect("original transport context absent");
    assert_eq!(transport["send_certainty"], row.send_certainty);
    assert_eq!(transport["outcome"], row.outcome.as_deref().unwrap());
    assert_eq!(
        transport["raw_response_ref"].as_str(),
        row.raw_response_ref.as_deref()
    );
    match &row.response_payload {
        Some(bytes) => {
            assert!(!bytes.is_empty() && bytes.len() <= 65_536);
            assert_eq!(row.send_certainty, "sent");
            assert!(matches!(
                row.outcome.as_deref(),
                Some("provider_response" | "provider_failure")
            ));
            assert!(raw.http_status.is_some());
            assert_eq!(
                raw.original_transport_outcome,
                if raw.response_complete {
                    "received"
                } else {
                    "partial_received"
                }
            );
            assert!(
                row.raw_response_ref
                    .as_deref()
                    .is_some_and(|s| s.contains(&response_sha256.unwrap()))
            );
        }
        None => {
            assert_eq!(row.send_certainty, "sent_unknown");
            assert_eq!(row.outcome.as_deref(), Some("provider_failure"));
            assert_eq!(raw.original_transport_outcome, "transport_failure");
            assert!(!raw.response_complete && raw.http_status.is_none());
            assert!(
                row.raw_response_ref
                    .as_deref()
                    .is_some_and(|s| s.ends_with("transport-unknown"))
            );
        }
    }
    println!(
        "S01 durable dispatch={} state={} certainty={} outcome={:?} response_bytes={} response_sha256={:?} raw_ref={:?} tokens={:?}/{:?}",
        row.id,
        row.state,
        row.send_certainty,
        row.outcome,
        row.response_payload.as_ref().map_or(0, Vec::len),
        raw.response_sha256,
        row.raw_response_ref,
        used.input_tokens,
        used.output_tokens
    );
}
