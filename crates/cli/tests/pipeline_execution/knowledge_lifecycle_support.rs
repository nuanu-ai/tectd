use crate::recovery_support::{Mcp, action_params};
use crate::support::route;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub struct CommittedKnowledge {
    pub receipt: Value,
    pub exact: Value,
}

pub fn context(value: &Value) -> &Value {
    value
        .get("created")
        .or_else(|| value.get("advanced"))
        .or_else(|| value.get("current"))
        .or_else(|| value.get("replay"))
        .expect("lifecycle response must carry its current context")
}

pub async fn run_manifest_checkpoint(pool: &PgPool, run_id: Uuid) -> (i64, Option<Uuid>, i64) {
    sqlx::query_as(
        "SELECT r.revision,r.knowledge_manifest_id,(SELECT count(*) FROM pipeline_knowledge_manifests m WHERE m.tenant_id=r.tenant_id AND m.workspace_id=r.workspace_id AND m.run_id=r.id) FROM slice_pipeline_runs r WHERE r.id=$1",
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub(crate) fn output_data<'a>(context: &'a Value, phase: &str) -> &'a Value {
    &context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["output"]["phase_id"] == phase)
        .unwrap()["output"]["data"]["data"]
}

pub(crate) fn output_digest<'a>(context: &'a Value, phase: &str) -> &'a Value {
    &context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["output"]["phase_id"] == phase)
        .unwrap()["digest"]
}

pub(crate) fn method_reads(action: &Value) -> Value {
    Value::Array(
        action["context_input"]["required_method_reads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                json!({"instruction_id":value["instruction_id"],
                    "version":value["version"],"digest":value["digest"]})
            })
            .collect(),
    )
}

pub(crate) fn omit_nulls(value: &mut Value) {
    match value {
        Value::Object(values) => {
            values.retain(|_, value| !value.is_null());
            values.values_mut().for_each(omit_nulls);
        }
        Value::Array(values) => values.iter_mut().for_each(omit_nulls),
        _ => {}
    }
}

pub(crate) async fn complete_agent(client: &mut Mcp, current: &Value, data: Value) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    let output = &mut params["output"];
    output["method_reads"] = method_reads(action);
    output["body"] = json!("Exact mechanical integration fixture output for this pinned phase.");
    output["data"] = data;
    output["verdict"] = json!("ready");
    output["outcome"] = json!("completed");
    output["transition"] = json!("continue");
    output["findings"] = json!([]);
    output["dispositions"] = json!([]);
    omit_nulls(&mut params);
    route(client, "command", "knowledge.change_phase_complete", params).await
}

pub async fn query_current(client: &mut Mcp, change_id: &Value) -> Value {
    route(
        client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )
    .await
}

pub async fn commit_create(client: &mut Mcp, document: Value) -> CommittedKnowledge {
    let request = begin_create_request(&document, json!({"kind":"workspace"}), Uuid::new_v4());
    let current = route(client, "command", "knowledge.change_begin", request).await;
    commit_create_from_current(client, document, current).await
}

pub(crate) fn begin_create_request(document: &Value, owner: Value, request_id: Uuid) -> Value {
    json!({
        "request_id":request_id,
        "intent":"Publish the exact pinned business-operation identity constraint.",
        "desired_outcome":"One exact reviewed current knowledge unit is published.",
        "sources":document["sources"],
        "operation_hints":[{"client_label":"knowledge-document","operation":"create",
            "reason":"The pinned source supports this bounded document.",
            "authority_basis":"Current authenticated workspace owner."}],
        "owner":owner,
        "completion":{"canonical_result":true,"exact_delivery":true,
            "impact_recorded":true,"search":"not_required","erasure":"not_required"},
        "delivery_mode":"phasewise"
    })
}

pub(crate) async fn advance_create_to_domain_checks(
    client: &mut Mcp,
    document: &Value,
    current: Value,
) -> (Value, Vec<Value>) {
    advance_create_to_domain_checks_with_identity(client, document, current, Vec::new()).await
}

pub(crate) async fn advance_create_to_domain_checks_with_identity(
    client: &mut Mcp,
    document: &Value,
    current: Value,
    identity_matches: Vec<Value>,
) -> (Value, Vec<Value>) {
    let mut current = advance_create_to_baseline(client, current).await;
    let mut candidate = context(&current)["candidate_baseline"].clone();
    candidate["identity_matches"] = Value::Array(identity_matches);
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-resolve-baseline","data":candidate}),
    )
    .await;
    advance_create_after_baseline(client, document, current).await
}

pub(crate) async fn advance_create_to_baseline(client: &mut Mcp, mut current: Value) -> Value {
    let origin = &context(&current)["origin"];
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-intake","data":{
            "bounded_outcome":origin["desired_outcome"],
            "operation_hints":origin["operation_hints"],
            "authority_boundary":"Current authenticated workspace owner.",
            "completion":origin["completion"]}}),
    )
    .await;
    current
}

pub(crate) async fn advance_create_after_baseline(
    client: &mut Mcp,
    document: &Value,
    current: Value,
) -> (Value, Vec<Value>) {
    let current = advance_create_to_prepare(client, document, current).await;
    let params = create_prepare_params(&current, document);
    let current = route(client, "command", "knowledge.change_phase_complete", params).await;
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
            let methods = obligation["method_refs"].as_array().unwrap().iter().map(|method| {
                json!({"instruction_id":method["id"],"version":method["version"],"digest":method["digest"]})
            }).collect::<Vec<_>>();
            json!({"operation_id":obligation["operation_id"],"profile_id":obligation["profile_id"],
                "obligation_id":obligation["obligation_id"],"disposition":"satisfied",
                "reason":"Checked against the exact selected profile method and typed source.",
                "changeset_digest":changeset_digest,"method_reads":methods,"input_digests":input_digests})
        })
        .collect::<Vec<_>>();
    (current, receipts)
}

pub(crate) async fn advance_create_to_prepare(
    client: &mut Mcp,
    document: &Value,
    mut current: Value,
) -> Value {
    let operation = context(&current)["origin"]["operations"][0].clone();
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-qualify-plan","data":{"operations":[{
            "operation_id":operation["operation_id"],"knowledge_kind":document["knowledge_kind"],
            "profiles":document["profiles"],"classification_basis":"Exact fixture kind and profiles from the pinned source."}]}}),
    )
    .await;
    let ctx = context(&current);
    current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-qualify-evidence","data":{
            "claims":[{"operation_id":operation["operation_id"],
                "claim":"The exact declaration supports the bounded authentication constraint.",
                "source_indexes":[0],"assumptions":[],"gaps":[]}],
            "source_pins":ctx["origin"]["source_pins"],
            "source_pin_digest":ctx["candidate_source_pin_digest"],"unresolved_gaps":[]}}),
    )
    .await;
    current
}

pub(crate) fn create_prepare_params(current: &Value, document: &Value) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    let ctx = context(current);
    let operation = &ctx["origin"]["operations"][0];
    let hint = &ctx["origin"]["operation_hints"][0];
    let evidence_digest = output_data(ctx, "kc-qualify-evidence")["source_pin_digest"].clone();
    let output = &mut params["output"];
    output["method_reads"] = method_reads(action);
    output["body"] = json!("Exact mechanical integration fixture output for this pinned phase.");
    output["data"] = json!({"phase":"kc-prepare-change","data":{"revision":1,"operations":[{
            "operation_id":operation["operation_id"],"unit_id":operation["unit_id"],
            "client_label":operation["client_label"],"operation":"create",
            "document":document,
            "replacement_bindings":[],"reason":hint["reason"],"authority_basis":hint["authority_basis"],
            "dependency_operation_ids":[],"binding_pins":[]}],
            "semantic_diff":"Publish the exact source-derived document as the requested current revision.",
            "evidence_digest":evidence_digest,"digest":""}});
    output["verdict"] = json!("ready");
    output["outcome"] = json!("completed");
    output["transition"] = json!("continue");
    output["findings"] = json!([]);
    output["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

pub(crate) async fn commit_create_from_current(
    client: &mut Mcp,
    document: Value,
    current: Value,
) -> CommittedKnowledge {
    let current = advance_create_to_review(client, &document, current).await;
    let current = complete_review(client, &current, "ready").await;
    let publication = route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&current["actions"][0]).clone(),
    )
    .await;
    let committed = route(
        client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await;
    let receipt = committed["applied"].clone();
    let exact = route(
        client,
        "query",
        "knowledge.unit",
        json!({"unit_id":receipt["applied_operations"][0]["unit_id"],
            "revision":receipt["applied_operations"][0]["revision"]}),
    )
    .await;
    assert_eq!(exact["document"]["document"], document);
    CommittedKnowledge { receipt, exact }
}

pub(crate) async fn advance_create_to_review(
    client: &mut Mcp,
    document: &Value,
    current: Value,
) -> Value {
    advance_create_to_review_with_identity(client, document, current, Vec::new()).await
}

pub(crate) async fn advance_create_to_review_with_identity(
    client: &mut Mcp,
    document: &Value,
    current: Value,
    identity_matches: Vec<Value>,
) -> Value {
    let (mut current, receipts) =
        advance_create_to_domain_checks_with_identity(client, document, current, identity_matches)
            .await;
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
    current
}

pub(crate) async fn complete_review(client: &mut Mcp, current: &Value, outcome: &str) -> Value {
    let ctx = context(current);
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
    complete_agent(
        client,
        current,
        json!({"phase":"kc-review-reconcile","data":{"outcome":outcome,
            "reviewed_digests":reviewed,"covered_operation_ids":ctx["plan"]["operation_ids"],
            "covered_obligation_ids":obligations,"findings":[],
            "summary":"The exact typed source, plan, checks and impact are ready for canonical publication."}}),
    )
    .await
}

pub async fn settle_and_finish(client: &mut Mcp, committed: &CommittedKnowledge) -> Value {
    settle_and_finish_receipt(client, &committed.receipt).await
}

pub async fn settle_and_finish_receipt(client: &mut Mcp, receipt: &Value) -> Value {
    let current = query_current(client, &receipt["change_id"]).await;
    let settled = route(
        client,
        "command",
        "knowledge.change_settle_effects",
        action_params(&current["actions"][0]).clone(),
    )
    .await;
    assert_eq!(settled["settled"]["required_complete"], true);
    let current = query_current(client, &receipt["change_id"]).await;
    let ctx = context(&current);
    let erased = receipt.get("operations").is_some();
    let mut terminal = current["actions"][0].clone();
    terminal["arguments"]["params"]["output"]["method_reads"] = method_reads(&terminal);
    terminal["arguments"]["params"]["output"]["body"] = if erased {
        json!("Backend-generated opaque terminal handoff for an erased knowledge change.")
    } else {
        json!("Canonical publication and required synchronous effects completed.")
    };
    terminal["arguments"]["params"]["output"]["data"] = json!({"phase":"kc-result-handoff","data":{
        "canonical":"applied","user_outcome":"achieved",
        "summary":if erased { "Knowledge change completed after its owned semantic payload was erased." } else { "The exact reviewed unit is current." },
        "remaining_work":ctx["effects_report"]["remaining_work"],
        "publisher_receipt_id":receipt["id"],"effects":ctx["effects_report"]["effects"]}});
    terminal["arguments"]["params"]["output"]["verdict"] = json!("complete");
    terminal["arguments"]["params"]["output"]["outcome"] = json!("completed");
    terminal["arguments"]["params"]["output"]["transition"] = json!("complete");
    terminal["arguments"]["params"]["output"]["findings"] = json!([]);
    terminal["arguments"]["params"]["output"]["dispositions"] = json!([]);
    route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&terminal).clone(),
    )
    .await
}
