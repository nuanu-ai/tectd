use super::knowledge_lifecycle_support::{complete_agent, context, output_data, output_digest};
use crate::recovery_support::{Mcp, action_params};
use crate::support::route;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone)]
pub struct SingleOperation {
    pub operation: &'static str,
    pub unit_id: Option<Value>,
    pub expected_revision: Option<i64>,
    pub expected_lifecycle: Option<&'static str>,
    pub document: Option<Value>,
    pub revalidation: Option<Value>,
    pub successor: Option<Value>,
    pub replacement_bindings: Value,
    pub sources: Value,
    pub knowledge_kind: Value,
    pub profiles: Value,
    pub erasure: &'static str,
    pub authored_followup: bool,
}

pub async fn ready_single(client: &mut Mcp, spec: SingleOperation) -> Value {
    let mut hint = json!({
        "client_label":"operation-under-test","operation":spec.operation,
        "reason":"The exact reviewed source and current baseline support this operation.",
        "authority_basis":"Current authenticated workspace owner.","depends_on_labels":[]
    });
    if let Some(value) = &spec.unit_id {
        hint["unit_id"] = value.clone();
    }
    if let Some(value) = spec.expected_revision {
        hint["expected_revision"] = json!(value);
    }
    if let Some(value) = spec.expected_lifecycle {
        hint["expected_lifecycle"] = json!(value);
    }
    let mut current = route(
        client,
        "command",
        "knowledge.change_begin",
        json!({
            "request_id":Uuid::new_v4(),"intent":format!("Apply one reviewed {} operation.",spec.operation),
            "desired_outcome":"The exact reviewed canonical operation is committed.",
            "sources":spec.sources,"operation_hints":[hint],"owner":{"kind":"workspace"},
            "completion":{"canonical_result":true,"exact_delivery":true,"impact_recorded":true,
                "search":"not_required","erasure":spec.erasure},"delivery_mode":"phasewise"
        }),
    ).await;
    let origin = &context(&current)["origin"];
    current = complete_agent(client,&current,json!({"phase":"kc-intake","data":{
        "bounded_outcome":origin["desired_outcome"],"operation_hints":origin["operation_hints"],
        "authority_boundary":"Current authenticated workspace owner.","completion":origin["completion"]}})).await;
    ready_single_from_baseline(client, spec, current).await
}

pub async fn ready_single_from_baseline(
    client: &mut Mcp,
    spec: SingleOperation,
    mut current: Value,
) -> Value {
    let candidate = context(&current)["candidate_baseline"].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-resolve-baseline","data":candidate}),
    )
    .await;
    let assignment = context(&current)["origin"]["operations"][0].clone();
    current=complete_agent(client,&current,json!({"phase":"kc-qualify-plan","data":{"operations":[{
        "operation_id":assignment["operation_id"],"knowledge_kind":spec.knowledge_kind,
        "profiles":spec.profiles,"classification_basis":"Exact existing document kind and profile contract."}]}})).await;
    let ctx = context(&current);
    let indexes = (0..ctx["origin"]["source_pins"].as_array().unwrap().len())
        .map(|v| json!(v))
        .collect::<Vec<_>>();
    current=complete_agent(client,&current,json!({"phase":"kc-qualify-evidence","data":{
        "claims":[{"operation_id":assignment["operation_id"],"claim":"The exact sources support the bounded canonical operation.",
            "source_indexes":indexes,"assumptions":[],"gaps":[]}],
        "source_pins":ctx["origin"]["source_pins"],"source_pin_digest":ctx["candidate_source_pin_digest"],"unresolved_gaps":[]}})).await;
    let ctx = context(&current);
    let operation = &ctx["origin"]["operations"][0];
    let hint = &ctx["origin"]["operation_hints"][0];
    let mut planned = json!({
        "operation_id":operation["operation_id"],"unit_id":operation["unit_id"],
        "client_label":operation["client_label"],"operation":spec.operation,
        "replacement_bindings":spec.replacement_bindings,"reason":hint["reason"],
        "authority_basis":hint["authority_basis"],"dependency_operation_ids":operation["dependency_operation_ids"],
        "binding_pins":[]
    });
    if let Some(value) = spec.expected_revision {
        planned["expected_revision"] = json!(value);
    }
    if let Some(value) = spec.expected_lifecycle {
        planned["expected_lifecycle"] = json!(value);
    }
    if let Some(value) = spec.document {
        planned["document"] = value;
    }
    if let Some(value) = spec.revalidation {
        planned["revalidation"] = value;
    }
    if let Some(value) = spec.successor {
        planned["successor"] = value;
    }
    let evidence_digest = output_data(ctx, "kc-qualify-evidence")["source_pin_digest"].clone();
    current=complete_agent(client,&current,json!({"phase":"kc-prepare-change","data":{
        "revision":1,"operations":[planned],"semantic_diff":format!("Apply exact {} semantics.",spec.operation),
        "evidence_digest":evidence_digest,"digest":""}})).await;
    let ctx = context(&current);
    let changeset_digest = output_data(ctx, "kc-prepare-change")["digest"].clone();
    let input_digests = ctx["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| input["digest"].clone())
        .collect::<Vec<_>>();
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
            "reason":"Checked against the exact selected profile method and current typed source.",
            "changeset_digest":changeset_digest,"method_reads":methods,"input_digests":input_digests})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-domain-checks","data":{
        "receipts":receipts,"unresolved_obligation_ids":[]}}),
    )
    .await;
    let mut impact = context(&current)["candidate_impact"].clone();
    if spec.authored_followup {
        impact["followups"] = json!([{"reference":"operator-followup:verified-notice","owner_ref":"workspace-owner",
            "effect":"notify affected operator after commit","blocking":false}]);
    }
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
    current=complete_agent(client,&current,json!({"phase":"kc-review-reconcile","data":{"outcome":"ready",
        "reviewed_digests":reviewed,"covered_operation_ids":ctx["plan"]["operation_ids"],
        "covered_obligation_ids":obligations,"findings":[],"summary":"All typed inputs, checks and effects are reviewed."}})).await;
    route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&current["actions"][0]).clone(),
    )
    .await
}

pub async fn commit_single(client: &mut Mcp, spec: SingleOperation) -> Value {
    let publication = ready_single(client, spec).await;
    route(
        client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await
}

pub async fn ready_pair_erase(client: &mut Mcp, targets: [(Value, i64, &'static str); 2]) -> Value {
    let hints = targets
        .iter()
        .enumerate()
        .map(|(index, (unit, revision, lifecycle))| {
            json!({"client_label":format!("erase-target-{index}"),"operation":"erase",
                "unit_id":unit,"expected_revision":revision,"expected_lifecycle":lifecycle,
                "reason":"Erase this exact authorized unit and all registered owned copies.",
                "authority_basis":"Current authenticated workspace owner.","depends_on_labels":[]})
        })
        .collect::<Vec<_>>();
    let mut current = route(
        client,
        "command",
        "knowledge.change_begin",
        json!({"request_id":Uuid::new_v4(),"intent":"Erase two exact reviewed units atomically.",
            "desired_outcome":"Both units and their registered owned copies are erased in one compound change.",
            "sources":[],"operation_hints":hints,"owner":{"kind":"workspace"},
            "completion":{"canonical_result":true,"exact_delivery":true,"impact_recorded":true,
                "search":"not_required","erasure":"owned_live_copies"},"delivery_mode":"phasewise"}),
    )
    .await;
    let origin = &context(&current)["origin"];
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-intake","data":{"bounded_outcome":origin["desired_outcome"],
            "operation_hints":origin["operation_hints"],"authority_boundary":"Current authenticated workspace owner.",
            "completion":origin["completion"]}}),
    )
    .await;
    let candidate = context(&current)["candidate_baseline"].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-resolve-baseline","data":candidate}),
    )
    .await;
    let operations = context(&current)["origin"]["operations"]
        .as_array()
        .unwrap()
        .clone();
    let qualified = operations
        .iter()
        .map(|operation| json!({"operation_id":operation["operation_id"],"knowledge_kind":"constraint",
            "profiles":["general"],"classification_basis":"Exact current constraint unit and erasure contract."}))
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
            "claim":"The current owner authorizes the bounded exact erasure.",
            "source_indexes":[],"assumptions":[],"gaps":[]})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-qualify-evidence","data":{"claims":claims,
            "source_pins":ctx["origin"]["source_pins"],"source_pin_digest":ctx["candidate_source_pin_digest"],
            "unresolved_gaps":[]}}),
    )
    .await;
    let ctx = context(&current);
    let planned = operations
        .iter()
        .map(|operation| {
            let index = operation["client_label"]
                .as_str()
                .unwrap()
                .strip_prefix("erase-target-")
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let (unit, revision, lifecycle) = &targets[index];
            json!({"operation_id":operation["operation_id"],"unit_id":unit,
                "client_label":operation["client_label"],"operation":"erase",
                "expected_revision":revision,"expected_lifecycle":lifecycle,
                "replacement_bindings":[],"reason":"Erase this exact authorized unit and all registered owned copies.",
                "authority_basis":"Current authenticated workspace owner.",
                "dependency_operation_ids":operation["dependency_operation_ids"],"binding_pins":[]})
        })
        .collect::<Vec<_>>();
    let evidence_digest = output_data(ctx, "kc-qualify-evidence")["source_pin_digest"].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-prepare-change","data":{"revision":1,"operations":planned,
            "semantic_diff":"Erase both exact reviewed unit payloads and registered owned copies.",
            "evidence_digest":evidence_digest,"digest":""}}),
    )
    .await;
    let ctx = context(&current);
    let changeset_digest = output_data(ctx, "kc-prepare-change")["digest"].clone();
    let receipts = ctx["plan"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|obligation| {
            let methods = obligation["method_refs"].as_array().unwrap().iter().map(|method| {
                json!({"instruction_id":method["id"],"version":method["version"],"digest":method["digest"]})
            }).collect::<Vec<_>>();
            json!({"operation_id":obligation["operation_id"],"profile_id":obligation["profile_id"],
                "obligation_id":obligation["obligation_id"],"disposition":"satisfied",
                "reason":"Checked against the exact selected profile method and current typed state.",
                "changeset_digest":changeset_digest,"method_reads":methods,"input_digests":[]})
        })
        .collect::<Vec<_>>();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-domain-checks","data":{"receipts":receipts,"unresolved_obligation_ids":[]}}),
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
        .map(|value| value["obligation_id"].clone())
        .collect::<Vec<_>>();
    current = complete_agent(client,&current,json!({"phase":"kc-review-reconcile","data":{"outcome":"ready",
        "reviewed_digests":reviewed,"covered_operation_ids":ctx["plan"]["operation_ids"],
        "covered_obligation_ids":obligations,"findings":[],"summary":"Both exact erasures and their machine impact are reviewed."}})).await;
    route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&current["actions"][0]).clone(),
    )
    .await
}

pub async fn commit_pair_erase(
    client: &mut Mcp,
    targets: [(Value, i64, &'static str); 2],
) -> Value {
    let publication = ready_pair_erase(client, targets).await;
    route(
        client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await
}
