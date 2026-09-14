use super::knowledge_lifecycle_support::{complete_agent, context, output_data, output_digest};
use crate::recovery_support::{Mcp, action_params};
use crate::support::route;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn ready_create_then_supersede(
    client: &mut Mcp,
    predecessor: Value,
    predecessor_revision: i64,
    successor_document: Value,
    replacement_bindings: Value,
) -> Value {
    let mut current=route(client,"command","knowledge.change_begin",json!({
        "request_id":Uuid::new_v4(),"intent":"Create an exact successor and supersede its predecessor atomically.",
        "desired_outcome":"The successor is current only after its predecessor is atomically superseded.",
        "sources":successor_document["sources"],"operation_hints":[
            {"client_label":"successor","operation":"create","reason":"Create the exact reviewed successor.",
                "authority_basis":"Current authenticated workspace owner.","depends_on_labels":[]},
            {"client_label":"predecessor","operation":"supersede","unit_id":predecessor,
                "expected_revision":predecessor_revision,"expected_lifecycle":"active",
                "reason":"Replace the predecessor only with the same-Change reviewed successor.",
                "authority_basis":"Current authenticated workspace owner.","depends_on_labels":["successor"]}],
        "owner":{"kind":"workspace"},"completion":{"canonical_result":true,"exact_delivery":true,
            "impact_recorded":true,"search":"not_required","erasure":"not_required"},"delivery_mode":"phasewise"})).await;
    let origin = &context(&current)["origin"];
    current=complete_agent(client,&current,json!({"phase":"kc-intake","data":{
        "bounded_outcome":origin["desired_outcome"],"operation_hints":origin["operation_hints"],
        "authority_boundary":"Current authenticated workspace owner.","completion":origin["completion"]}})).await;
    let baseline = context(&current)["candidate_baseline"].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-resolve-baseline","data":baseline}),
    )
    .await;
    let operations = context(&current)["origin"]["operations"]
        .as_array()
        .unwrap()
        .clone();
    let qualified = operations
        .iter()
        .map(|operation| {
            json!({"operation_id":operation["operation_id"],
        "knowledge_kind":"constraint","profiles":["general"],
        "classification_basis":"Exact General constraint operation in the compound plan."})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-qualify-plan","data":{"operations":qualified}}),
    )
    .await;
    let ctx = context(&current);
    let claims = operations
        .iter()
        .map(|operation| {
            json!({"operation_id":operation["operation_id"],
        "claim":"The exact reviewed plan supports this bounded compound operation.",
        "source_indexes":if operation["operation"]=="create" {json!([0])} else {json!([])},
        "assumptions":[],"gaps":[]})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-qualify-evidence","data":{
        "claims":claims,"source_pins":ctx["origin"]["source_pins"],
        "source_pin_digest":ctx["candidate_source_pin_digest"],"unresolved_gaps":[]}}),
    )
    .await;
    let ctx = context(&current);
    let create = ctx["origin"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["client_label"] == "successor")
        .unwrap();
    let supersede = ctx["origin"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["client_label"] == "predecessor")
        .unwrap();
    let evidence_digest = output_data(ctx, "kc-qualify-evidence")["source_pin_digest"].clone();
    current=complete_agent(client,&current,json!({"phase":"kc-prepare-change","data":{"revision":1,
        "operations":[{"operation_id":create["operation_id"],"unit_id":create["unit_id"],
            "client_label":"successor","operation":"create","document":successor_document,
            "replacement_bindings":[],"reason":"Create the exact reviewed successor.",
            "authority_basis":"Current authenticated workspace owner.","dependency_operation_ids":[],"binding_pins":[]},
            {"operation_id":supersede["operation_id"],"unit_id":supersede["unit_id"],
            "client_label":"predecessor","operation":"supersede","expected_revision":predecessor_revision,
            "expected_lifecycle":"active","successor":{"operation_id":create["operation_id"]},
            "replacement_bindings":replacement_bindings,
            "reason":"Replace the predecessor only with the same-Change reviewed successor.",
            "authority_basis":"Current authenticated workspace owner.",
            "dependency_operation_ids":supersede["dependency_operation_ids"],"binding_pins":[]}],
        "semantic_diff":"Create the distinct successor before atomically superseding the exact predecessor.",
        "evidence_digest":evidence_digest,"digest":""}})).await;
    let ctx = context(&current);
    let changeset = output_data(ctx, "kc-prepare-change")["digest"].clone();
    let receipts = ctx["plan"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|obligation| {
            let methods = obligation["method_refs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|method| {
                    json!({
            "instruction_id":method["id"],"version":method["version"],"digest":method["digest"]})
                })
                .collect::<Vec<_>>();
            json!({"operation_id":obligation["operation_id"],"profile_id":obligation["profile_id"],
            "obligation_id":obligation["obligation_id"],"disposition":"satisfied",
            "reason":"Checked against the exact selected profile method and compound plan.",
            "changeset_digest":changeset,"method_reads":methods,"input_digests":[]})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-domain-checks","data":{
        "receipts":receipts,"unresolved_obligation_ids":[]}}),
    )
    .await;
    let impact = context(&current)["candidate_impact"].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-impact-plan","data":impact}),
    )
    .await;
    let ctx = context(&current);
    let reviewed = vec![
        ctx["plan"]["digest"].clone(),
        output_data(ctx, "kc-prepare-change")["digest"].clone(),
        output_data(ctx, "kc-qualify-evidence")["source_pin_digest"].clone(),
        output_data(ctx, "kc-impact-plan")["digest"].clone(),
        output_digest(ctx, "kc-domain-checks").clone(),
    ];
    let obligations = ctx["plan"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["obligation_id"].clone())
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-review-reconcile","data":{"outcome":"ready",
        "reviewed_digests":reviewed,"covered_operation_ids":ctx["plan"]["operation_ids"],
        "covered_obligation_ids":obligations,"findings":[],
        "summary":"The exact compound order, evidence, checks and impact are reviewed."}}),
    )
    .await;
    route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&current["actions"][0]).clone(),
    )
    .await
}

pub async fn commit_create_then_supersede(
    client: &mut Mcp,
    predecessor: Value,
    predecessor_revision: i64,
    successor_document: Value,
    replacement_bindings: Value,
) -> Value {
    let publication = ready_create_then_supersede(
        client,
        predecessor,
        predecessor_revision,
        successor_document,
        replacement_bindings,
    )
    .await;
    route(
        client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await
}
