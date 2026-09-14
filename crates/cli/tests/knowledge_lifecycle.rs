#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
mod knowledge_operation_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    commit_create, context, method_reads, omit_nulls, settle_and_finish, settle_and_finish_receipt,
};
use knowledge_operation_support::{SingleOperation, commit_pair_erase, commit_single};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test]
async fn dk2_identity_qualification_is_exact_and_repeatable() {
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
    let first = tect_postgres::current_knowledge_database_identity(&pool)
        .await
        .unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let stored: (String, i64, bool) = sqlx::query_as(
        "SELECT qualified_system_identifier,qualified_database_oid::bigint, \
         tect_dk_database_identity_ready() FROM durable_knowledge_capability WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.0, first.system_identifier);
    assert_eq!(stored.1, i64::from(first.database_oid));
    assert!(stored.2);
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT tect_dk_database_identity_ready()")
            .fetch_one(&runtime)
            .await
            .unwrap()
    );
    let internal_execute: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_function_privilege($1, \
         'public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean)', \
         'EXECUTE')",
    )
    .bind(&role)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!internal_execute);
}

#[tokio::test]
async fn dk2_create_reaches_native_exact_read_and_terminal_result() {
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
    let socket = root.join("dk2.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk2-{}", Uuid::new_v4()));
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
        &format!("dk2-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    assert_eq!(committed.exact["document"]["document"], fixture["document"]);
    let finished = settle_and_finish(&mut client, &committed).await;
    assert_eq!(context(&finished)["run"]["status"], "completed");
    client.finish().await;
}

#[tokio::test]
async fn dk2_all_canonical_operations_reach_native_commit() {
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
    let socket = root.join("dk2-operations.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk2-ops-{}", Uuid::new_v4()));
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
        &format!("dk2-ops-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let original = fixture["document"].clone();
    let first = commit_create(&mut client, original.clone()).await;
    let unit = first.receipt["applied_operations"][0]["unit_id"].clone();
    let first_finished = settle_and_finish(&mut client, &first).await;
    assert_eq!(context(&first_finished)["run"]["status"], "completed");
    let post_commit_rewind = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({"request_id":Uuid::new_v4(),"change_id":first.receipt["change_id"],
            "run_id":first.receipt["run_id"],"run_revision":context(&first_finished)["run"]["revision"],
            "revisit_phase_id":"kc-review-reconcile",
            "reason":"A committed semantic run must remain immutable.",
            "input":"Attempt to reopen a completed publisher run."}),
    )
    .await;
    assert_eq!(
        post_commit_rewind["error"]["code"], "forbidden",
        "record_input must not rewind a run after its publisher receipt exists"
    );

    let mut revised = original.clone();
    revised["title"] = json!("Fixture-owner identity applicability constraint");
    revised["canonical_text"] = json!(
        "Within this isolated test workspace, the enrolled host credential and its bound native session UUID are required for fixture operations."
    );
    revised["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:fixture-owner:identity-revision-2");
    revised["sources"][0]["snapshot"]["text"] = json!(
        "The isolated fixture owner narrows this revision to its own workspace and does not assert an upstream source change."
    );
    let revise = commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revised.clone()),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: revised["sources"].clone(),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: true,
        },
    )
    .await;
    assert_eq!(revise["applied"]["applied_operations"][0]["revision"], 2);
    let revise_finished = settle_and_finish_receipt(&mut client, &revise["applied"]).await;
    assert_eq!(context(&revise_finished)["run"]["status"], "completed");

    let revised_exact = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit.clone(),"revision":2}),
    )
    .await;
    let observed_at:String=sqlx::query_scalar("SELECT pg_catalog.to_char(pg_catalog.clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')")
        .fetch_one(&pool).await.unwrap();
    let observation_text = format!(
        "Fresh native exact read observed unit {} revision {} with RDF digest {}; this records publication/read only.",
        revised_exact["document"]["unit_id"].as_str().unwrap(),
        revised_exact["document"]["revision"].as_i64().unwrap(),
        revised_exact["document"]["rdf_digest"].as_str().unwrap()
    );

    let validation_source = json!({"kind":"snapshot","snapshot":{
        "title":"Observed current native publication read","uri":"urn:tect:dk2:source:runtime:publication-read-20260914",
        "text":observation_text,"observed_at":observed_at,"evidence_kind":"runtime_verification"}});
    let revalidate=commit_single(&mut client,SingleOperation{
        operation:"revalidate",unit_id:Some(unit.clone()),expected_revision:Some(2),expected_lifecycle:Some("active"),
        document:None,revalidation:Some(json!({"sources":[validation_source.clone()],
            "evidence_basis":"Fresh publication/read observation supporting the unchanged fixture declaration; not proof of negative authentication enforcement.",
            "valid_until":"2030-09-14T09:00:00Z","review_due_at":"2027-09-14T09:00:00Z"})),
        successor:None,replacement_bindings:json!([]),sources:json!([validation_source]),
        knowledge_kind:json!("constraint"),profiles:json!(["general"]),erasure:"not_required",authored_followup:false,
    }).await;
    assert_eq!(
        revalidate["applied"]["applied_operations"][0]["revision"],
        2
    );
    let revalidate_finished = settle_and_finish_receipt(&mut client, &revalidate["applied"]).await;
    assert_eq!(context(&revalidate_finished)["run"]["status"], "completed");

    let mut successor_document = original.clone();
    successor_document["title"] = json!("Fixture-owner successor identity constraint");
    successor_document["canonical_text"] = json!(
        "The isolated fixture successor defines identity requirements for its distinct successor scope."
    );
    successor_document["conditions"] =
        json!(["Applies only to the distinct fixture-owner successor scope."]);
    successor_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:fixture-owner:successor");
    successor_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner declares a distinct successor scope for supersession testing.");
    let second = commit_create(&mut client, successor_document).await;
    let successor = second.receipt["applied_operations"][0]["unit_id"].clone();
    let second_finished = settle_and_finish(&mut client, &second).await;
    assert_eq!(context(&second_finished)["run"]["status"], "completed");
    let supersede = commit_single(
        &mut client,
        SingleOperation {
            operation: "supersede",
            unit_id: Some(unit.clone()),
            expected_revision: Some(2),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: Some(json!({"unit_id":successor})),
            replacement_bindings: revised["bindings"].clone(),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        supersede["applied"]["applied_operations"][0]["operation"],
        "supersede"
    );
    let supersede_finished = settle_and_finish_receipt(&mut client, &supersede["applied"]).await;
    assert_eq!(context(&supersede_finished)["run"]["status"], "completed");

    let retract = commit_single(
        &mut client,
        SingleOperation {
            operation: "retract",
            unit_id: Some(successor.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        retract["applied"]["applied_operations"][0]["operation"],
        "retract"
    );
    let retract_finished = settle_and_finish_receipt(&mut client, &retract["applied"]).await;
    assert_eq!(context(&retract_finished)["run"]["status"], "completed");

    let erased_targets = [
        (unit.clone(), 2, "superseded"),
        (successor.clone(), 1, "retracted"),
    ];
    let erase = commit_pair_erase(&mut client, erased_targets.clone()).await;
    let erased_operations = erase["applied_erased"]["operations"].as_array().unwrap();
    assert_eq!(erased_operations.len(), 2);
    assert!(
        erased_operations
            .iter()
            .all(|operation| operation["state"] == "payload_erased")
    );
    let mut sequences = erased_operations
        .iter()
        .map(|operation| operation["receipt"]["erasure_sequence"].as_i64().unwrap())
        .collect::<Vec<_>>();
    sequences.sort_unstable();
    assert_eq!(sequences[1], sequences[0] + 1);
    let erase_finished = settle_and_finish_receipt(&mut client, &erase["applied_erased"]).await;
    let erased_context = context(&erase_finished);
    assert_eq!(erased_context["run"]["status"], "completed");
    assert_eq!(erased_context["result"]["canonical"], "applied");
    let change_id =
        Uuid::parse_str(erase["applied_erased"]["change_id"].as_str().unwrap()).unwrap();
    let stored: (bool, bool, bool, bool, bool, bool, String, bool) = sqlx::query_as(
        "SELECT publisher_receipt IS NULL,erased_publisher_receipt IS NOT NULL, \
         effects_report IS NULL,erased_effects_report IS NOT NULL, \
         result IS NULL,erased_result IS NOT NULL,status,payload_erased \
         FROM knowledge_change_runs WHERE change_id=$1",
    )
    .bind(change_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (true, true, true, true, true, true, "completed".into(), true)
    );

    let target_ids = erased_targets
        .iter()
        .map(|(unit, _, _)| Uuid::parse_str(unit.as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let before: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_publication_events WHERE unit_id=ANY($1)), \
         (SELECT count(*) FROM knowledge_suppression_ledger WHERE unit_id=ANY($1))",
    )
    .bind(&target_ids)
    .fetch_one(&pool)
    .await
    .unwrap();
    let opaque_request_id = Uuid::new_v4();
    let opaque_request = json!({
        "request_id":opaque_request_id,
        "intent":"Confirm that two exact erased targets already satisfy the requested erasure scope.",
        "desired_outcome":"Record an opaque no-change terminal result without reviving erased payloads.",
        "sources":[],
        "operation_hints":erased_targets.iter().enumerate().map(|(index,(unit,revision,_))| json!({
            "client_label":format!("already-erased-{index}"),"operation":"erase","unit_id":unit,
            "expected_revision":revision,"expected_lifecycle":"erased",
            "reason":"Confirm the existing opaque suppression state for this exact unit.",
            "authority_basis":"Current authenticated workspace owner.","depends_on_labels":[]
        })).collect::<Vec<_>>(),
        "owner":{"kind":"workspace"},
        "completion":{"canonical_result":true,"exact_delivery":true,"impact_recorded":true,
            "search":"not_required","erasure":"owned_live_copies"},
        "delivery_mode":"whole"
    });
    let opaque = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        opaque_request.clone(),
    )
    .await;
    let opaque_context = context(&opaque);
    assert!(opaque_context["origin"].is_null());
    assert_eq!(opaque_context["run"]["delivery_mode"], "phasewise");
    assert_eq!(
        opaque_context["run"]["current_phase_id"],
        "kc-result-handoff"
    );
    assert_eq!(
        opaque_context["erased_no_change_proof"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_begin",
            opaque_request
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    let opaque_rewind = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({
        "request_id":Uuid::new_v4(),"change_id":opaque_context["change_id"],
        "run_id":opaque_context["run"]["id"],"run_revision":opaque_context["run"]["revision"],
        "revisit_phase_id":"kc-resolve-baseline","reason":"Erased payload cannot be reopened.",
        "input":"Attempt to reintroduce semantic input into an opaque run."}),
    )
    .await;
    assert_eq!(opaque_rewind["error"]["code"], "forbidden");
    let action = &opaque["actions"][0];
    let mut terminal = recovery_support::action_params(action).clone();
    terminal["output"]["method_reads"] = method_reads(action);
    terminal["output"]["body"] =
        json!("Backend-qualified opaque no-change handoff for already-erased targets.");
    terminal["output"]["data"] = json!({"phase":"kc-result-handoff","data":{
        "canonical":"no_change","user_outcome":"achieved",
        "summary":"The requested owned-copy erasure scope was already satisfied.",
        "remaining_work":[],"effects":[]}});
    terminal["output"]["verdict"] = json!("complete");
    terminal["output"]["outcome"] = json!("completed");
    terminal["output"]["transition"] = json!("complete");
    terminal["output"]["findings"] = json!([]);
    terminal["output"]["dispositions"] = json!([]);
    omit_nulls(&mut terminal);
    let terminal_replay = terminal.clone();
    let opaque_finished = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        terminal,
    )
    .await;
    let opaque_finished_context = context(&opaque_finished);
    assert_eq!(opaque_finished_context["run"]["status"], "completed");
    assert_eq!(opaque_finished_context["result"]["canonical"], "no_change");
    assert_eq!(
        opaque_finished_context["result"]["user_outcome"],
        "achieved"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_phase_complete",
            terminal_replay
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    let opaque_change =
        Uuid::parse_str(opaque_finished_context["change_id"].as_str().unwrap()).unwrap();
    let persisted: (bool, bool, bool, bool, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT c.payload_erased AND c.intent IS NULL AND c.sources IS NULL \
            AND c.operation_hints IS NULL AND c.completion IS NULL, \
         r.payload_erased AND r.erased_no_change_proof IS NOT NULL \
            AND r.result IS NULL AND r.erased_result IS NOT NULL, \
         bool_and(o.payload_erased AND o.output IS NULL AND o.digest IS NULL), \
         bool_and(a.payload_erased AND a.output_digest IS NULL), \
         count(DISTINCT o.id),count(DISTINCT a.id), \
         (SELECT count(*) FROM knowledge_lifecycle_command_receipts q \
          WHERE q.erased_change_id=c.id AND q.payload_erased \
          AND q.request_payload IS NULL AND q.result_payload IS NULL), \
         (SELECT count(*) FROM knowledge_change_inputs i WHERE i.run_id=r.id) \
         FROM knowledge_lifecycle_changes c JOIN knowledge_change_runs r ON r.change_id=c.id \
         JOIN knowledge_change_outputs o ON o.run_id=r.id \
         JOIN knowledge_change_attempts a ON a.run_id=r.id \
         WHERE c.id=$1 GROUP BY c.id,r.id",
    )
    .bind(opaque_change)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, (true, true, true, true, 1, 1, 2, 0));
    let after: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_publication_events WHERE unit_id=ANY($1)), \
         (SELECT count(*) FROM knowledge_suppression_ledger WHERE unit_id=ANY($1))",
    )
    .bind(&target_ids)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after, before,
        "opaque no-change must not republish or re-erase"
    );
    client.finish().await;
}
