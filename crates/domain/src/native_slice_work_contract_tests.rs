use super::*;
use serde_json::{Value, json};
fn fixture() -> Value {
    let id = "11111111-1111-4111-8111-111111111111";
    let sha = "a".repeat(64);
    let source = |p| json!({"source_id":id,"path":p});
    json!({"contract_kind":"native_slice_work_contract_v1",
        "target":{"scope_id":id,"slice_id":id,"slice_revision":1,"run_id":id,"run_revision":4,"phase_id":NATIVE_WORK_CONTRACT_PHASE,"definition_kind":"slice.full-design-to-execution","definition_version":NATIVE_WORK_CONTRACT_VERSION,"definition_digest":sha},
        "session_declaration":{"workspace_id":id,"session_id":id,"host_id":id,"native_session_id":id},"source_checkpoint":null,
        "required_reads":[{"reference":{"kind":"native_output","phase_id":"slice-full-dev-entry-gate","output_revision":1,"digest":sha},"required":true},{"reference":{"kind":"native_output","phase_id":"slice-design-spec-shaper","output_revision":1,"digest":sha},"required":true}],
        "write_scope":{"allowed_roots":[source("src")],"allowed_paths":[source("src/file.rs")],"denied_roots":[source(".git"),source(".tect"),source("tect/workspace")],"before_hash_required":true},
        "authority":{"status":"authorized","source":{"kind":"native_input","input_id":id,"sequence":1,"digest":sha},"scope":[{"action":"source_edit","targets":[source("src/file.rs")]},{"action":"source_test","targets":[source("tests/focused.rs")]}],"limitations":"Source only; external effects need separate authority."},
        "proof_requirements":[{"id":"proof-1","obligation":"Focused source proof","evidence_kind":"native_output","completion_required":true}],
        "validation_requirements":[{"id":"validation-1","obligation":"Focused test","proof_requirement_ids":["proof-1"],"completion_required":true}],
        "result_closure":{"native_result_required":true,"summary_required":true,"evidence_required":true,"scope_impact_required":true,"remaining_work_required":true,"handoff_when_blocked":true},
        "refresh_resume":{"refresh_before_write":true,"recheck_authority":true,"rehash_required_reads":true,"verify_before_hash":true,"reconcile_unknown_outcome":true,"resume_from_current_native_context":true}})
}
#[test]
fn native_declaration_accepts_source_only_bounded_contract() {
    assert!(NativeSliceWorkContract::parse(&fixture().to_string()).is_ok());
}
#[test]
fn closed_shape_and_semantics_reject_rehashed_json_mutations() {
    let mutations: Vec<fn(&mut Value)> = vec![
        |v| v["member_id"] = json!("invented"),
        |v| v["session_declaration"]["principal_id"] = json!("invented"),
        |v| v["session_declaration"] = Value::Null,
        |v| v["session_declaration"]["session_id"] = json!(Uuid::nil()),
        |v| v["session_declaration"]["native_session_id"] = json!("not-native"),
        |v| v["target"]["definition_digest"] = json!("A".repeat(64)),
        |v| v["target"]["run_revision"] = json!(0),
        |v| v["required_reads"][0]["reference"]["member_id"] = json!("extra"),
        |v| v["required_reads"][0]["required"] = json!(false),
        |v| v["write_scope"]["allowed_paths"][0]["path"] = json!("../escape"),
        |v| v["write_scope"]["allowed_paths"][0]["path"] = json!("."),
        |v| v["write_scope"]["denied_roots"] = json!([]),
        |v| v["write_scope"]["before_hash_required"] = json!(false),
        |v| v["authority"]["status"] = json!("unknown"),
        |v| v["authority"]["source"] = Value::Null,
        |v| v["authority"]["scope"][0]["action"] = json!("deployment"),
        |v| v["authority"]["scope"][0]["targets"][0]["path"] = json!("src/other.rs"),
        |v| {
            let p = v["proof_requirements"][0].clone();
            v["proof_requirements"].as_array_mut().unwrap().push(p);
        },
        |v| v["validation_requirements"][0]["proof_requirement_ids"] = json!(["unknown"]),
        |v| v["refresh_resume"]["reconcile_unknown_outcome"] = json!(false),
        |v| {
            v.as_object_mut().unwrap().remove("source_checkpoint");
        },
        |v| {
            v.as_object_mut().unwrap().remove("session_declaration");
        },
        |v| {
            v["authority"].as_object_mut().unwrap().remove("source");
        },
    ];
    for mutate in mutations {
        let mut v = fixture();
        mutate(&mut v);
        assert!(
            NativeSliceWorkContract::parse(&v.to_string()).is_err(),
            "accepted {v}"
        );
    }
}
#[test]
fn denied_subtree_and_phase_basis_cannot_be_omitted() {
    let mut v = fixture();
    let id = v["write_scope"]["allowed_paths"][0]["source_id"].clone();
    v["write_scope"]["denied_roots"]
        .as_array_mut()
        .unwrap()
        .push(json!({"source_id":id,"path":"src/file.rs/private"}));
    assert!(NativeSliceWorkContract::parse(&v.to_string()).is_err());
    let mut v = fixture();
    v["required_reads"].as_array_mut().unwrap().remove(1);
    assert!(NativeSliceWorkContract::parse(&v.to_string()).is_err());
}
#[test]
fn canonical_uuid_spelling_is_enforced() {
    let mut v = fixture();
    v["target"]["scope_id"] = json!("11111111111141118111111111111111");
    assert!(NativeSliceWorkContract::parse(&v.to_string()).is_err());
}
#[test]
fn semantic_gate_is_exact_and_legacy_contracts_are_unchanged() {
    let mut definition: PipelineDefinitionSnapshot = serde_json::from_value(json!({
        "kind":"slice.full-design-to-execution","version":NATIVE_WORK_CONTRACT_VERSION,"digest":"a".repeat(64),
        "overview":{"id":"overview","version":"1","digest":"a".repeat(64),"body":"overview","origin_refs":[]},
        "default_mode":"phasewise","allowed_modes":["phasewise"],"completion_contract":"complete","escalation_contract":"escalate","forbidden_claims":[],
        "phases":[{"id":NATIVE_WORK_CONTRACT_PHASE,"ordinal":4,"title":"Contract","required":true,"disposition_required":false,"instructions":[],"skills":[],"resources":[],
            "required_artifacts":[{"name_pattern":NATIVE_WORK_CONTRACT_ARTIFACT,"media_type":"application/json","schema_resource_id":NATIVE_WORK_CONTRACT_SCHEMA,"schema_resource_digest":"a".repeat(64),"schema_ref":"schema:a","required":true,"minimum_matches":1}],
            "required_fields":[],"allowed_verdicts":["contract_ready"],"required_dispositions":[],"allowed_backward_to":[],"fresh_reviewer_input":false,"retry_policy":"repeatable","output_contract":"contract"}]
    })).unwrap();
    let request: CompletePipelinePhase=serde_json::from_value(json!({"request_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"run_revision":4,"phase_id":NATIVE_WORK_CONTRACT_PHASE,"outcome":"completed","transition":"continue","output":{"body":"legacy","producer_context_id":"fixture","verdict":"contract_ready","artifacts":[{"name":NATIVE_WORK_CONTRACT_ARTIFACT,"media_type":"application/json","body":"{}","digest":"a".repeat(64)}]}})).unwrap();
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_err()
    );
    definition.version = "0.6.0-native.engineering.2".into();
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_ok()
    );
    definition.version = NATIVE_WORK_CONTRACT_VERSION.into();
    definition.kind = PipelineKind::OperationalExecution;
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_ok()
    );
    definition.kind = PipelineKind::FullDesignToExecution;
    for (id, ordinal) in [
        ("slice-full-dev-entry-gate", 1),
        ("slice-design-spec-shaper", 3),
    ] {
        let mut phase = definition.phases[0].clone();
        phase.id = id.into();
        phase.ordinal = ordinal;
        definition.phases.push(phase);
    }
    let mut request = request;
    let mut valid = fixture();
    valid["target"]["run_id"] = json!(request.run_id);
    request.output.artifacts[0].body = valid.to_string();
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_ok()
    );
    let mut blocked = request.clone();
    blocked.output.verdict = Some("blocked_missing_artifact_contract".into());
    assert!(
        validate_native_work_contract_output(&blocked, &definition, &definition.phases[0]).is_err()
    );
    blocked.output.artifacts.clear();
    assert!(
        validate_native_work_contract_output(&blocked, &definition, &definition.phases[0]).is_ok()
    );
    let mut dependent = valid.clone();
    dependent["required_reads"].as_array_mut().unwrap().push(json!({"reference":{"kind":"native_output","phase_id":NATIVE_WORK_CONTRACT_PHASE,"output_revision":1,"digest":"a".repeat(64)},"required":true}));
    request.output.artifacts[0].body = dependent.to_string();
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_err()
    );
    valid["authority"]["source"] = json!({"kind":"native_output","phase_id":NATIVE_WORK_CONTRACT_PHASE,"output_revision":1,"digest":"a".repeat(64)});
    request.output.artifacts[0].body = valid.to_string();
    assert!(
        validate_native_work_contract_output(&request, &definition, &definition.phases[0]).is_err()
    );
}
#[test]
fn every_source_declaration_rejects_private_paths_but_denials_name_them() {
    let source = fixture()["write_scope"]["allowed_paths"][0]["source_id"].clone();
    for private in [
        ".git/config",
        "src/.git/config",
        ".tect/state",
        "src/.tect/state",
        "tect/workspace/state",
        "../escape",
        "/absolute",
    ] {
        let mut v = fixture();
        v["required_reads"].as_array_mut().unwrap().push(json!({"reference":{"kind":"source_file","source_id":source,"path":private,"content_sha256":"a".repeat(64)},"required":true}));
        assert!(
            NativeSliceWorkContract::parse(&v.to_string()).is_err(),
            "read accepted {private}"
        );
        for action in ["source_plan", "source_test"] {
            let mut v = fixture();
            v["authority"]["scope"][0]["action"] = json!(action);
            v["authority"]["scope"][0]["targets"][0]["path"] = json!(private);
            assert!(
                NativeSliceWorkContract::parse(&v.to_string()).is_err(),
                "{action} accepted {private}"
            );
        }
        let mut v = fixture();
        v["authority"]["source"] = json!({"kind":"source_file","source_id":source,"path":private,"content_sha256":"a".repeat(64)});
        assert!(
            NativeSliceWorkContract::parse(&v.to_string()).is_err(),
            "authority source accepted {private}"
        );
        let mut v = fixture();
        v["write_scope"]["allowed_roots"][0]["path"] = json!(private);
        assert!(
            NativeSliceWorkContract::parse(&v.to_string()).is_err(),
            "allowed root accepted {private}"
        );
    }
    let mut v = fixture();
    v["write_scope"]["allowed_roots"][0]["path"] = json!(".");
    assert!(
        NativeSliceWorkContract::parse(&v.to_string()).is_ok(),
        "root allowed scope and explicit private denials are valid"
    );
}

#[test]
fn zero_write_scope_only_accepts_plan_and_test_authority() {
    let mut read_only = fixture();
    read_only["write_scope"]["allowed_paths"] = json!([]);
    read_only["authority"]["scope"][0]["action"] = json!("source_plan");
    assert!(NativeSliceWorkContract::parse(&read_only.to_string()).is_ok());
    // Nonempty edit targets must remain inside actual allowed paths; zero paths denies every edit.
    for target in ["src/file.rs", "src", "tests/focused.rs"] {
        let mut edit = read_only.clone();
        edit["authority"]["scope"][0]["action"] = json!("source_edit");
        edit["authority"]["scope"][0]["targets"][0]["path"] = json!(target);
        assert!(
            NativeSliceWorkContract::parse(&edit.to_string()).is_err(),
            "zero-write scope accepted edit {target}"
        );
    }
    let mut no_context = read_only.clone();
    no_context["write_scope"]["allowed_roots"] = json!([]);
    assert!(NativeSliceWorkContract::parse(&no_context.to_string()).is_err());
}
