#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    advance_create_to_baseline, begin_create_request, complete_agent, context, method_reads,
    omit_nulls,
};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route};
use tect_postgres::admin;
use uuid::Uuid;

fn blocked_baseline(current: &Value) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    let mut baseline = context(current)["candidate_baseline"].clone();
    baseline["assessment_gaps"] = json!(["Owner input is required for this exact fixture."]);
    baseline["missing_context"] = baseline["assessment_gaps"].clone();
    params["revisit_phase_id"] = json!("kc-resolve-baseline");
    params["output"]["method_reads"] = method_reads(action);
    params["output"]["body"] = json!("Persist the exact unresolved basis gap.");
    params["output"]["data"] = json!({"phase":"kc-resolve-baseline","data":baseline});
    params["output"]["verdict"] = json!("needs_context");
    params["output"]["outcome"] = json!("waiting_input");
    params["output"]["transition"] = json!("block");
    params["output"]["findings"] = json!([{"id":"mode-gap",
        "summary":"Owner input is required.","owner_ref":"knowledge-change-owner",
        "revisit_phase_id":"kc-resolve-baseline","closed":false}]);
    params["output"]["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

async fn begin_whole(client: &mut Mcp, document: &Value) -> Value {
    let mut request = begin_create_request(document, json!({"kind":"workspace"}), Uuid::new_v4());
    request["delivery_mode"] = json!("whole");
    route(client, "command", "knowledge.change_begin", request).await
}

#[tokio::test]
async fn substantive_gap_and_explicit_reason_escalate_delivery_one_way() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    repository(&root.join("source"));
    let socket = root.join("delivery-mode.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk2-mode-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("dk2-mode-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let document = &fixture["document"];

    let whole = begin_whole(&mut client, document).await;
    assert_eq!(context(&whole)["run"]["delivery_mode"], "whole");
    let at_baseline = advance_create_to_baseline(&mut client, whole).await;
    let waiting = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        blocked_baseline(&at_baseline),
    )
    .await;
    assert_eq!(context(&waiting)["run"]["delivery_mode"], "phasewise");
    assert_eq!(
        context(&waiting)["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let resumed = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({"request_id":Uuid::new_v4(),"change_id":context(&waiting)["change_id"],
            "run_id":context(&waiting)["run"]["id"],"run_revision":context(&waiting)["run"]["revision"],
            "revisit_phase_id":"kc-resolve-baseline","reason":"Resolve the exact gap.",
            "input":"The current owner supplies the requested bounded context."}),
    )
    .await;
    assert_eq!(context(&resumed)["run"]["delivery_mode"], "phasewise");

    let second = begin_whole(&mut client, document).await;
    let at_second_baseline = advance_create_to_baseline(&mut client, second).await;
    let baseline = context(&at_second_baseline)["candidate_baseline"].clone();
    let at_plan = complete_agent(
        &mut client,
        &at_second_baseline,
        json!({"phase":"kc-resolve-baseline","data":baseline}),
    )
    .await;
    let action = &at_plan["actions"][0];
    let operation = context(&at_plan)["origin"]["operations"][0].clone();
    let mut params = action_params(action).clone();
    params["output"]["phasewise_reason"] =
        json!("The qualified plan requires focused phase-by-phase presentation.");
    params["output"]["method_reads"] = method_reads(action);
    params["output"]["body"] = json!("Qualify the exact plan and escalate presentation mode.");
    params["output"]["data"] = json!({"phase":"kc-qualify-plan","data":{"operations":[{
        "operation_id":operation["operation_id"],"knowledge_kind":document["knowledge_kind"],
        "profiles":document["profiles"],"classification_basis":"Exact fixture kind and profiles."}]}});
    params["output"]["verdict"] = json!("ready");
    params["output"]["outcome"] = json!("completed");
    params["output"]["transition"] = json!("continue");
    params["output"]["findings"] = json!([]);
    params["output"]["dispositions"] = json!([]);
    omit_nulls(&mut params);
    let phasewise = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        params,
    )
    .await;
    assert_eq!(context(&phasewise)["run"]["delivery_mode"], "phasewise");
    assert_eq!(context(&phasewise)["plan"]["delivery_mode"], "phasewise");
    assert_eq!(
        context(&phasewise)["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let ordinary = begin_whole(&mut client, document).await;
    let ordinary = advance_create_to_baseline(&mut client, ordinary).await;
    let ordinary = route(&mut client,"command","knowledge.change_record_input",json!({
        "request_id":Uuid::new_v4(),"change_id":context(&ordinary)["change_id"],
        "run_id":context(&ordinary)["run"]["id"],"run_revision":context(&ordinary)["run"]["revision"],
        "revisit_phase_id":"kc-resolve-baseline","reason":"Record exact ordinary context.",
        "input":"This ordinary input does not amend a target or replace a source."})).await;
    assert_eq!(context(&ordinary)["run"]["delivery_mode"], "phasewise");
    client.finish().await;
}
