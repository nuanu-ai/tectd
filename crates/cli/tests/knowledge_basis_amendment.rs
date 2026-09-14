#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    commit_create, context, query_current, settle_and_finish_receipt,
};
use knowledge_operation_support::{
    SingleOperation, commit_single, ready_single, ready_single_from_baseline,
};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

fn revise_spec(unit: Value, revision: i64, document: Value) -> SingleOperation {
    SingleOperation {
        operation: "revise",
        unit_id: Some(unit),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        sources: document["sources"].clone(),
        document: Some(document),
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        knowledge_kind: json!("constraint"),
        profiles: json!(["general"]),
        erasure: "not_required",
        authored_followup: false,
    }
}

fn guard(exact: &Value) -> Value {
    json!({"unit_id":exact["document"]["unit_id"],
        "revision":exact["document"]["revision"],"lifecycle":exact["document"]["lifecycle"],
        "rdf_digest":exact["document"]["rdf_digest"],"unit_iri":exact["document"]["unit_iri"],
        "revision_iri":exact["document"]["revision_iri"]})
}

fn amendment_request(
    current: &Value,
    request_id: Uuid,
    update: Value,
    sources: Option<Value>,
) -> Value {
    let ctx = context(current);
    let mut amendment = json!({"target_updates":[update]});
    if let Some(sources) = sources {
        amendment["replacement_sources"] = sources;
    }
    json!({"request_id":request_id,"change_id":ctx["change_id"],"run_id":ctx["run"]["id"],
        "run_revision":ctx["run"]["revision"],"revisit_phase_id":"kc-resolve-baseline",
        "reason":"Acknowledge the exact newly observed native target basis.",
        "input":"The owner explicitly requalifies against the exact current target revision.",
        "basis_amendment":amendment})
}

#[tokio::test]
async fn exact_target_repin_reuses_one_cursor_and_invalid_amendments_roll_back() {
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
    let socket = root.join("basis-amendment.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk2-basis-{}", Uuid::new_v4()));
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
        &format!("dk2-basis-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let original = fixture["document"].clone();
    let created = commit_create(&mut client, original.clone()).await;
    let unit = created.receipt["applied_operations"][0]["unit_id"].clone();

    let mut intended = original.clone();
    intended["title"] = json!("Owner-qualified target after explicit basis reconciliation");
    intended["canonical_text"] = json!(
        "The same cursor publishes this owner-qualified revision only after an exact native target repin."
    );
    let pending = ready_single(&mut client, revise_spec(unit.clone(), 1, intended.clone())).await;
    let old_commit = action_params(&pending["actions"][0]).clone();

    let mut intervening = original.clone();
    intervening["title"] = json!("Independent current target revision");
    intervening["canonical_text"] = json!(
        "An independent reviewed change advances the target before the pending change commits."
    );
    let independent = commit_single(&mut client, revise_spec(unit.clone(), 1, intervening)).await;
    assert_eq!(
        independent["applied"]["applied_operations"][0]["revision"],
        2
    );
    let exact = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit,"revision":2}),
    )
    .await;
    let stale = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        old_commit.clone(),
    )
    .await;
    assert_eq!(stale["error"]["code"], "stale_context");
    let pending_ctx = context(&pending);
    let operation_id = pending_ctx["origin"]["operations"][0]["operation_id"].clone();
    let valid_update = json!({"operation_id":operation_id,"previous_expected_revision":1,
        "previous_expected_lifecycle":"active","replacement_guard":guard(&exact)});

    let mut wrong_old = valid_update.clone();
    wrong_old["previous_expected_revision"] = json!(2);
    let wrong = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(&pending, Uuid::new_v4(), wrong_old, None),
    )
    .await;
    assert_eq!(wrong["error"]["code"], "context_changed");
    let mut forged = valid_update.clone();
    forged["replacement_guard"]["rdf_digest"] = json!("forged-native-digest");
    let forged = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(&pending, Uuid::new_v4(), forged, None),
    )
    .await;
    assert_eq!(forged["error"]["code"], "context_changed");
    let mut foreign = valid_update.clone();
    foreign["operation_id"] = json!(Uuid::new_v4());
    let foreign = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(&pending, Uuid::new_v4(), foreign, None),
    )
    .await;
    assert_eq!(foreign["error"]["code"], "invalid_arguments");

    let invalid_source = json!([{"kind":"pipeline_output","output":{
        "run_id":Uuid::new_v4(),"output_id":Uuid::new_v4(),"digest":"missing-output-digest",
        "evidence_kind":"declaration","evidence_scope":"Exact missing fixture output."}}]);
    let invalid = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(
            &pending,
            Uuid::new_v4(),
            valid_update.clone(),
            Some(invalid_source),
        ),
    )
    .await;
    assert_eq!(invalid["error"]["code"], "invalid_source");
    let unchanged = query_current(&mut client, &pending_ctx["change_id"]).await;
    assert_eq!(
        context(&unchanged)["run"]["revision"],
        pending_ctx["run"]["revision"]
    );
    assert_eq!(
        context(&unchanged)["origin"]["operations"][0]["expected_revision"],
        1
    );
    assert_eq!(context(&unchanged)["origin"]["source_revision"], 0);

    let amended = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(&pending, Uuid::new_v4(), valid_update, None),
    )
    .await;
    assert_eq!(
        context(&amended)["run"]["current_phase_id"],
        "kc-resolve-baseline"
    );
    assert_eq!(context(&amended)["run"]["delivery_mode"], "phasewise");
    assert_eq!(
        context(&amended)["origin"]["operations"][0]["expected_revision"],
        2
    );
    assert_eq!(
        context(&amended)["origin"]["operation_hints"][0]["expected_revision"],
        1
    );
    assert_eq!(
        context(&amended)["inputs"][0]["applied_basis_amendment"]["target_updates"][0]["replacement_guard"],
        guard(&exact)
    );
    let old_seal = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        old_commit,
    )
    .await;
    assert_eq!(old_seal["error"]["code"], "stale_revision");

    let republished =
        ready_single_from_baseline(&mut client, revise_spec(unit.clone(), 2, intended), amended)
            .await;
    let committed = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&republished["actions"][0]).clone(),
    )
    .await;
    assert_eq!(committed["applied"]["applied_operations"][0]["revision"], 3);
    let committed_current = query_current(&mut client, &committed["applied"]["change_id"]).await;
    let postcommit = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        amendment_request(
            &committed_current,
            Uuid::new_v4(),
            json!({
            "operation_id":operation_id,"previous_expected_revision":2,
            "previous_expected_lifecycle":"active","replacement_guard":guard(&exact)}),
            None,
        ),
    )
    .await;
    assert_eq!(postcommit["error"]["code"], "forbidden");

    let producer_run = Uuid::parse_str(created.receipt["run_id"].as_str().unwrap()).unwrap();
    let (source_output_id, source_output_digest): (Uuid, String) = sqlx::query_as(
        "SELECT o.id,o.digest FROM knowledge_change_outputs o \
         JOIN knowledge_change_output_bindings b ON b.tenant_id=o.tenant_id \
         AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id \
         WHERE o.run_id=$1 AND o.phase_id='kc-review-reconcile' AND NOT b.stale",
    )
    .bind(producer_run)
    .fetch_one(&pool)
    .await
    .unwrap();
    let source_a = json!({"kind":"pipeline_output","output":{
        "run_id":producer_run,"output_id":source_output_id,"digest":source_output_digest,
        "evidence_kind":"declaration",
        "evidence_scope":"Exact reviewed output from the fixture owner's earlier Knowledge Change."}});
    let mut source_based = original.clone();
    source_based["title"] = json!("Owner-qualified declaration with replaceable exact evidence");
    source_based["canonical_text"] = json!(
        "This revision is published only after its stale exact source is explicitly replaced on the same cursor."
    );
    source_based["sources"] = json!([source_a.clone()]);
    let source_pending = ready_single(
        &mut client,
        revise_spec(unit.clone(), 3, source_based.clone()),
    )
    .await;
    let stale_source_commit = action_params(&source_pending["actions"][0]).clone();
    sqlx::query(
        "UPDATE knowledge_change_output_bindings SET stale=true, \
         stale_reason='superseded-by-test-source-epoch',updated_at=pg_catalog.clock_timestamp() \
         WHERE run_id=$1 AND output_id=$2",
    )
    .bind(producer_run)
    .bind(source_output_id)
    .execute(&pool)
    .await
    .unwrap();
    let stale_source = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        stale_source_commit.clone(),
    )
    .await;
    assert_eq!(stale_source["error"]["code"], "invalid_source");

    let source_b = json!({"kind":"snapshot","snapshot":{
        "title":"Fixture-owner replacement evidence",
        "uri":"urn:tect:dk2:source:fixture-owner:replacement-epoch-1",
        "text":"The fixture owner supplies a new exact declaration after the prior output became stale.",
        "evidence_kind":"declaration"}});
    let source_ctx = context(&source_pending);
    let amendment_id = Uuid::new_v4();
    let source_amendment = json!({
        "request_id":amendment_id,"change_id":source_ctx["change_id"],
        "run_id":source_ctx["run"]["id"],"run_revision":source_ctx["run"]["revision"],
        "revisit_phase_id":"kc-resolve-baseline",
        "reason":"Replace an exact source that became stale after review.",
        "input":"The owner explicitly supplies and requalifies the complete replacement source set.",
        "basis_amendment":{"target_updates":[],"replacement_sources":[source_b.clone()]}
    });
    let amended_source = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        source_amendment.clone(),
    )
    .await;
    let amended_source_ctx = context(&amended_source);
    assert_eq!(amended_source_ctx["origin"]["source_revision"], 1);
    let recorded = &amended_source_ctx["inputs"][0]["applied_basis_amendment"]["source_change"];
    assert_eq!(recorded["previous_source_revision"], 0);
    assert_eq!(recorded["replacement_source_revision"], 1);
    assert_eq!(recorded["previous_sources"], json!([source_a]));
    assert_eq!(recorded["replacement_sources"], json!([source_b.clone()]));
    assert!(
        recorded["previous_pins"][0]["source_iri"]
            .as_str()
            .unwrap()
            .ends_with(":0")
    );
    assert!(
        recorded["replacement_pins"][0]["source_iri"]
            .as_str()
            .unwrap()
            .ends_with(":set:1:0")
    );
    let replay = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        source_amendment.clone(),
    )
    .await;
    assert_eq!(context(&replay)["origin"]["source_revision"], 1);
    assert_eq!(context(&replay)["inputs"].as_array().unwrap().len(), 1);
    let mut conflict = source_amendment;
    conflict["reason"] = json!("A different payload must not alias the saved amendment.");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_record_input",
            conflict
        )
        .await["error"]["code"],
        "input_conflict"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_commit",
            stale_source_commit
        )
        .await["error"]["code"],
        "stale_revision"
    );

    source_based["sources"] = json!([source_b.clone()]);
    let requalified = ready_single_from_baseline(
        &mut client,
        revise_spec(unit.clone(), 3, source_based),
        amended_source,
    )
    .await;
    let source_committed = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&requalified["actions"][0]).clone(),
    )
    .await;
    assert_eq!(
        source_committed["applied"]["applied_operations"][0]["revision"],
        4
    );
    let source_change = Uuid::parse_str(source_ctx["change_id"].as_str().unwrap()).unwrap();
    settle_and_finish_receipt(&mut client, &source_committed["applied"]).await;
    let exact_source_revision = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit,"revision":4}),
    )
    .await;
    assert_eq!(
        exact_source_revision["document"]["document"]["sources"],
        json!([source_b])
    );
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(unit),
            expected_revision: Some(4),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "owned_live_copies",
            authored_followup: false,
        },
    )
    .await;
    settle_and_finish_receipt(&mut client, &erased["applied_erased"]).await;
    let scrubbed_input: (i64, bool) = sqlx::query_as(
        "SELECT count(*),bool_and(i.payload_erased AND i.input IS NULL AND i.digest IS NULL \
         AND i.reason IS NULL AND i.applied_basis_amendment IS NULL) \
         FROM knowledge_change_inputs i JOIN knowledge_change_runs r ON r.id=i.run_id \
         WHERE r.change_id=$1",
    )
    .bind(source_change)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(scrubbed_input, (1, true));
    let scrubbed_receipts: (i64, bool) = sqlx::query_as(
        "SELECT count(*),bool_and(payload_erased AND request_payload IS NULL \
         AND result_payload IS NULL) FROM knowledge_lifecycle_command_receipts \
         WHERE erased_change_id=$1",
    )
    .bind(source_change)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(scrubbed_receipts.0 > 0 && scrubbed_receipts.1);
    client.finish().await;
}
