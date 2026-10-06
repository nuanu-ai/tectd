//! Source-specific phase completion, preserving the actual producer action.
use super::*;
use tect_domain::KnowledgeChangePhaseId;
pub fn phase_completion_action(value: &Value) -> &Value {
    metadata(value);
    let current = context(value);
    let phase: KnowledgeChangePhaseId =
        serde_json::from_value(current["run"]["current_phase_id"].clone()).unwrap();
    let authored = phase.agent_authored();
    let kind = if authored {
        "needs_context"
    } else {
        assert_eq!(phase, KnowledgeChangePhaseId::KcPublicationGate);
        "ready_call"
    };
    let actions: Vec<_> = value["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| {
            a["tool"] == "command" && a["arguments"]["route"] == "knowledge.change_phase_complete"
        })
        .collect();
    assert_eq!(actions.len(), 1);
    let action = actions[0];
    assert_eq!(action["kind"], kind);
    let mut action_keys = BTreeSet::from(["kind", "tool", "arguments"]);
    if authored {
        action_keys.insert("context_input");
    }
    assert_eq!(keys(action), action_keys);
    assert_eq!(
        keys(&action["arguments"]),
        BTreeSet::from(["route", "params"])
    );
    let params = &action["arguments"]["params"];
    let mut param_keys = BTreeSet::from([
        "request_id",
        "change_id",
        "run_id",
        "run_revision",
        "phase_id",
    ]);
    if authored {
        param_keys.insert("output");
    }
    assert_eq!(keys(params), param_keys);
    uuid(&params["request_id"]);
    assert_eq!(params["change_id"], current["change_id"]);
    assert_eq!(params["run_id"], current["run"]["id"]);
    assert_eq!(params["run_revision"], current["run"]["revision"]);
    assert_eq!(params["phase_id"], current["run"]["current_phase_id"]);
    if authored {
        let output = &params["output"];
        assert_eq!(
            keys(output),
            BTreeSet::from([
                "phase_id",
                "expected_run_revision",
                "plan_revision",
                "plan_digest",
                "consumed_outputs",
                "consumed_inputs",
                "baseline_guards",
                "source_digests"
            ])
        );
        assert_eq!(output["phase_id"], params["phase_id"]);
        assert_eq!(output["expected_run_revision"], params["run_revision"]);
        let descriptor = &action["context_input"];
        let mut descriptor_keys =
            BTreeSet::from(["required_method_reads", "required_obligations", "fields"]);
        if phase == KnowledgeChangePhaseId::KcResultHandoff
            && current.get("erased_no_change_proof").is_some()
        {
            descriptor_keys.extend(["erased_no_change_proof", "required_result"]);
            assert_eq!(
                descriptor["erased_no_change_proof"],
                current["erased_no_change_proof"]
            );
            assert_eq!(
                descriptor["required_result"],
                json!({"canonical":"no_change","user_outcome":"achieved","remaining_work":[],"publisher_receipt_id":"omit","effects":[]})
            );
        }
        assert_eq!(keys(descriptor), descriptor_keys);
        assert!(descriptor["required_method_reads"].is_array());
        assert!(descriptor["required_obligations"].is_array());
        let expected_fields = json!([
   {"path":"arguments.params.output.body","format":"Substantive result bound to the supplied exact machine pins."},
   {"path":"arguments.params.output.data","format":format!("Typed {} semantic data matching the phase schema.",phase.as_str())},
   {"path":"arguments.params.output.verdict","format":"Substantive verdict for this exact phase."},
   {"path":"arguments.params.output.method_reads","format":"Acknowledge only required_method_reads actually consumed from the delivered context."},
   {"path":"arguments.params.output.outcome","format":"completed, waiting_input, or blocked."},
   {"path":"arguments.params.output.transition","format":"continue, complete, block, or escalate as allowed by the current contract."},
   {"path":"arguments.params.output.findings","format":"Typed findings; use an empty list only when there are none."},
   {"path":"arguments.params.output.dispositions","format":"Exact dispositions; use an empty list only when none apply."}]);
        assert_eq!(descriptor["fields"], expected_fields);
    }
    action
}
