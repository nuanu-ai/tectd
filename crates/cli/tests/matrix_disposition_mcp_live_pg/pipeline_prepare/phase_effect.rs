use super::*;
use std::path::Path;

pub(super) async fn exercise(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    verifier: &mut Mcp,
    opened: &Value,
    begun: &Value,
    root: &Path,
    socket: &Path,
) {
    let slice = &opened["created"];
    let run = &begun["run"];
    let run_id = Uuid::parse_str(run["id"].as_str().unwrap()).unwrap();
    let (verdict, outcome, transition) = crate::full_support::successful_route(begun);
    let request = crate::full_support::completion(begun, verdict, outcome, transition, None, None);
    let completed = route(
        owner,
        "command",
        "slice.pipeline.phase.complete",
        request.clone(),
    )
    .await;
    let context = &completed["context"];
    let attempt_id: Uuid = sqlx::query_scalar("SELECT id FROM slice_pipeline_phase_attempts WHERE workspace_id=$1 AND run_id=$2 AND request_id=$3")
        .bind(workspace).bind(run_id).bind(Uuid::parse_str(request["request_id"].as_str().unwrap()).unwrap())
        .fetch_one(pool).await.unwrap();
    let attempt = context["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["id"] == json!(attempt_id))
        .unwrap();
    assert_eq!(attempt["outcome"], "completed");
    let before_run = context["run"].clone();
    let before_attempts = context["attempts"].as_array().unwrap().len();

    let get = json!({"run_id":run_id,"attempt_id":attempt_id});
    let effect = route(verifier, "query", "pipeline.phase_effect.get", get.clone()).await;
    let material = &effect["material"];
    assert_eq!(material["workspace_id"], json!(workspace));
    assert_eq!(material["slice_id"], slice["id"]);
    assert_eq!(material["run_id"], json!(run_id));
    assert_eq!(material["attempt_id"], json!(attempt_id));
    assert_eq!(material["phase_id"], attempt["phase_id"]);
    assert_eq!(material["selected_option_id"], slice["selected_option_id"]);
    assert_eq!(
        material["verification_plan_id"],
        slice["verification_plan_id"]
    );
    assert_eq!(
        material["verification_plan_version"],
        slice["verification_plan_source_definition_version"]
    );
    assert_eq!(
        material["verification_plan_digest"],
        slice["verification_plan_digest"]
    );
    assert_eq!(material["obligation"]["phase_id"], attempt["phase_id"]);
    assert_eq!(material["output_id"], attempt["output_id"]);
    assert_eq!(material["output_digest"], attempt["output_digest"]);
    assert_eq!(material["output"]["body_digest"], attempt["output_digest"]);
    assert_eq!(
        material["caller_principal_id"],
        material["slice_opener_principal_id"]
    );
    assert_eq!(
        material["caller_principal_id"],
        material["matrix_owner_principal_id"]
    );
    assert_ne!(
        effect["verifier_principal_id"],
        material["caller_principal_id"]
    );
    assert_ne!(effect["verifier_session_id"], material["caller_session_id"]);

    let base = json!({"request_id":Uuid::new_v4(),"run_id":run_id,"attempt_id":attempt_id,
        "expected_effect_digest":effect["effect_digest"],"verdict":"pass",
        "observation":format!("I independently read saved output {} with body digest {} and checked its phase obligation {}.",
            material["output_id"].as_str().unwrap(), material["output_digest"].as_str().unwrap(),
            material["obligation_digest"].as_str().unwrap()),
        "observed_output_digest":material["output_digest"],
        "summary":"Independent synthetic inspection of saved phase output passes."});
    let owner_get = route_error(owner, "query", "pipeline.phase_effect.get", get.clone()).await;
    assert_eq!(owner_get["error"]["code"], "forbidden");
    let owner_verify = route_error(
        owner,
        "command",
        "pipeline.phase_effect.verify",
        base.clone(),
    )
    .await;
    assert_eq!(owner_verify["error"]["code"], "forbidden");
    let stale = json!({"run_id":run_id,"attempt_id":Uuid::new_v4()});
    assert_eq!(
        route_error(
            verifier,
            "query",
            "pipeline.phase_effect.get",
            stale.clone()
        )
        .await["error"]["code"],
        "not_found"
    );
    let mut stale_verify = base.clone();
    stale_verify["request_id"] = json!(Uuid::new_v4());
    stale_verify["attempt_id"] = stale["attempt_id"].clone();
    assert_eq!(
        route_error(
            verifier,
            "command",
            "pipeline.phase_effect.verify",
            stale_verify
        )
        .await["error"]["code"],
        "not_found"
    );
    let mut wrong_effect = base.clone();
    wrong_effect["request_id"] = json!(Uuid::new_v4());
    wrong_effect["expected_effect_digest"] = json!("a".repeat(64));
    assert_eq!(
        route_error(
            verifier,
            "command",
            "pipeline.phase_effect.verify",
            wrong_effect
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let mut wrong_output = base.clone();
    wrong_output["request_id"] = json!(Uuid::new_v4());
    wrong_output["observed_output_digest"] = json!("b".repeat(64));
    assert_eq!(
        route_error(
            verifier,
            "command",
            "pipeline.phase_effect.verify",
            wrong_output
        )
        .await["error"]["code"],
        "input_conflict"
    );

    // A citation to the caller's work is not an independent inspection. It
    // cannot carry a pass/fail attestation without an observed output digest.
    let mut citation_only = base.clone();
    citation_only["request_id"] = json!(Uuid::new_v4());
    citation_only["observation"] = json!("The caller supplied a citation to its own phase output.");
    citation_only
        .as_object_mut()
        .unwrap()
        .remove("observed_output_digest");
    assert_eq!(
        route_error(
            verifier,
            "command",
            "pipeline.phase_effect.verify",
            citation_only
        )
        .await["error"]["code"],
        "invalid_arguments"
    );

    let caller_id = Uuid::parse_str(material["caller_principal_id"].as_str().unwrap()).unwrap();
    assert_eq!(sqlx::query("UPDATE principals SET role='verifier' WHERE tenant_id=(SELECT tenant_id FROM workspaces WHERE id=$1) AND id=$2 AND role='owner'")
        .bind(workspace).bind(caller_id).execute(pool).await.unwrap().rows_affected(), 1);
    let self_get_response = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "query",
                json!({"route":"pipeline.phase_effect.get","params":get.clone()}),
            ),
        )
        .await;
    let self_verify_response = owner
        .exchange(
            "tools/call",
            recovery_support::public_call(
                "command",
                json!({"route":"pipeline.phase_effect.verify","params":base.clone()}),
            ),
        )
        .await;
    assert_eq!(sqlx::query("UPDATE principals SET role='owner' WHERE tenant_id=(SELECT tenant_id FROM workspaces WHERE id=$1) AND id=$2 AND role='verifier'")
        .bind(workspace).bind(caller_id).execute(pool).await.unwrap().rows_affected(), 1);
    assert_eq!(
        recovery_support::tool_payload(&self_get_response)["error"]["code"],
        "forbidden"
    );
    assert_eq!(
        recovery_support::tool_payload(&self_verify_response)["error"]["code"],
        "forbidden"
    );

    let foreign_enrollment = admin::enroll_host(pool, None, vec![]).await.unwrap();
    let foreign_owner_config = root.join("phase-foreign-owner.json");
    host_file(&foreign_owner_config, &foreign_enrollment.auth);
    let foreign_key = format!("phase-foreign-{}", Uuid::new_v4());
    let mut foreign_owner = Mcp::start(
        socket,
        &foreign_owner_config,
        &Uuid::new_v4().to_string(),
        &foreign_key,
    )
    .await;
    let foreign_workspace = foreign_owner.call("open_workspace", json!({})).await;
    let foreign_workspace_id =
        Uuid::parse_str(foreign_workspace["workspace"]["id"].as_str().unwrap()).unwrap();
    let foreign_verifier_enrollment = admin::prepare_verifier_enrollment(
        pool,
        foreign_enrollment.tenant_id,
        foreign_workspace_id,
    )
    .await
    .unwrap()
    .try_commit()
    .await
    .unwrap();
    let foreign_verifier_config = root.join("phase-foreign-verifier.json");
    host_file(&foreign_verifier_config, &foreign_verifier_enrollment.auth);
    let mut foreign_verifier = Mcp::start(
        socket,
        &foreign_verifier_config,
        &Uuid::new_v4().to_string(),
        &foreign_key,
    )
    .await;
    foreign_verifier.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut foreign_verifier,
            "query",
            "pipeline.phase_effect.get",
            get
        )
        .await["error"]["code"],
        "not_found"
    );
    assert_eq!(
        route_error(
            &mut foreign_verifier,
            "command",
            "pipeline.phase_effect.verify",
            base.clone()
        )
        .await["error"]["code"],
        "not_found"
    );
    foreign_verifier.finish().await;
    foreign_owner.finish().await;

    for verdict in ["unknown", "fail", "pass"] {
        let mut verify = base.clone();
        verify["request_id"] = json!(Uuid::new_v4());
        verify["verdict"] = json!(verdict);
        if verdict == "unknown" {
            verify.as_object_mut().unwrap().remove("observation");
            verify
                .as_object_mut()
                .unwrap()
                .remove("observed_output_digest");
            verify["summary"] =
                json!("Only caller citation is available; outcome remains unknown.");
        } else if verdict == "fail" {
            verify["observation"] = json!(format!(
                "I independently read saved output {} and found its claimed proof insufficient for the bound obligation.",
                material["output_id"].as_str().unwrap()
            ));
            verify["summary"] = json!("Independent synthetic inspection finds insufficient proof.");
        }
        let receipt = route(
            verifier,
            "command",
            "pipeline.phase_effect.verify",
            verify.clone(),
        )
        .await;
        assert_eq!(receipt["verdict"], verdict);
        assert_eq!(receipt["run_id"], json!(run_id));
        assert_eq!(receipt["attempt_id"], json!(attempt_id));
        assert_eq!(receipt["effect_digest"], effect["effect_digest"]);
        assert_eq!(receipt["output_digest"], material["output_digest"]);
        assert_eq!(
            receipt["verifier_principal_id"],
            effect["verifier_principal_id"]
        );
        assert_eq!(
            receipt["verifier_session_id"],
            effect["verifier_session_id"]
        );
        let persisted: (Uuid,Uuid,String,String,String) = sqlx::query_as(
            "SELECT run_id,attempt_id,effect_digest,output_digest,verdict FROM pipeline_phase_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2")
            .bind(workspace).bind(Uuid::parse_str(verify["request_id"].as_str().unwrap()).unwrap()).fetch_one(pool).await.unwrap();
        assert_eq!(
            persisted,
            (
                run_id,
                attempt_id,
                effect["effect_digest"].as_str().unwrap().into(),
                material["output_digest"].as_str().unwrap().into(),
                verdict.into()
            )
        );
        let replay = route(verifier, "command", "pipeline.phase_effect.verify", verify).await;
        assert_eq!(replay, receipt);
    }
    let after = route(
        owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(after["run"], before_run);
    assert_eq!(after["attempts"].as_array().unwrap().len(), before_attempts);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pipeline_phase_effect_attestations WHERE workspace_id=$1 AND run_id=$2 AND attempt_id=$3")
        .bind(workspace).bind(run_id).bind(attempt_id).fetch_one(pool).await.unwrap();
    assert_eq!(count, 3);
}
