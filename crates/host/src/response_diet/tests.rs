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
fn all_pipeline_replies_are_snapshot_references_with_truthful_availability() {
    for overview in ["# Overview", "{\n \"v1_identity\": {}\n}"] {
        let mut data = json!({"context":run_context(overview)});
        compact_pipeline(pipeline_context_mut(&mut data).unwrap());
        let compact = &data["context"];
        assert_eq!(compact["delivery_scope"], "snapshot_reference");
        assert_eq!(
            compact["definition"],
            json!({"kind":"slice.lightweight-tdd-development","version":"1","digest":"d"})
        );
        assert_eq!(compact["counts"]["outputs"], 1);
        assert_eq!(compact["output_availability"]["complete"], true);
        for field in [
            "outputs",
            "outputs_complete",
            "bindings",
            "delivered_phases",
            "attempts",
            "inputs",
        ] {
            assert!(compact.get(field).is_none());
        }
        assert_eq!(
            compact["field_destinations"]["bindings_and_outputs"],
            json!({"view":"details","section":"outputs"})
        );
    }
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
