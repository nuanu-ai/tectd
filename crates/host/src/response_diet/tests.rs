use super::*;

fn phase() -> Value {
    json!({"id":"p1","ordinal":1,"title":"Intent","instructions":[{"id":"i","body":"step"}]})
}

fn run_context(overview_body: &str) -> Value {
    json!({
        "run":{"id":"r"},
        "definition":{
            "kind":"slice.lightweight-tdd-development","version":"1","digest":"d",
            "overview":{"id":"o","version":"1","digest":"od","body":overview_body},
            "default_mode":"phasewise","allowed_modes":["phasewise"],
            "phases":[phase()],
            "completion_contract":"complete","escalation_contract":"escalate",
            "forbidden_claims":["claim"]
        },
        "delivered_phases":[phase()],
        "bindings":[{"phase_id":"p0","output_id":"o0","output_digest":"x"}],
        "outputs":[{"id":"o0","body":"previous output"}],
        "outputs_complete":true
    })
}

#[test]
fn mutation_reply_keeps_one_phase_and_drops_repeated_static_bodies() {
    let mut data = json!({"context":run_context("# Overview"),"result":null});
    let context = pipeline_context_mut(&mut data).unwrap();
    pipeline_context(context, false, None);
    let context = &data["context"];
    assert!(context.get("delivered_phases").is_none());
    assert_eq!(context["definition"]["phases"], json!([phase()]));
    assert!(context["definition"]["overview"].get("body").is_none());
    assert_eq!(context["definition"]["overview"]["digest"], "od");
    for field in PIPELINE_STATIC_FIELDS {
        assert!(context["definition"].get(field).is_none(), "{field}");
    }
    assert_eq!(context["outputs"], json!([]));
    assert_eq!(context["outputs_complete"], false);
    assert_eq!(context["bindings"][0]["output_id"], "o0");
}

#[test]
fn reread_keeps_native_overview_and_outputs_but_not_a_legacy_manifest() {
    let map = json!([{"ordinal":1,"id":"p1","title":"Intent"}]);
    let mut native = json!({"created":run_context("# Research overview")});
    pipeline_context(
        pipeline_context_mut(&mut native).unwrap(),
        true,
        Some(map.clone()),
    );
    let created = &native["created"];
    assert_eq!(
        created["definition"]["overview"]["body"],
        "# Research overview"
    );
    assert_eq!(created["definition"]["phase_map"], map);
    assert_eq!(created["definition"]["completion_contract"], "complete");
    assert_eq!(created["outputs"][0]["body"], "previous output");
    assert!(created.get("delivered_phases").is_none());

    let mut legacy = run_context("{\n  \"v1_identity\": {}\n}");
    pipeline_context(&mut legacy, true, Some(map));
    assert!(legacy["definition"]["overview"].get("body").is_none());
    assert_eq!(legacy["definition"]["overview"]["id"], "o");
}

#[test]
fn distinct_delivered_phases_are_not_treated_as_a_duplicate() {
    let mut context = run_context("# Overview");
    context["definition"]["phases"] = json!([]);
    pipeline_context(&mut context, true, None);
    assert_eq!(context["delivered_phases"], json!([phase()]));
}

#[test]
fn planning_reply_drops_manifest_method_copy_and_delivered_snapshot_bodies() {
    let method = json!({"id":"m","revision":"1","digest":"md","body":"method text"});
    let mut context = json!({
        "candidate_set":{"id":"c","revision":2},
        "snapshot":{"id":"s","method":method,
            "rules":[{"id":"r","revision":"1","text":"rule","applicability":"all"}],
            "catalogue":{"digest":"cd","revision":"1","entries":[{"id":"e"}]},
            "source_refs":[{"id":"sr"}]},
        "planning_knowledge":{"manifest":{"id":"k","digest":"kd","workspace_generation":3,
            "needs":{"stage":"scope","method":{"id":"m","version":"1","digest":"md","body":"method text"}}}}
    });
    planning_context(&mut context, true);
    assert!(
        context["planning_knowledge"]["manifest"]["needs"]["method"]
            .get("body")
            .is_none()
    );
    assert_eq!(context["planning_knowledge"]["manifest"]["digest"], "kd");
    assert_eq!(context["snapshot"]["method"]["body"], "method text");

    planning_context(&mut context, false);
    let snapshot = &context["snapshot"];
    assert!(snapshot["method"].get("body").is_none());
    assert_eq!(snapshot["method"]["digest"], "md");
    assert!(snapshot["rules"][0].get("text").is_none());
    assert_eq!(snapshot["rules"][0]["id"], "r");
    assert!(snapshot["catalogue"].get("entries").is_none());
    assert_eq!(snapshot["catalogue"]["digest"], "cd");
    assert_eq!(snapshot["source_refs"][0]["id"], "sr");
}

#[test]
fn outcome_planning_reaches_created_and_replayed_contexts() {
    let context = json!({"snapshot":{"method":{"id":"m","body":"text"}}});
    let mut value = json!({"created":{"planning":context.clone()},"replay":{"planning":context}});
    outcome_planning(&mut value, "planning", false);
    assert!(
        value["created"]["planning"]["snapshot"]["method"]
            .get("body")
            .is_none()
    );
    assert!(
        value["replay"]["planning"]["snapshot"]["method"]
            .get("body")
            .is_none()
    );
}

#[test]
fn saved_program_keeps_state_and_identity_only() {
    let mut program = json!({
        "id":"p","revision":2,"status":"open","current_step":"ready","input_cursor":1,
        "intent":"i","basis":"b","boundaries":"bo","constraints":"c","success":"s",
        "working_notes":"w",
        "planning_knowledge":{"manifest":{"id":"k","needs":{"method":{"id":"m","body":"text"}}}}
    });
    saved_program(&mut program);
    for field in PROGRAM_SAVED_FIELDS {
        assert!(program.get(field).is_none(), "{field}");
    }
    assert_eq!(program["revision"], 2);
    assert_eq!(program["current_step"], "ready");
    assert!(
        program["planning_knowledge"]["manifest"]["needs"]["method"]
            .get("body")
            .is_none()
    );
}
