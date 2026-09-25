use super::*;
use std::path::Path;

pub(super) async fn exercise(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    verifier: &mut Mcp,
    open: &Value,
    opened: &Value,
    disposition: &Value,
    matrix_disposition: &Value,
    matrix_match: &Value,
    work: &Value,
    selected_pipeline: &Value,
    root: &Path,
    socket: &Path,
) {
    let slice_id = Uuid::parse_str(opened["created"]["id"].as_str().unwrap()).unwrap();
    let open_request_id = Uuid::parse_str(open["request_id"].as_str().unwrap()).unwrap();
    let effect = route(
        verifier,
        "query",
        "pipeline.open_effect.get",
        json!({"slice_id":slice_id,"open_request_id":open_request_id}),
    )
    .await;
    assert_eq!(effect["material"]["slice"], opened["created"]);
    assert_eq!(effect["material"]["open_request"], open.clone());
    assert_eq!(effect["material"]["slice"]["pipeline"], *selected_pipeline);
    assert_eq!(effect["material"]["disposition"]["id"], disposition["id"]);
    assert_eq!(effect["material"]["disposition"]["work_id"], work["id"]);
    assert_eq!(
        effect["material"]["disposition"]["selected_kind"],
        *selected_pipeline
    );
    assert_eq!(effect["material"]["work"]["id"], work["id"]);
    assert_eq!(effect["material"]["work"]["revision"], work["revision"]);
    assert_eq!(
        effect["material"]["matrix_disposition_id"],
        matrix_disposition["disposition_id"]
    );
    assert_ne!(
        effect["verifier_principal_id"],
        effect["material"]["caller_principal_id"]
    );
    assert_eq!(
        effect["material"]["caller_principal_id"],
        effect["material"]["matrix_owner_principal_id"]
    );

    let stored_open: (Value, Value) = sqlx::query_as(
        "SELECT origin_payload,origin_result FROM native_slices WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(slice_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(effect["material"]["open_request"], stored_open.0);
    assert_eq!(effect["material"]["open_receipt"], stored_open.1);
    let stored_disposition: Value = sqlx::query_scalar(
        "SELECT result_payload FROM pipeline_advice_dispositions WHERE workspace_id=$1 AND disposition_id=$2",
    )
    .bind(workspace)
    .bind(Uuid::parse_str(disposition["id"].as_str().unwrap()).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(effect["material"]["disposition"], stored_disposition);
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2",
    )
    .bind(workspace)
    .bind(Uuid::parse_str(matrix_match["request_id"].as_str().unwrap()).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        effect["material"]["matrix_effect_attestation_id"],
        json!(matrix_effect_id)
    );

    let owner_get = route_error(
        owner,
        "query",
        "pipeline.open_effect.get",
        json!({"slice_id":slice_id,"open_request_id":open_request_id}),
    )
    .await;
    assert_eq!(owner_get["error"]["code"], "forbidden");
    let stale_request_id = Uuid::new_v4();
    let stale_get = route_error(
        verifier,
        "query",
        "pipeline.open_effect.get",
        json!({"slice_id":slice_id,"open_request_id":stale_request_id}),
    )
    .await;
    assert_eq!(stale_get["error"]["code"], "not_found");

    let request_id = Uuid::new_v4();
    let verify = json!({
        "request_id":request_id,"slice_id":slice_id,"open_request_id":open_request_id,
        "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
        "summary":"Synthetic verifier confirms the exact opened pipeline effect."
    });
    let mut wrong_digest = verify.clone();
    wrong_digest["request_id"] = json!(Uuid::new_v4());
    wrong_digest["expected_effect_digest"] = json!(format!(
        "{:x}",
        Sha256::digest(b"wrong pipeline effect digest")
    ));
    let rejected_digest = route_error(
        verifier,
        "command",
        "pipeline.open_effect.verify",
        wrong_digest,
    )
    .await;
    assert_eq!(rejected_digest["error"]["code"], "input_conflict");
    let stale_verify = route_error(
        verifier,
        "command",
        "pipeline.open_effect.verify",
        json!({
            "request_id":Uuid::new_v4(),"slice_id":slice_id,
            "open_request_id":stale_request_id,"expected_effect_digest":effect["effect_digest"],
            "verdict":"matches","summary":"Stale open receipt must not verify."
        }),
    )
    .await;
    assert_eq!(stale_verify["error"]["code"], "not_found");
    let owner_verify = route_error(
        owner,
        "command",
        "pipeline.open_effect.verify",
        verify.clone(),
    )
    .await;
    assert_eq!(owner_verify["error"]["code"], "forbidden");

    // Turn the fixture caller into a verifier only while probing the explicit
    // self-verification guard, then restore its owner role before assertions.
    let caller_id =
        Uuid::parse_str(effect["material"]["caller_principal_id"].as_str().unwrap()).unwrap();
    let promoted = sqlx::query(
        "UPDATE principals SET role='verifier' WHERE tenant_id=(SELECT tenant_id FROM workspaces WHERE id=$1) AND id=$2 AND role='owner'",
    )
    .bind(workspace)
    .bind(caller_id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(promoted.rows_affected(), 1);
    let self_get_response = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "query",
                json!({"route":"pipeline.open_effect.get","params":{
                    "slice_id":slice_id,"open_request_id":open_request_id
                }}),
            ),
        )
        .await;
    let self_verify_response = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "command",
                json!({
                    "route":"pipeline.open_effect.verify","params":verify
                }),
            ),
        )
        .await;
    let restored = sqlx::query(
        "UPDATE principals SET role='owner' WHERE tenant_id=(SELECT tenant_id FROM workspaces WHERE id=$1) AND id=$2 AND role='verifier'",
    )
    .bind(workspace)
    .bind(caller_id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(restored.rows_affected(), 1);
    let self_get = recovery_support::tool_payload(&self_get_response);
    let self_verify = recovery_support::tool_payload(&self_verify_response);
    assert_eq!(self_get["error"]["code"], "forbidden");
    assert_eq!(self_verify["error"]["code"], "forbidden");

    let attestation = route(
        verifier,
        "command",
        "pipeline.open_effect.verify",
        json!({
            "request_id":request_id,"slice_id":slice_id,"open_request_id":open_request_id,
            "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
            "summary":"Synthetic verifier confirms the exact opened pipeline effect."
        }),
    )
    .await;
    assert_eq!(attestation["request_id"], json!(request_id));
    assert_eq!(attestation["slice_id"], json!(slice_id));
    assert_eq!(attestation["open_request_id"], json!(open_request_id));
    assert_eq!(attestation["effect_digest"], effect["effect_digest"]);
    assert_eq!(attestation["verdict"], "matches");
    assert_eq!(
        attestation["verifier_principal_id"],
        effect["verifier_principal_id"]
    );
    assert_eq!(
        attestation["verifier_session_id"],
        effect["verifier_session_id"]
    );
    let persisted: (Uuid, Uuid, String, String) = sqlx::query_as(
        "SELECT slice_id,open_request_id,effect_digest,verdict FROM pipeline_open_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2",
    )
    .bind(workspace)
    .bind(request_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            slice_id,
            open_request_id,
            effect["effect_digest"].as_str().unwrap().to_owned(),
            "match".into()
        )
    );
    let attestations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipeline_open_effect_attestations WHERE workspace_id=$1 AND slice_id=$2",
    )
    .bind(workspace)
    .bind(slice_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(attestations, 1);
    let runs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$1 AND slice_id=$2",
    )
    .bind(workspace)
    .bind(slice_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        runs, 0,
        "effect verification cannot complete a pipeline phase"
    );

    let foreign_owner_enrollment = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let foreign_owner_config = root.join("foreign-owner.json");
    host_file(&foreign_owner_config, &foreign_owner_enrollment.auth);
    let foreign_workspace_key = format!("foreign-{}", Uuid::new_v4());
    let mut foreign_owner = Mcp::start(
        socket,
        &foreign_owner_config,
        &Uuid::new_v4().to_string(),
        &foreign_workspace_key,
    )
    .await;
    let foreign_opened = foreign_owner.call("open_workspace", json!({})).await;
    let foreign_workspace =
        Uuid::parse_str(foreign_opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let foreign_verifier_enrollment = admin::prepare_verifier_enrollment(
        pool,
        foreign_owner_enrollment.tenant_id,
        foreign_workspace,
    )
    .await
    .unwrap()
    .try_commit()
    .await
    .unwrap();
    let foreign_verifier_config = root.join("foreign-verifier.json");
    host_file(&foreign_verifier_config, &foreign_verifier_enrollment.auth);
    let mut foreign_verifier = Mcp::start(
        socket,
        &foreign_verifier_config,
        &Uuid::new_v4().to_string(),
        &foreign_workspace_key,
    )
    .await;
    foreign_verifier.call("open_workspace", json!({})).await;
    let foreign_get = route_error(
        &mut foreign_verifier,
        "query",
        "pipeline.open_effect.get",
        json!({"slice_id":slice_id,"open_request_id":open_request_id}),
    )
    .await;
    assert_eq!(foreign_get["error"]["code"], "not_found");
    let foreign_verify = route_error(
        &mut foreign_verifier,
        "command",
        "pipeline.open_effect.verify",
        json!({
            "request_id":Uuid::new_v4(),"slice_id":slice_id,
            "open_request_id":open_request_id,"expected_effect_digest":effect["effect_digest"],
            "verdict":"matches","summary":"Foreign tenant cannot inspect this effect."
        }),
    )
    .await;
    assert_eq!(foreign_verify["error"]["code"], "not_found");
    foreign_verifier.finish().await;
    foreign_owner.finish().await;
}
