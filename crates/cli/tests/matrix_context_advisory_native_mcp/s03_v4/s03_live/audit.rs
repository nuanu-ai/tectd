use super::*;
use sqlx::FromRow;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[derive(FromRow)]
struct Dispatch {
    id: Uuid,
    attempt_number: i32,
    predecessor_dispatch_id: Option<Uuid>,
    retry_basis: String,
    provider: String,
    model: String,
    material_digest: String,
    payload_digest: String,
    request_payload: Vec<u8>,
    response_payload: Option<Vec<u8>>,
    pipeline_response_sha256: Option<String>,
    raw_response_ref: Option<String>,
    state: String,
    send_certainty: String,
    outcome: Option<String>,
}

#[derive(FromRow)]
struct Observation {
    response_payload: Option<Vec<u8>>,
    response_sha256: Option<String>,
    response_complete: bool,
    original_transport_outcome: String,
    http_status: Option<i32>,
    original_transport_context: Option<Value>,
}

#[derive(FromRow)]
struct Consumption {
    calls: i64,
    retry_dispatches: i64,
    response_sha256: Option<String>,
    raw_response_ref: Option<String>,
    send_certainty: String,
    outcome: String,
    unknown_usage: bool,
    exhausted_after_response: bool,
}

struct OutcomeFacts<'a> {
    status: &'a str,
    opportunity_state: &'a str,
    reason: &'a str,
    dispatch_state: &'a str,
    certainty: &'a str,
    outcome: Option<&'a str>,
    response_nonempty: bool,
    observation_complete: bool,
    http_ok: bool,
    consumption_present: bool,
    unknown_usage: bool,
    exhausted: bool,
    interpretation_status: Option<&'a str>,
}

fn verdict(f: &OutcomeFacts<'_>) -> std::result::Result<bool, &'static str> {
    match f.status {
        "ranked" | "abstained" => {
            if f.opportunity_state != "advised"
                || f.reason != "provider_response"
                || f.dispatch_state != "sealed"
                || f.certainty != "sent"
                || f.outcome != Some("provider_response")
                || !f.response_nonempty
                || !f.observation_complete
                || !f.http_ok
                || !f.consumption_present
                || f.unknown_usage
                || f.exhausted
                || f.interpretation_status != Some(f.status)
            {
                return Err("ranked/abstained outcome lacks durable successful receipt");
            }
            Ok(f.status == "ranked")
        }
        "send_unknown" => {
            if f.certainty != "sent_unknown"
                || !matches!(f.dispatch_state, "sending" | "sealed")
                || !matches!(f.opportunity_state, "awaiting_response" | "unresolved")
                || !matches!(f.reason, "dispatch_authorized" | "send_unknown")
                || f.interpretation_status.is_some()
            {
                return Err("send_unknown outcome lacks uncertain-send receipt");
            }
            if f.dispatch_state == "sealed"
                && (f.outcome != Some("provider_failure") || !f.consumption_present)
            {
                return Err("sealed uncertain send lacks failure accounting");
            }
            Ok(false)
        }
        "budget_exhausted" => {
            if f.opportunity_state != "failed"
                || f.reason != "budget_exhausted_after_response"
                || f.dispatch_state != "sealed"
                || !f.consumption_present
                || !f.exhausted
                || f.interpretation_status.is_some()
            {
                return Err("budget-exhausted outcome lacks durable exhaustion evidence");
            }
            Ok(false)
        }
        "stale" => {
            if !matches!(f.opportunity_state, "failed" | "invalidated")
                || f.dispatch_state != "sealed"
                || !f.consumption_present
                || f.interpretation_status.is_some()
            {
                return Err("stale outcome lacks a terminal failed/invalidated receipt");
            }
            Ok(false)
        }
        _ => Err("unexpected post-send Pipeline status"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn readback(
    pool: &PgPool,
    workspace: Uuid,
    opportunity: Uuid,
    outcome: &Value,
    request_path: &Path,
    marker_path: &Path,
    digest: &str,
    manifest_digest: &str,
    profile: &str,
    call_id: &str,
) -> bool {
    let expected_marker = format!("call_id={call_id}\nrequest_sha256={digest}\n");
    assert_eq!(fs::read_to_string(marker_path).unwrap(), expected_marker);
    assert_eq!(
        fs::metadata(marker_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let reviewed = fs::read(request_path).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&reviewed)), digest);

    let rows: Vec<Dispatch> = sqlx::query_as(
        "SELECT id,attempt_number,predecessor_dispatch_id,retry_basis,provider,model,\
         material_digest,payload_digest,request_payload,response_payload,pipeline_response_sha256,\
         raw_response_ref,state,send_certainty,outcome FROM advisory_dispatch \
         WHERE workspace_id=$1 AND opportunity_id=$2 ORDER BY attempt_number",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "one Pipeline dispatch required after marking"
    );
    let row = &rows[0];
    assert_eq!(row.attempt_number, 1);
    assert!(row.predecessor_dispatch_id.is_none());
    assert_eq!(row.retry_basis, "initial");
    assert_eq!(row.provider, profile);
    assert_eq!(row.model, REAL_MODEL);
    assert_eq!(row.material_digest, manifest_digest);
    assert_eq!(row.payload_digest, digest);
    assert!(
        row.request_payload == reviewed,
        "saved request differs from reviewed bytes"
    );
    assert_eq!(outcome["opportunity_id"], opportunity.to_string());
    let status = outcome["status"].as_str().expect("Pipeline status missing");
    if let Some(reported_dispatch) = outcome["dispatch_id"].as_str() {
        assert_eq!(reported_dispatch, row.id.to_string());
    } else {
        // The public `stale` shape omits dispatch_id even after a sealed failure.
        assert_eq!(status, "stale");
    }

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
    assert_eq!(reservation.2, digest);
    assert_eq!(reservation.3, reviewed.len() as i64);

    let response_sha = row
        .response_payload
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    assert_eq!(row.pipeline_response_sha256, response_sha);
    let expected_ref = match &response_sha {
        Some(sha) => format!("jev:{profile}:{}:sha256:{sha}", row.id),
        None => format!("jev:{profile}:{}:transport-unknown", row.id),
    };
    let observation: Option<Observation> = sqlx::query_as(
        "SELECT response_payload,response_sha256,response_complete,original_transport_outcome,\
         http_status,original_transport_context \
         FROM advisory_provider_observations WHERE workspace_id=$1 AND dispatch_id=$2",
    )
    .bind(workspace)
    .bind(row.id)
    .fetch_optional(pool)
    .await
    .unwrap();
    let consumption: Option<Consumption> = sqlx::query_as(
        "SELECT calls,retry_dispatches,response_sha256,raw_response_ref,send_certainty,outcome,\
         unknown_usage,exhausted_after_response FROM advisory_budget_consumptions \
         WHERE workspace_id=$1 AND dispatch_id=$2",
    )
    .bind(workspace)
    .bind(row.id)
    .fetch_optional(pool)
    .await
    .unwrap();
    if row.state == "sealed" {
        assert_eq!(row.raw_response_ref.as_deref(), Some(expected_ref.as_str()));
        let raw = observation
            .as_ref()
            .expect("sealed dispatch lacks raw receipt");
        assert!(
            raw.response_payload == row.response_payload,
            "raw receipt bytes differ from dispatch"
        );
        assert_eq!(raw.response_sha256, response_sha);
        assert_eq!(
            raw.original_transport_outcome,
            match (&row.response_payload, raw.response_complete) {
                (None, _) => "transport_failure",
                (Some(_), true) => "received",
                (Some(_), false) => "partial_received",
            }
        );
        if row.response_payload.is_none() {
            assert_eq!(row.send_certainty, "sent_unknown");
            assert_eq!(row.outcome.as_deref(), Some("provider_failure"));
            assert!(!raw.response_complete && raw.http_status.is_none());
        } else {
            assert_eq!(row.send_certainty, "sent");
        }
        let transport = raw
            .original_transport_context
            .as_ref()
            .expect("transport context absent");
        assert_eq!(transport["send_certainty"], row.send_certainty);
        assert_eq!(transport["outcome"], row.outcome.as_deref().unwrap());
        assert_eq!(transport["raw_response_ref"], expected_ref);
        let used = consumption
            .as_ref()
            .expect("sealed dispatch lacks consumption");
        assert_eq!((used.calls, used.retry_dispatches), (1, 0));
        assert_eq!(used.response_sha256, response_sha);
        assert_eq!(used.raw_response_ref, row.raw_response_ref);
        assert_eq!(used.send_certainty, row.send_certainty);
        assert_eq!(Some(used.outcome.as_str()), row.outcome.as_deref());
    } else {
        assert_eq!(row.state, "sending");
        assert!(row.response_payload.is_none() && row.pipeline_response_sha256.is_none());
        assert!(row.raw_response_ref.is_none() && observation.is_none() && consumption.is_none());
    }
    let (opportunity_state, reason): (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    let interpretation: Option<(String, Value)> = sqlx::query_as(
        "SELECT response_sha256,ranking FROM pipeline_advice_interpretations \
         WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_optional(pool)
    .await
    .unwrap();
    if let Some((sha, _)) = &interpretation {
        assert_eq!(Some(sha), response_sha.as_ref());
    }
    let facts = OutcomeFacts {
        status,
        opportunity_state: &opportunity_state,
        reason: &reason,
        dispatch_state: &row.state,
        certainty: &row.send_certainty,
        outcome: row.outcome.as_deref(),
        response_nonempty: row
            .response_payload
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty()),
        observation_complete: observation
            .as_ref()
            .is_some_and(|raw| raw.response_complete),
        http_ok: observation
            .as_ref()
            .and_then(|raw| raw.http_status)
            .is_some_and(|code| (200..300).contains(&code)),
        consumption_present: consumption.is_some(),
        unknown_usage: consumption.as_ref().is_some_and(|used| used.unknown_usage),
        exhausted: consumption
            .as_ref()
            .is_some_and(|used| used.exhausted_after_response),
        interpretation_status: interpretation
            .as_ref()
            .and_then(|(_, ranking)| ranking["status"].as_str()),
    };
    let ranked = verdict(&facts).expect("reported outcome disagrees with durable audit");
    println!(
        "live audit call_id={} opportunity={} dispatch={} status={} dispatch_state={} send_certainty={} outcome={:?} response_bytes={} response_sha256={:?} raw_ref={:?} recommendation_success={} marker={}",
        send::CALL_ID,
        opportunity,
        row.id,
        status,
        row.state,
        row.send_certainty,
        row.outcome,
        row.response_payload.as_ref().map_or(0, Vec::len),
        response_sha,
        row.raw_response_ref,
        ranked,
        marker_path.display()
    );
    ranked
}

#[test]
fn audit_verdict_requires_real_ranked_receipt() {
    let mut facts = OutcomeFacts {
        status: "ranked",
        opportunity_state: "advised",
        reason: "provider_response",
        dispatch_state: "sealed",
        certainty: "sent",
        outcome: Some("provider_response"),
        response_nonempty: true,
        observation_complete: true,
        http_ok: true,
        consumption_present: true,
        unknown_usage: false,
        exhausted: false,
        interpretation_status: Some("ranked"),
    };
    assert!(verdict(&facts).unwrap());
    facts.interpretation_status = None;
    assert!(verdict(&facts).is_err());
    facts.interpretation_status = Some("abstained");
    facts.status = "abstained";
    assert!(!verdict(&facts).unwrap());
    facts.status = "send_unknown";
    facts.certainty = "sent_unknown";
    facts.opportunity_state = "unresolved";
    facts.reason = "send_unknown";
    facts.interpretation_status = None;
    facts.outcome = Some("provider_failure");
    assert!(!verdict(&facts).unwrap());
}
