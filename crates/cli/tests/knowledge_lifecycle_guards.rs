#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    advance_create_after_baseline, advance_create_to_baseline, begin_create_request, context,
    method_reads, omit_nulls, output_data, output_digest,
};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

fn blocked_domain_checks(current: &Value, receipts: Vec<Value>, unresolved: &str) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    params["revisit_phase_id"] = json!("kc-domain-checks");
    let output = &mut params["output"];
    output["method_reads"] = method_reads(action);
    output["body"] = json!("The exact obligation remains unresolved and needs owner input.");
    output["data"] = json!({"phase":"kc-domain-checks","data":{
        "receipts":receipts,"unresolved_obligation_ids":[unresolved]}});
    output["verdict"] = json!("needs_context");
    output["outcome"] = json!("waiting_input");
    output["transition"] = json!("block");
    output["findings"] = json!([{"id":"unresolved-obligation",
        "summary":"Owner evidence is required for the exact obligation.",
        "owner_ref":"knowledge-change-owner","revisit_phase_id":"kc-domain-checks",
        "closed":false}]);
    output["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

fn blocked_baseline(current: &Value, baseline: Value) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    params["revisit_phase_id"] = json!("kc-resolve-baseline");
    let output = &mut params["output"];
    output["method_reads"] = method_reads(action);
    output["body"] =
        json!("The authored baseline conflict remains explicit and requires owner input.");
    output["data"] = json!({"phase":"kc-resolve-baseline","data":baseline});
    output["verdict"] = json!("needs_context");
    output["outcome"] = json!("waiting_input");
    output["transition"] = json!("block");
    output["findings"] = json!([{"id":"baseline-authority-conflict",
        "summary":"The exact fixture authority boundary needs owner reconciliation.",
        "owner_ref":"knowledge-change-owner","revisit_phase_id":"kc-resolve-baseline",
        "closed":false}]);
    output["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

fn review_params(
    current: &Value,
    outcome: &str,
    findings: Vec<Value>,
    revisit: Option<&str>,
) -> Value {
    let ctx = context(current);
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    let output = &mut params["output"];
    output["method_reads"] = method_reads(action);
    output["body"] = json!("Review the exact prior findings and pinned corrective output.");
    output["data"] = json!({"phase":"kc-review-reconcile","data":{
        "outcome":outcome,
        "reviewed_digests":[ctx["plan"]["digest"],
            output_data(ctx,"kc-prepare-change")["digest"],
            output_data(ctx,"kc-qualify-evidence")["source_pin_digest"],
            output_data(ctx,"kc-impact-plan")["digest"],
            output_digest(ctx,"kc-domain-checks")],
        "covered_operation_ids":ctx["plan"]["operation_ids"],
        "covered_obligation_ids":ctx["plan"]["obligations"].as_array().unwrap().iter()
            .map(|value|value["obligation_id"].clone()).collect::<Vec<_>>(),
        "findings":findings,
        "summary":"The exact corrective phase output determines closure."}});
    let waiting = outcome == "findings";
    output["verdict"] = json!(if waiting { "needs_context" } else { "ready" });
    output["outcome"] = json!(if waiting {
        "waiting_input"
    } else {
        "completed"
    });
    output["transition"] = json!(if waiting { "block" } else { "continue" });
    output["findings"] = output["data"]["data"]["findings"].clone();
    output["dispositions"] = json!([]);
    params["revisit_phase_id"] = revisit.map_or(Value::Null, |value| json!(value));
    omit_nulls(&mut params);
    params
}

#[tokio::test]
async fn unresolved_obligation_waits_for_input_and_cannot_be_reused() {
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
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("dk2-guards.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk2-guards-{}", Uuid::new_v4()));
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
        &format!("dk2-guards-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let document = fixture["document"].clone();
    let begun = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_create_request(&document, json!({"kind":"workspace"}), Uuid::new_v4()),
    )
    .await;
    let at_baseline = advance_create_to_baseline(&mut client, begun).await;
    let mut baseline = context(&at_baseline)["candidate_baseline"].clone();
    baseline["assessment_conflicts"] =
        json!(["The fixture authority basis requires explicit owner confirmation."]);
    baseline["conflicts"] = baseline["assessment_conflicts"].clone();
    let baseline_waiting = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        blocked_baseline(&at_baseline, baseline),
    )
    .await;
    assert_eq!(
        context(&baseline_waiting)["candidate_baseline"]["assessment_conflicts"],
        json!(["The fixture authority basis requires explicit owner confirmation."])
    );
    assert_eq!(
        context(&baseline_waiting)["candidate_baseline"]["conflicts"],
        context(&baseline_waiting)["candidate_baseline"]["assessment_conflicts"]
    );
    let baseline_input = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({"request_id":Uuid::new_v4(),
            "change_id":context(&baseline_waiting)["change_id"],
            "run_id":context(&baseline_waiting)["run"]["id"],
            "run_revision":context(&baseline_waiting)["run"]["revision"],
            "revisit_phase_id":"kc-resolve-baseline",
            "reason":"The authenticated owner resolves the explicit authority conflict.",
            "input":"The current workspace owner confirms the bounded fixture authority."}),
    )
    .await;
    let mut reconciled = context(&baseline_input)["candidate_baseline"].clone();
    reconciled["assessment_conflicts"] = json!([]);
    reconciled["conflicts"] = json!([]);
    let after_baseline = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &baseline_input,
        json!({"phase":"kc-resolve-baseline","data":reconciled}),
    )
    .await;
    let (at_checks, mut receipts) =
        advance_create_after_baseline(&mut client, &document, after_baseline).await;
    let unresolved_id = receipts[0]["obligation_id"].as_str().unwrap().to_owned();
    receipts[0]["disposition"] = json!("unresolved");
    receipts[0]["reason"] = json!("The exact owner evidence is not yet available.");
    let waiting = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        blocked_domain_checks(&at_checks, receipts.clone(), &unresolved_id),
    )
    .await;
    let waiting_context = context(&waiting);
    assert_eq!(waiting_context["run"]["status"], "waiting_input");
    assert_eq!(
        waiting_context["run"]["current_phase_id"],
        "kc-domain-checks"
    );
    let run_id = Uuid::parse_str(waiting_context["run"]["id"].as_str().unwrap()).unwrap();
    let unresolved_output: Uuid = sqlx::query_scalar(
        "SELECT id FROM knowledge_change_outputs WHERE run_id=$1 \
         AND phase_id='kc-domain-checks' ORDER BY revision DESC LIMIT 1",
    )
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut forged = receipts.clone();
    forged[0]["disposition"] = json!("reused");
    forged[0]["reused_receipt_id"] = json!(unresolved_output);
    forged[0]["reason"] = json!("An unresolved receipt cannot become reusable proof.");
    let action = &waiting["actions"][0];
    let mut forged_params = action_params(action).clone();
    forged_params["output"]["method_reads"] = method_reads(action);
    forged_params["output"]["body"] = json!("Attempt to reuse unresolved evidence.");
    forged_params["output"]["data"] = json!({"phase":"kc-domain-checks","data":{
        "receipts":forged,"unresolved_obligation_ids":[]}});
    forged_params["output"]["verdict"] = json!("ready");
    forged_params["output"]["outcome"] = json!("completed");
    forged_params["output"]["transition"] = json!("continue");
    forged_params["output"]["findings"] = json!([]);
    forged_params["output"]["dispositions"] = json!([]);
    forged_params["revisit_phase_id"] = Value::Null;
    omit_nulls(&mut forged_params);
    let rejected = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        forged_params,
    )
    .await;
    assert_eq!(
        rejected["error"]["code"], "invalid_arguments",
        "an unresolved obligation receipt must not be accepted as a reused proof"
    );

    let input = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({"request_id":Uuid::new_v4(),
            "change_id":waiting_context["change_id"],
            "run_id":waiting_context["run"]["id"],
            "run_revision":waiting_context["run"]["revision"],
            "revisit_phase_id":"kc-domain-checks",
            "reason":"Resolve the exact pending obligation with authenticated owner input.",
            "input":"The workspace owner supplies the required exact fixture authority evidence."}),
    )
    .await;
    let input_context = context(&input);
    let input_digests = input_context["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| input["digest"].clone())
        .collect::<Vec<_>>();
    let mut resolved = receipts;
    resolved[0]["disposition"] = json!("satisfied");
    resolved[0]["reason"] = json!("The authenticated owner input resolves this obligation.");
    for receipt in &mut resolved {
        receipt["input_digests"] = json!(input_digests);
    }
    let advanced = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &input,
        json!({"phase":"kc-domain-checks","data":{
            "receipts":resolved,"unresolved_obligation_ids":[]}}),
    )
    .await;
    assert_eq!(context(&advanced)["run"]["status"], "active");
    assert_eq!(
        context(&advanced)["run"]["current_phase_id"],
        "kc-impact-plan"
    );
    let impact = context(&advanced)["candidate_impact"].clone();
    let at_review = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &advanced,
        json!({"phase":"kc-impact-plan","data":impact}),
    )
    .await;
    let ctx = context(&at_review);
    let action = &at_review["actions"][0];
    let mut omitted = action_params(action).clone();
    omitted["output"]["method_reads"] = method_reads(action);
    omitted["output"]["body"] = json!("Attempt review with the KC06 checks digest omitted.");
    omitted["output"]["data"] = json!({"phase":"kc-review-reconcile","data":{
        "outcome":"ready","reviewed_digests":[ctx["plan"]["digest"],
            output_data(ctx,"kc-prepare-change")["digest"],
            output_data(ctx,"kc-qualify-evidence")["source_pin_digest"],
            output_data(ctx,"kc-impact-plan")["digest"]],
        "covered_operation_ids":ctx["plan"]["operation_ids"],
        "covered_obligation_ids":ctx["plan"]["obligations"].as_array().unwrap().iter()
            .map(|value|value["obligation_id"].clone()).collect::<Vec<_>>(),
        "findings":[],"summary":"An incomplete review must not advance."}});
    omitted["output"]["verdict"] = json!("ready");
    omitted["output"]["outcome"] = json!("completed");
    omitted["output"]["transition"] = json!("continue");
    omitted["output"]["findings"] = json!([]);
    omitted["output"]["dispositions"] = json!([]);
    omit_nulls(&mut omitted);
    let omission = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        omitted,
    )
    .await;
    assert_eq!(
        omission["error"]["code"], "needs_context",
        "KC08 must reject a review that omits the exact KC06 receipt digest"
    );
    let original_checks_digest = output_digest(context(&at_review), "kc-domain-checks").clone();
    let open_finding = json!({"id":"review-corrective-proof",
        "summary":"The exact domain-check corrective output must be newer than this finding.",
        "owner_ref":"knowledge-change-owner","revisit_phase_id":"kc-domain-checks",
        "closed":false});
    let first_waiting = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(
            &at_review,
            "findings",
            vec![open_finding.clone()],
            Some("kc-domain-checks"),
        ),
    )
    .await;
    let after_first_checks = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &first_waiting,
        json!({"phase":"kc-domain-checks","data":{
            "receipts":resolved.clone(),"unresolved_obligation_ids":[]}}),
    )
    .await;
    let first_impact = context(&after_first_checks)["candidate_impact"].clone();
    let second_review = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &after_first_checks,
        json!({"phase":"kc-impact-plan","data":first_impact}),
    )
    .await;
    let second_waiting = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(
            &second_review,
            "findings",
            vec![open_finding.clone()],
            Some("kc-domain-checks"),
        ),
    )
    .await;
    assert_eq!(
        context(&second_waiting)["run"]["status"],
        "waiting_input",
        "a repeated honest unresolved review checkpoint must persist"
    );
    let after_second_checks = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &second_waiting,
        json!({"phase":"kc-domain-checks","data":{
            "receipts":resolved,"unresolved_obligation_ids":[]}}),
    )
    .await;
    let second_impact = context(&after_second_checks)["candidate_impact"].clone();
    let final_review = knowledge_lifecycle_support::complete_agent(
        &mut client,
        &after_second_checks,
        json!({"phase":"kc-impact-plan","data":second_impact}),
    )
    .await;
    let omitted_prior = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(&final_review, "ready", vec![], None),
    )
    .await;
    assert_eq!(omitted_prior["error"]["code"], "needs_context");

    let close = |digest: Value| {
        json!({"id":"review-corrective-proof",
            "summary":"The exact domain-check corrective output closes this finding.",
            "owner_ref":"knowledge-change-owner","revisit_phase_id":"kc-domain-checks",
            "closed":true,"closure_output_digest":digest})
    };
    let unrelated = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(
            &final_review,
            "ready",
            vec![close(
                output_digest(context(&final_review), "kc-impact-plan").clone(),
            )],
            None,
        ),
    )
    .await;
    assert_eq!(unrelated["error"]["code"], "needs_context");
    let predating = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(
            &final_review,
            "ready",
            vec![close(original_checks_digest)],
            None,
        ),
    )
    .await;
    assert_eq!(predating["error"]["code"], "needs_context");
    let current_checks_digest = output_digest(context(&final_review), "kc-domain-checks").clone();
    let reviewed = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        review_params(
            &final_review,
            "ready",
            vec![close(current_checks_digest)],
            None,
        ),
    )
    .await;
    assert_eq!(
        context(&reviewed)["run"]["current_phase_id"],
        "kc-publication-gate"
    );
    client.finish().await;
}
