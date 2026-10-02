use super::*;

#[allow(dead_code)]
pub(crate) struct NativeContractFixtureFacts {
    state: Value,
    source_id: Value,
    content_sha256: String,
}

/// Reads authenticated fixture identity and actual selected source bytes. The
/// caller creates this file only in its private fixture repository beforehand.
#[allow(dead_code)]
pub(crate) async fn native_contract_fixture_facts(client: &mut Mcp) -> NativeContractFixtureFacts {
    let state = client.call("get_state", json!({})).await;
    let source = state["selected_worktrees"]
        .as_array()
        .unwrap()
        .first()
        .unwrap();
    let root = std::path::Path::new(source["path"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let file = root.join("native-contract-fixture.txt");
    assert_eq!(file.canonicalize().unwrap().parent().unwrap(), root);
    let content_sha256 = format!("{:x}", Sha256::digest(std::fs::read(file).unwrap()));
    NativeContractFixtureFacts {
        source_id: source["id"].clone(),
        state,
        content_sha256,
    }
}

#[allow(dead_code)]
pub(crate) fn completion_with_contract(
    context: &Value,
    mut request: Value,
    facts: &NativeContractFixtureFacts,
) -> Value {
    if context["run"]["definition_version"] != "0.6.0-native.engineering.3"
        || context["run"]["current_phase_id"] != "slice-contract-writer"
        || request["output"]["verdict"] != "contract_ready"
    {
        return request;
    }
    let prior = completion::consumed_outputs(context);
    let output_ref = |id: &str| {
        let output = prior.iter().find(|o| o["phase_id"] == id).unwrap();
        json!({"kind":"native_output","phase_id":output["phase_id"],
            "output_revision":output["output_revision"],"digest":output["digest"]})
    };
    let first = output_ref("slice-full-dev-entry-gate");
    let design = output_ref("slice-design-spec-shaper");
    let source = json!({"source_id":facts.source_id,"path":"native-contract-fixture.txt"});
    let run = &context["run"];
    let session = &facts.state["session"];
    let contract = json!({
        "contract_kind":"native_slice_work_contract_v1",
        "target":{"scope_id":run["scope_id"],"slice_id":run["slice_id"],"slice_revision":run["slice_revision"],
            "run_id":run["id"],"run_revision":run["revision"],"phase_id":"slice-contract-writer",
            "definition_kind":run["definition_kind"],"definition_version":run["definition_version"],"definition_digest":run["definition_digest"]},
        "session_declaration":{"workspace_id":facts.state["workspace"]["id"],"session_id":session["id"],
            "host_id":session["host_id"],"native_session_id":session["native_session_id"]},
        "source_checkpoint":context.get("source_checkpoint").cloned().unwrap_or(Value::Null),
        "required_reads":[{"reference":first,"required":true},{"reference":design,"required":true},
            {"reference":{"kind":"source_file","source_id":facts.source_id,"path":"native-contract-fixture.txt",
                "content_sha256":facts.content_sha256},"required":true}],
        "write_scope":{"allowed_roots":[{"source_id":facts.source_id,"path":"."}],"allowed_paths":[source],
            "denied_roots":[{"source_id":facts.source_id,"path":".git"},{"source_id":facts.source_id,"path":".tect"},
                {"source_id":facts.source_id,"path":"tect/workspace"}],"before_hash_required":true},
        "authority":{"status":"authorized","source":first,"scope":[{"action":"source_plan","targets":[source]},
            {"action":"source_test","targets":[source]}],"limitations":"Private synthetic QA fixture only. No business source writes, Git mutation, external effects or live acceptance authority."},
        "proof_requirements":[{"id":"private-fixture-proof","obligation":"Private fixture native provenance and source hash checks only.",
            "evidence_kind":"native_output","completion_required":true}],
        "validation_requirements":[{"id":"private-fixture-validation","obligation":"Private fixture backend rejects malformed or stale contract pins.",
            "proof_requirement_ids":["private-fixture-proof"],"completion_required":true}],
        "result_closure":{"native_result_required":true,"summary_required":true,"evidence_required":true,
            "scope_impact_required":true,"remaining_work_required":true,"handoff_when_blocked":true},
        "refresh_resume":{"refresh_before_write":true,"recheck_authority":true,"rehash_required_reads":true,
            "verify_before_hash":true,"reconcile_unknown_outcome":true,"resume_from_current_native_context":true}
    });
    let body = contract.to_string();
    let artifact = request["output"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["name"] == "work-order-contract.json")
        .unwrap();
    artifact["digest"] = json!(format!("{:x}", Sha256::digest(body.as_bytes())));
    artifact["body"] = json!(body);
    request
}
