use super::*;
use std::path::Path;

pub(super) struct VerifierContractInput<'a> {
    pub(super) pool: &'a PgPool,
    pub(super) enrollment: &'a admin::Enrollment,
    pub(super) workspace: Uuid,
    pub(super) candidate_set: Uuid,
    pub(super) opportunity: Uuid,
    pub(super) save_request: Uuid,
    pub(super) binding: (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64, Uuid),
    pub(super) root: &'a Path,
    pub(super) socket: &'a Path,
    pub(super) key: &'a str,
    pub(super) author: &'a mut Mcp,
}

pub(super) async fn assert_public_verifier_contract(input: VerifierContractInput<'_>) {
    let VerifierContractInput {
        pool,
        enrollment,
        workspace,
        candidate_set,
        opportunity,
        save_request,
        binding,
        root,
        socket,
        key,
        author,
    } = input;
    let verifier = admin::prepare_verifier_enrollment(pool, enrollment.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(verifier.principal_id, enrollment.principal_id);
    let verifier_file = root.join("verifier-host.json");
    host_file(&verifier_file, &verifier.auth);
    let mut verifier_mcp =
        Mcp::start(socket, &verifier_file, &Uuid::new_v4().to_string(), key).await;
    route(&mut verifier_mcp, "command", "workspace.open", json!({})).await;
    let verify_request = json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,"candidate_set_id":candidate_set,
        "caller_link_id":binding.7,"caller_receipt_request_id":save_request,"target_revision":binding.6
    });
    let owner_forbidden = route_error(
        author,
        "command",
        "candidate.advisory.verify",
        verify_request.clone(),
    )
    .await;
    assert_eq!(owner_forbidden["error"]["code"], "forbidden");
    let before_verify: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2)")
        .bind(enrollment.tenant_id).bind(workspace).fetch_one(pool).await.unwrap();
    let pass = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        verify_request.clone(),
    )
    .await;
    assert_eq!(pass["observation"]["status"], "passed");
    assert_eq!(
        pass["observation"]["qualification"],
        "independently_observed"
    );
    assert_eq!(
        pass["observation"]["actor_id"],
        verifier.principal_id.to_string()
    );
    assert_eq!(pass["establishes_independent_approval"], false);
    assert_eq!(pass["establishes_current_acceptance"], false);
    let same_transaction: (String,String,String,String) = sqlx::query_as(
        "SELECT d.xmin::text,r.xmin::text,p.xmin::text,l.xmin::text FROM scope_candidate_drafts d JOIN scope_candidate_receipts r ON r.tenant_id=d.tenant_id AND r.workspace_id=d.workspace_id AND r.candidate_set_id=d.candidate_set_id AND r.result_revision=d.set_revision JOIN advisory_scope_caller_link l ON l.tenant_id=r.tenant_id AND l.workspace_id=r.workspace_id AND l.candidate_set_id=r.candidate_set_id AND l.caller_request_id=r.request_id JOIN advisory_scope_preservation_receipt p ON p.tenant_id=l.tenant_id AND p.workspace_id=l.workspace_id AND p.receipt_id=l.preservation_receipt_id WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.candidate_set_id=$3 AND d.set_revision=$4 AND r.request_id=$5"
    ).bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(binding.6).bind(save_request).fetch_one(pool).await.unwrap();
    assert_eq!(same_transaction.0, same_transaction.1);
    assert_eq!(same_transaction.0, same_transaction.2);
    assert_eq!(same_transaction.0, same_transaction.3);
    let persisted_evidence: Value = sqlx::query_scalar("SELECT evidence_payload FROM advisory_scope_selected_save_observation WHERE tenant_id=$1 AND workspace_id=$2 AND observation_id=$3")
        .bind(enrollment.tenant_id).bind(workspace).bind(id(&pass["observation"]["id"])).fetch_one(pool).await.unwrap();
    let source_before: (Uuid,i64) = sqlx::query_as("SELECT p.id,p.revision FROM programs p JOIN scope_candidate_sets c ON c.tenant_id=p.tenant_id AND c.workspace_id=p.workspace_id AND c.program_id=p.id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).fetch_one(pool).await.unwrap();
    sqlx::query(
        "UPDATE programs SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(source_before.0)
    .execute(pool)
    .await
    .unwrap();
    let mut stale_source_request = verify_request.clone();
    stale_source_request["request_id"] = json!(Uuid::new_v4());
    let stale_source = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        stale_source_request.clone(),
    )
    .await;
    sqlx::query("UPDATE programs SET revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .bind(source_before.0)
        .bind(source_before.1)
        .execute(pool)
        .await
        .unwrap();
    if let Ok(path) = std::env::var("JEV_V04_PROOF_PATH") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"inputs":"SYNTHETIC source/advice fixture", "caller":"actual authenticated MCP main-boundary save", "workspace":workspace,"candidate_set":candidate_set,"owner_fixture_principal":enrollment.principal_id,"verifier_principal":verifier.principal_id,"save_request":save_request,"target_revision":binding.6,"caller_link_id":binding.7,"same_transaction_xmin":same_transaction,"positive_verify_request":verify_request,"positive_verifier":pass,"positive_persisted_evidence":persisted_evidence,"negative_current_source_request":stale_source_request,"negative_current_source":stale_source})).unwrap()).unwrap();
    }
    assert_eq!(
        stale_source["observation"]["status"], "failed",
        "new independent observation must rebind current Program source"
    );
    assert!(
        stale_source["observation"]["reason_codes"]
            .to_string()
            .contains("preservation_missing_or_failed")
    );
    let after_stale: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2)")
        .bind(enrollment.tenant_id).bind(workspace).fetch_one(pool).await.unwrap();
    assert_eq!(before_verify, after_stale);

    assert_eq!(
        route(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            verify_request.clone()
        )
        .await,
        pass
    );
    let detail = route(
        &mut verifier_mcp,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    assert_eq!(
        detail["opportunity"]["selected_save_observation"]["qualification"],
        "independently_observed"
    );
    assert!(detail.get("scope_decomposition").is_none());
    let audit = route(
        &mut verifier_mcp,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set,"limit":50}),
    )
    .await;
    assert!(audit.to_string().contains("independently_observed"));
    for field in [
        "actor_id",
        "session_id",
        "status",
        "evidence_digest",
        "qualification",
        "verifier_digest",
    ] {
        let mut forged = verify_request.clone();
        forged[field] = json!("forged");
        let rejection = route_error(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            forged,
        )
        .await;
        assert_eq!(rejection["error"]["code"], "invalid_arguments", "{field}");
    }
    let mut wrong_target = verify_request.clone();
    wrong_target["caller_link_id"] = json!(Uuid::new_v4());
    assert_eq!(
        route_error(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            wrong_target
        )
        .await["error"]["code"],
        "forbidden"
    );
    let another_file = root.join("another-verifier-host.json");
    for (field, value) in [
        ("caller_receipt_request_id", json!(Uuid::new_v4())),
        ("target_revision", json!(binding.6 + 1)),
    ] {
        let mut forged = verify_request.clone();
        forged[field] = value;
        assert_eq!(
            route_error(
                &mut verifier_mcp,
                "command",
                "candidate.advisory.verify",
                forged
            )
            .await["error"]["code"],
            "forbidden",
            "{field} must match the server-held selected-save target"
        );
    }
    host_file(&another_file, &verifier.auth);
    let mut another_session =
        Mcp::start(socket, &another_file, &Uuid::new_v4().to_string(), key).await;
    route(&mut another_session, "command", "workspace.open", json!({})).await;
    assert_eq!(
        route_error(
            &mut another_session,
            "command",
            "candidate.advisory.verify",
            verify_request.clone()
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let foreign_owner = admin::enroll_host(pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let foreign_file = root.join("foreign-host.json");
    host_file(&foreign_file, &foreign_owner.auth);
    let mut foreign_mcp = Mcp::start(socket, &foreign_file, &Uuid::new_v4().to_string(), key).await;
    route(&mut foreign_mcp, "command", "workspace.open", json!({})).await;
    assert_eq!(
        route_error(
            &mut foreign_mcp,
            "query",
            "candidate.advisory.get",
            json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity})
        )
        .await["error"]["code"],
        "not_found"
    );
    let after_verify: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2)")
        .bind(enrollment.tenant_id).bind(workspace).fetch_one(pool).await.unwrap();
    assert_eq!(before_verify, after_verify);
    let retained_material: Value = sqlx::query_scalar("SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(binding.6).fetch_one(pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(binding.6).execute(pool).await.unwrap();
    let mut failed_request = verify_request.clone();
    failed_request["request_id"] = json!(Uuid::new_v4());
    let failure = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        failed_request,
    )
    .await;
    assert_eq!(failure["observation"]["status"], "failed");
    assert_eq!(
        failure["observation"]["qualification"],
        "independently_observed"
    );
    assert!(
        failure["observation"]["reason_codes"]
            .to_string()
            .contains("saved_material_missing_or_mismatched")
    );
    // Restore only the deliberate disposable-fixture corruption, then observe
    // the current effect again through a new authenticated Verifier request.
    sqlx::query("UPDATE scope_candidate_drafts SET payload=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(binding.6).bind(&retained_material).execute(pool).await.unwrap();
    let mut final_request = verify_request.clone();
    final_request["request_id"] = json!(Uuid::new_v4());
    let final_current = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        final_request.clone(),
    )
    .await;
    assert_eq!(final_current["observation"]["status"], "passed");
    let final_evidence: Value = sqlx::query_scalar("SELECT evidence_payload FROM advisory_scope_selected_save_observation WHERE tenant_id=$1 AND workspace_id=$2 AND observation_id=$3")
        .bind(enrollment.tenant_id).bind(workspace).bind(id(&final_current["observation"]["id"])).fetch_one(pool).await.unwrap();
    if let Ok(path) = std::env::var("JEV_V04_PROOF_PATH") {
        let mut proof: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        proof["final_current_request"] = final_request;
        proof["final_current_verifier"] = final_current;
        proof["final_current_persisted_evidence"] = final_evidence;
        proof["final_current_saved_material"] = retained_material;
        proof["negative_effect_counts_unchanged"] =
            json!(before_verify == after_verify && before_verify == after_stale);
        std::fs::write(path, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    }
    foreign_mcp.finish().await;
    another_session.finish().await;
    verifier_mcp.finish().await;
}
