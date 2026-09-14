#[path = "pipeline_execution/knowledge_compound_support.rs"]
#[allow(dead_code)]
mod knowledge_compound_support;
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

use knowledge_compound_support::ready_create_then_supersede;
use knowledge_lifecycle_support::{commit_create, settle_and_finish, settle_and_finish_receipt};
use knowledge_operation_support::{SingleOperation, commit_single, ready_pair_erase};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test]
async fn compound_commit_rolls_back_when_one_reviewed_target_changes() {
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
    let socket = root.join("dk2-atomicity.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk2-atomicity-{}", Uuid::new_v4()),
    );
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
        &format!("dk2-atomicity-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let first_document = fixture["document"].clone();
    let first = commit_create(&mut client, first_document).await;
    settle_and_finish(&mut client, &first).await;
    let first_unit = first.receipt["applied_operations"][0]["unit_id"].clone();
    let mut second_document = fixture["document"].clone();
    second_document["title"] = json!("Independent compound rollback target");
    second_document["canonical_text"] =
        json!("This independent fixture target changes after compound review.");
    second_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:atomicity:second");
    second_document["sources"][0]["snapshot"]["text"] =
        json!("The test owner declares a distinct second rollback target.");
    let second = commit_create(&mut client, second_document.clone()).await;
    settle_and_finish(&mut client, &second).await;
    let second_unit = second.receipt["applied_operations"][0]["unit_id"].clone();

    let ready = ready_pair_erase(
        &mut client,
        [
            (first_unit.clone(), 1, "active"),
            (second_unit.clone(), 1, "active"),
        ],
    )
    .await;
    let mut revised = second_document.clone();
    revised["title"] = json!("Changed compound rollback target");
    revised["canonical_text"] =
        json!("The second target changed after the compound erasure review and seal.");
    revised["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:atomicity:second-revised");
    revised["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner records a post-review target revision.");
    let revised_sources = revised["sources"].clone();
    let changed = commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(second_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revised),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: revised_sources,
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    settle_and_finish_receipt(&mut client, &changed["applied"]).await;
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&ready["actions"][0]).clone(),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "stale_context",
        "a target change after review must refuse the whole compound commit"
    );
    let first_exact = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":first_unit,"revision":1}),
    )
    .await;
    assert_eq!(
        first_exact["document"]["lifecycle"], "active",
        "the first operation must roll back when the second target guard is stale"
    );
    let first_suppressed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_ledger WHERE unit_id=$1)",
    )
    .bind(Uuid::parse_str(first_exact["document"]["unit_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !first_suppressed,
        "a refused compound commit must not leave a suppression ledger entry"
    );
    let second_exact = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":second_unit,"revision":2}),
    )
    .await;
    assert_eq!(second_exact["document"]["lifecycle"], "active");

    let mut successor_document = fixture["document"].clone();
    successor_document["title"] = json!("Same-Change ordered successor");
    successor_document["canonical_text"] = json!(
        "This distinct successor becomes current only after its same-Change create operation."
    );
    successor_document["conditions"] =
        json!(["Applies only to the same-Change successor ordering fixture."]);
    successor_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:atomicity:same-change-successor");
    successor_document["sources"][0]["snapshot"]["text"] =
        json!("The test owner declares a distinct same-Change successor.");
    let ready_compound = ready_create_then_supersede(
        &mut client,
        first_unit.clone(),
        1,
        successor_document.clone(),
        fixture["document"]["bindings"].clone(),
    )
    .await;
    let ready_context = knowledge_lifecycle_support::context(&ready_compound);
    let successor_operation = ready_context["origin"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["client_label"] == "successor")
        .unwrap();
    let planned_successor =
        Uuid::parse_str(successor_operation["unit_id"].as_str().unwrap()).unwrap();
    let compound_change_id = Uuid::parse_str(ready_context["change_id"].as_str().unwrap()).unwrap();
    let compound_run_id = Uuid::parse_str(ready_context["run"]["id"].as_str().unwrap()).unwrap();
    let ready_revision = ready_context["run"]["revision"].as_i64().unwrap();
    let compound_commit = action_params(&ready_compound["actions"][0]).clone();
    sqlx::query("DROP TRIGGER IF EXISTS tect_test_reject_supersession ON knowledge_supersessions")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE OR REPLACE FUNCTION public.tect_test_reject_supersession() RETURNS trigger \
        LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'bounded compound rollback injection'; END $$",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER tect_test_reject_supersession BEFORE INSERT ON knowledge_supersessions \
        FOR EACH ROW EXECUTE FUNCTION public.tect_test_reject_supersession()",
    )
    .execute(&pool)
    .await
    .unwrap();
    let injected = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        compound_commit.clone(),
    )
    .await;
    assert_eq!(
        injected["error"]["code"], "storage_unavailable",
        "the bounded test trigger must fail after the first canonical operation begins"
    );
    let rolled_back:(bool,i64,bool,i64,String)=sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads WHERE unit_id=$1), \
         (SELECT count(*) FROM knowledge_publication_events WHERE lifecycle_change_id=$2), \
         publisher_receipt IS NULL,(SELECT count(*) FROM knowledge_lifecycle_effects WHERE change_id=$2), \
         current_phase_id FROM knowledge_change_runs WHERE id=$3 AND revision=$4")
        .bind(planned_successor).bind(compound_change_id).bind(compound_run_id).bind(ready_revision)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        rolled_back,
        (false, 0, true, 0, "kc-commit".into()),
        "the first mutation, event, receipt and effects must all roll back with the second operation"
    );
    sqlx::query("DROP TRIGGER tect_test_reject_supersession ON knowledge_supersessions")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION public.tect_test_reject_supersession()")
        .execute(&pool)
        .await
        .unwrap();
    let compound = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        compound_commit,
    )
    .await;
    let compound_receipt = &compound["applied"];
    assert_eq!(
        compound_receipt["applied_operations"][0]["operation"],
        "create"
    );
    assert_eq!(
        compound_receipt["applied_operations"][1]["operation"],
        "supersede"
    );
    let successor_unit = compound_receipt["applied_operations"][0]["unit_id"].clone();
    let compound_change = compound_receipt["change_id"].clone();
    let compound_commit_request: Value = sqlx::query_scalar(
        "SELECT request_payload FROM knowledge_lifecycle_command_receipts \
         WHERE (request_payload->>'change_id')::uuid=$1 AND operation='commit'",
    )
    .bind(Uuid::parse_str(compound_change.as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    settle_and_finish_receipt(&mut client, compound_receipt).await;
    let predecessor = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":first_unit,"revision":1}),
    )
    .await;
    assert_eq!(predecessor["document"]["lifecycle"], "superseded");
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(first_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("superseded"),
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
    let survivor = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":successor_unit,"revision":1}),
    )
    .await;
    assert_eq!(survivor["document"]["document"], successor_document);
    let compound_run: (bool, bool, Value) = sqlx::query_as(
        "SELECT publisher_receipt IS NULL,erased_publisher_receipt IS NOT NULL, \
         erased_publisher_receipt FROM knowledge_change_runs WHERE change_id=$1",
    )
    .bind(Uuid::parse_str(compound_change.as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(compound_run.0 && compound_run.1);
    assert!(
        compound_run.2["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| {
                operation["state"] == "intact" && operation["receipt"]["unit_id"] == successor_unit
            })
    );
    assert!(
        compound_run.2["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| {
                operation["state"] == "payload_erased"
                    && operation["receipt"]["unit_id"] == first_unit
            })
    );
    let erased_replay = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        compound_commit_request,
    )
    .await;
    assert_eq!(
        erased_replay["error"]["code"], "knowledge_payload_erased",
        "the redacted whole-Change receipt must not replay as intact producer proof"
    );
    client.finish().await;
}
