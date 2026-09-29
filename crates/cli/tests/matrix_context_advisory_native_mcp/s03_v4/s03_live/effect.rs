//! Continue a durably ranked real-provider attempt in its original owner session.
//! The synthetic operating facts remain test data, never approved product facts.
use super::*;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn verify_ranked_caller_effect(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    socket: &Path,
    verifier_host: &Path,
    workspace_key: &str,
    ready: &Value,
    work: &Value,
    prepared: &Value,
    manifest: &PipelineRecommendationManifest,
    ranked: &Value,
    matrix_effect_id: Uuid,
) {
    assert_eq!(ranked["status"], "ranked");
    manifest.validate_digest().unwrap();
    let selected_id = ranked["ranked_ids"][0].as_str().unwrap();
    let option = manifest
        .options
        .iter()
        .find(|option| option.id == selected_id)
        .expect("ranked ID must be one of the frozen eligible options");
    option.verification_plan.validate().unwrap();
    assert!(
        !option.verification_plan.obligations.is_empty(),
        "selected plan must retain mandatory phases"
    );
    let opportunity = id(&prepared["opportunity_id"]);
    let audit: (String, String, i64) = sqlx::query_as(
        "SELECT state,primary_reason,\
         (SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2) \
         FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(audit, ("advised".into(), "provider_response".into(), 1));
    let before: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM native_slices WHERE workspace_id=$1),\
         (SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$1)",
    )
    .bind(workspace)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before, (0, 0), "ranked advice must have no caller effect");

    let disposition = route(
        owner,
        "command",
        "pipeline.recommendation.disposition",
        json!({
            "request_id":Uuid::new_v4(),
            "opportunity_id":prepared["opportunity_id"],
            "expected_work_revision":work["revision"],
            "manifest_digest":prepared["manifest_digest"],
            "action":"accept_recommendation",
            "rationale":"Fixture caller accepts the durably ranked JEV option."
        }),
    )
    .await;
    assert_eq!(disposition["selected_option_id"], selected_id);
    assert_eq!(disposition["selected_kind"], option.kind.as_str());
    let after_disposition: i64 =
        sqlx::query_scalar("SELECT count(*) FROM native_slices WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(after_disposition, 0, "disposition cannot open a Slice");

    let set = ready["candidate_set"]["id"].clone();
    let open_request = json!({
        "request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
        "scope_revision":ready["scope"]["revision"],
        "candidate_set_id":set,"candidate_set_revision":ready["candidate_set"]["revision"],
        "candidate_snapshot_id":ready["snapshot"]["id"],
        "candidate_id":work["id"],"candidate_revision":work["revision"],
        "disposition_id":disposition["id"]
    });
    let opened = route(owner, "command", "slice.open", open_request.clone()).await;
    let slice = &opened["created"];
    assert_eq!(slice["pipeline"], option.kind.as_str());
    assert_eq!(slice["selected_option_id"], selected_id);
    assert_eq!(slice["verification_plan_id"], option.verification_plan.id);
    assert_eq!(
        slice["verification_plan_digest"],
        option.verification_plan.digest
    );
    assert_eq!(
        slice["verification_plan_source_definition_digest"],
        option.verification_plan.source_definition_digest
    );
    let slice_id = id(&slice["id"]);
    let begun = route(
        owner,
        "command",
        "slice.pipeline.begin",
        json!({
            "request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
            "slice_id":slice_id,"slice_revision":slice["revision"],
            "definition_version":option.verification_plan.source_definition_version,
            "qualification_reason":"Fixture caller explicitly begins the selected pinned pipeline."
        }),
    )
    .await;
    let run = &begun["created"]["run"];
    assert_eq!(run["selected_option_id"], selected_id);
    assert_eq!(run["verification_plan_id"], option.verification_plan.id);
    assert_eq!(
        run["verification_plan_digest"],
        option.verification_plan.digest
    );
    let stored: (String, String, String) = sqlx::query_as(
        "SELECT selected_option_id,verification_plan_id,verification_plan_digest \
         FROM slice_pipeline_runs WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(id(&run["id"]))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(stored.0, selected_id);
    assert_eq!(stored.1, option.verification_plan.id);
    assert_eq!(stored.2, option.verification_plan.digest);

    let mut verifier = Mcp::start(
        socket,
        verifier_host,
        &Uuid::new_v4().to_string(),
        workspace_key,
    )
    .await;
    verifier.call("open_workspace", json!({})).await;
    let effect = route(
        &mut verifier,
        "query",
        "pipeline.open_effect.get",
        json!({"slice_id":slice_id,"open_request_id":open_request["request_id"]}),
    )
    .await;
    assert_eq!(effect["material"]["slice"], *slice);
    assert_eq!(effect["material"]["disposition"]["id"], disposition["id"]);
    assert_eq!(effect["material"]["manifest_digest"], manifest.digest);
    assert_eq!(
        effect["material"]["matrix_effect_attestation_id"],
        matrix_effect_id.to_string()
    );
    assert_eq!(
        effect["material"]["slice"]["verification_plan_digest"],
        option.verification_plan.digest
    );
    assert_ne!(
        effect["verifier_principal_id"],
        effect["material"]["caller_principal_id"]
    );
    assert_eq!(
        route_error(
            owner,
            "query",
            "pipeline.open_effect.get",
            json!({"slice_id":slice_id,"open_request_id":open_request["request_id"]}),
        )
        .await["error"]["code"],
        "forbidden"
    );
    let saved_plan: Value = sqlx::query_scalar(
        "SELECT option->'verification_plan' FROM pipeline_advice_contexts c,\
         jsonb_array_elements(c.manifest_payload->'options') option \
         WHERE c.workspace_id=$1 AND c.opportunity_id=$2 AND option->>'id'=$3",
    )
    .bind(workspace)
    .bind(id(&prepared["opportunity_id"]))
    .bind(selected_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        saved_plan,
        serde_json::to_value(&option.verification_plan).unwrap()
    );
    assert_eq!(
        saved_plan["obligations"].as_array().unwrap().len(),
        option.verification_plan.obligations.len()
    );
    let attested = route(
        &mut verifier,
        "command",
        "pipeline.open_effect.verify",
        json!({
            "request_id":Uuid::new_v4(),"slice_id":slice_id,
            "open_request_id":open_request["request_id"],
            "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
            "summary":"Independent fixture Verifier read the exact selected plan and opening effect."
        }),
    )
    .await;
    assert_eq!(attested["verdict"], "matches");
    assert_eq!(attested["effect_digest"], effect["effect_digest"]);
    let attestation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipeline_open_effect_attestations \
         WHERE workspace_id=$1 AND slice_id=$2",
    )
    .bind(workspace)
    .bind(slice_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(attestation_count, 1);
    println!(
        "live caller effect opportunity={} selected_option={} slice={} run={} plan_digest={} mandatory_phases={} verifier_attestation={}",
        prepared["opportunity_id"],
        selected_id,
        slice_id,
        run["id"],
        option.verification_plan.digest,
        option.verification_plan.obligations.len(),
        attested["request_id"]
    );
    verifier.finish().await;
}
