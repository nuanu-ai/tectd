//! Real tectd-mcp -> tectd -> PostgreSQL acceptance for Engineering Matrix task sources.
//! This test migrates and writes fixtures. Run it only against a disposable PG18 database.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::route;
use tect_postgres::admin;
use uuid::Uuid;

fn source_input(mode: &str, criticality: &str) -> Value {
    let absent = json!({"state":"absent"});
    json!({
        "mode":{"state":"known","value":mode,"provenance":"owner task brief"},
        "envelope":{"scale":absent,"operational_facts":{"state":"absent"}},
        "criticality":{"state":"known","value":criticality,"provenance":"owner task brief"},
        "intent":absent,"urgency":absent,"promised_behavior":absent,
        "promised_proof":absent,"affected_guarantees":absent,
        "actual_exposure":absent,"demand_commitment":absent,
        "latency_commitment":absent,"urgent_repair":absent
    })
}

fn record(task_id: Uuid, revision: i64, request_id: Uuid, input: Value) -> Value {
    json!({
        "task_id":task_id,"revision":revision,
        "expected_current_revision":revision - 1,
        "request_id":request_id,"input":input
    })
}

fn assert_receipt(receipt: &Value, task_id: Uuid, revision: i64, request_id: Uuid, input: &Value) {
    assert_eq!(receipt["task_id"], json!(task_id));
    assert_eq!(receipt["revision"], revision);
    assert_eq!(receipt["request_id"], json!(request_id));
    assert_eq!(&receipt["input"], input);
    let digest = receipt["input_digest"].as_str().expect("source digest");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(Uuid::parse_str(receipt["recorded_by_principal_id"].as_str().unwrap()).is_ok());
    assert!(Uuid::parse_str(receipt["recorded_by_session_id"].as_str().unwrap()).is_ok());
}

/// Refuse to migrate or enroll unless both URLs resolve to the same dedicated
/// disposable PG18 database and the runtime URL authenticates as the test role.
async fn disposable_pg18_pair(admin_url: &str, runtime_url: &str, role: &str) -> PgPool {
    let admin_pool = PgPool::connect(admin_url).await.unwrap();
    let label = format!("task-source-preflight-{}", Uuid::new_v4());
    let runtime_pool = PgPool::connect(&tagged_url(runtime_url, &label))
        .await
        .unwrap();
    let mut runtime = runtime_pool.acquire().await.unwrap();
    let (admin_database, admin_oid, admin_version, admin_system): (String, i64, i32, String) =
        sqlx::query_as(
            "SELECT current_database(),oid::bigint,current_setting('server_version_num')::integer, \
             (SELECT system_identifier::text FROM pg_catalog.pg_control_system()) \
             FROM pg_catalog.pg_database WHERE datname=current_database()",
        )
        .fetch_one(&admin_pool)
        .await
        .unwrap();
    let (runtime_database, runtime_oid, runtime_user, runtime_version, runtime_pid): (
        String,
        i64,
        String,
        i32,
        i32,
    ) = sqlx::query_as(
        "SELECT current_database(),oid::bigint,current_user, \
         current_setting('server_version_num')::integer,pg_backend_pid() \
         FROM pg_catalog.pg_database WHERE datname=current_database()",
    )
    .fetch_one(&mut *runtime)
    .await
    .unwrap();
    assert_eq!(
        admin_database, "tect_test",
        "admin URL must target tect_test"
    );
    assert_eq!(
        runtime_database, "tect_test",
        "runtime URL must target tect_test"
    );
    assert_eq!(admin_version / 10_000, 18, "admin URL must target PG18");
    assert_eq!(runtime_version / 10_000, 18, "runtime URL must target PG18");
    assert_eq!(
        runtime_user, role,
        "runtime URL must use the test runtime role"
    );
    assert!(
        admin_system.parse::<u64>().is_ok_and(|value| value != 0),
        "admin URL must expose a valid cluster system identifier"
    );
    assert_eq!(admin_oid, runtime_oid, "database OIDs must match");
    let same_backend: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_stat_activity a \
         JOIN pg_catalog.pg_roles r ON r.oid=a.usesysid \
         WHERE a.pid=$1 AND a.datid::bigint=$2 AND a.application_name=$3 \
         AND r.rolname=$4)",
    )
    .bind(runtime_pid)
    .bind(admin_oid)
    .bind(&label)
    .bind(role)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!(
        same_backend,
        "admin and runtime URLs must reach the same PG instance"
    );
    admin_pool
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "MIGRATES AND WRITES FIXTURES; requires disposable PostgreSQL 18 and TECT_TEST_DISPOSABLE_PG=1 plus TECT_TEST_* URLs/role"]
async fn task_source_revisions_replay_conflict_staleness_and_workspace_isolation() {
    assert_eq!(
        std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(),
        Ok("1"),
        "explicit disposable-PostgreSQL opt-in required before connecting"
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = disposable_pg18_pair(&admin_url, &runtime_url, &role).await;
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("task-source.sock");
    let runtime = tagged_url(&runtime_url, &format!("task-source-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);

    let owner_key = format!("matrix-owner-{}", Uuid::new_v4());
    let mut owner = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &owner_key).await;
    let opened = owner.call("open_workspace", json!({})).await;
    let owner_session_id = opened["session"]["id"].clone();
    let task_id = Uuid::new_v4();
    let rev1_request_id = Uuid::new_v4();
    let rev1_input = source_input("mvp", "Customer-facing preview");
    let rev1_params = record(task_id, 1, rev1_request_id, rev1_input.clone());
    let rev1 = route(
        &mut owner,
        "command",
        "task.source.record",
        rev1_params.clone(),
    )
    .await;
    assert_receipt(&rev1, task_id, 1, rev1_request_id, &rev1_input);
    assert_eq!(rev1["recorded_by_session_id"], owner_session_id);
    let current1 = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(
        current1, rev1,
        "first current read must return the recorded revision"
    );

    let rev2_request_id = Uuid::new_v4();
    let rev2_input = source_input("production", "Payment-facing preview");
    let rev2 = route(
        &mut owner,
        "command",
        "task.source.record",
        record(task_id, 2, rev2_request_id, rev2_input.clone()),
    )
    .await;
    assert_receipt(&rev2, task_id, 2, rev2_request_id, &rev2_input);
    assert_ne!(rev2["input_digest"], rev1["input_digest"]);
    let current2 = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(current2, rev2, "current read must advance to revision 2");

    let card_summary = route(
        &mut owner,
        "query",
        "scope.advisory.card",
        json!({"task_id":task_id,"expected_task_revision":2}),
    )
    .await;
    assert_eq!(card_summary["task_id"], task_id.to_string());
    assert_eq!(card_summary["task_revision"], "2");
    assert_eq!(card_summary["catalogue_version"], "EM02-INITIAL@0.1");
    assert_eq!(
        card_summary["source_verification_status"],
        "owner_reported_pending_independent_verification"
    );
    assert_eq!(card_summary["resolved"], false);
    assert!(card_summary["selected_card"].is_null());
    let scope_summary = card_summary["mandatory_cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|card| card["id"] == "EM02-SCOPE@0.1")
        .expect("scope card in summary catalogue");
    assert!(
        scope_summary["summary"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    assert!(scope_summary.get("body").is_none());

    let card_full = route(
        &mut owner,
        "query",
        "scope.advisory.card",
        json!({
            "task_id":task_id,"expected_task_revision":2,
            "detail":"full","card_id":"EM02-SCOPE@0.1"
        }),
    )
    .await;
    for field in [
        "catalogue_version",
        "task_id",
        "task_revision",
        "source_verification_status",
        "resolved",
        "mandatory_cards",
        "unresolved_evidence",
    ] {
        assert_eq!(
            card_full[field], card_summary[field],
            "full detail changed {field}"
        );
    }
    assert_eq!(card_full["selected_card"]["id"], "EM02-SCOPE@0.1");
    assert_eq!(
        card_full["selected_card"]["summary"],
        scope_summary["summary"]
    );
    assert!(
        card_full["selected_card"]["body"]
            .as_str()
            .is_some_and(|body| !body.is_empty())
    );

    let stale_card = owner
        .call_error(
            "query",
            json!({"route":"scope.advisory.card","params":{
                "task_id":task_id,"expected_task_revision":1
            }}),
        )
        .await;
    assert_eq!(stale_card["error"]["code"], "stale_revision");

    let replay = route(
        &mut owner,
        "command",
        "task.source.record",
        rev1_params.clone(),
    )
    .await;
    assert_eq!(
        replay, rev1,
        "exact revision 1 replay must return its original receipt"
    );
    let mut changed_replay = rev1_params;
    changed_replay["input"]["criticality"]["value"] = json!("Changed after receipt");
    let conflict = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":changed_replay}),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "input_conflict");

    let stale = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":record(task_id, 2, Uuid::new_v4(), rev2_input)}),
        )
        .await;
    assert_eq!(stale["error"]["code"], "stale_revision");
    let after_refusals = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(after_refusals, rev2);

    let rev3_input = source_input("production", "Payment-facing preview");
    let choice_set = json!({
        "schema":"tect.matrix-choice-set/1",
        "choice_set_id":"preview-implementation-options",
        "version":1,
        "task_id":task_id.to_string(),
        "task_revision":"3",
        "decision_question":"Which approach should address the payment-facing preview?",
        "candidates":[
            {
                "candidate_id":"adapt-existing-preview",
                "title":"Adapt existing preview",
                "approach":"Adapt the current preview path to use the recorded payment state.",
                "assumption_fact_ids":["criticality"]
            },
            {
                "candidate_id":"separate-payment-preview",
                "title":"Separate payment preview",
                "approach":"Build a separate payment preview path with an explicit mode boundary.",
                "assumption_fact_ids":["mode"]
            }
        ]
    });
    let rev3_request_id = Uuid::new_v4();
    let mut rev3_params = record(task_id, 3, rev3_request_id, rev3_input.clone());
    rev3_params["choice_set"] = choice_set.clone();
    let rev3 = route(
        &mut owner,
        "command",
        "task.source.record",
        rev3_params.clone(),
    )
    .await;
    assert_receipt(&rev3, task_id, 3, rev3_request_id, &rev3_input);
    assert_eq!(rev3["choice_set"], choice_set);
    let choice_digest = rev3["choice_set_digest"]
        .as_str()
        .expect("choice-set digest");
    assert_eq!(choice_digest.len(), 64);
    assert!(choice_digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let current3 = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(
        current3, rev3,
        "current read must include revision 3 choice set"
    );
    let rev3_replay = route(
        &mut owner,
        "command",
        "task.source.record",
        rev3_params.clone(),
    )
    .await;
    assert_eq!(
        rev3_replay, rev3,
        "exact choice-set replay must return its receipt"
    );
    let mut changed_choice = rev3_params;
    changed_choice["choice_set"]["candidates"][0]["approach"] =
        json!("Replace the current preview path with a new implementation.");
    let choice_conflict = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":changed_choice}),
        )
        .await;
    assert_eq!(choice_conflict["error"]["code"], "input_conflict");
    let stale_rev2 = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":record(task_id, 2, Uuid::new_v4(), rev3_input)}),
        )
        .await;
    assert_eq!(stale_rev2["error"]["code"], "stale_revision");
    let after_choice_refusals = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(after_choice_refusals, rev3);

    let advisory_key = format!("matrix-advisory-{}", Uuid::new_v4());
    let advisory_params = json!({
        "task_id":task_id,"expected_task_revision":3,
        "request_key":advisory_key,
        "session_preference":"use_workspace",
        "request_preference":"use_workspace"
    });
    let no_call = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        advisory_params.clone(),
    )
    .await;
    assert_eq!(no_call["task_id"], task_id.to_string());
    assert_eq!(no_call["task_revision"], 3);
    assert_eq!(no_call["choice_set_digest"], rev3["choice_set_digest"]);
    assert_eq!(no_call["request_key"], advisory_key);
    assert_eq!(no_call["state"], "no_call");
    assert_eq!(no_call["reason"], "workspace_disabled");
    assert_eq!(no_call["config_revision"], 0);
    assert_eq!(no_call["provider_called"], false);
    assert_eq!(no_call["material_digest"].as_str().unwrap().len(), 64);
    let opportunity_id = Uuid::parse_str(no_call["opportunity_id"].as_str().unwrap()).unwrap();
    let workspace_id = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let (capability, decision_point, dispatch_count): (String, String, i64) = sqlx::query_as(
        "SELECT o.capability,o.decision_point, \
         (SELECT count(*) FROM advisory_dispatch d \
          WHERE d.tenant_id=o.tenant_id AND d.workspace_id=o.workspace_id AND d.opportunity_id=o.id) \
         FROM advisory_opportunity o \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(capability, "engineering_profile");
    assert_eq!(decision_point, "engineering.profile.before_selection");
    assert_eq!(dispatch_count, 0, "terminal no-call must have no dispatch");

    let saved_no_call = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({"task_id":task_id,"request_key":advisory_key}),
    )
    .await;
    assert_eq!(
        saved_no_call, no_call,
        "saved opportunity and digest must match"
    );
    let repeated_no_call = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        advisory_params.clone(),
    )
    .await;
    assert_eq!(
        repeated_no_call, no_call,
        "exact request must be idempotent"
    );
    let mut changed_preference = advisory_params.clone();
    changed_preference["request_preference"] = json!("skip");
    let preference_conflict = owner
        .call_error(
            "command",
            json!({"route":"engineering.advisory.request","params":changed_preference}),
        )
        .await;
    assert_eq!(preference_conflict["error"]["code"], "input_conflict");
    let mut changed_revision = advisory_params.clone();
    changed_revision["expected_task_revision"] = json!(2);
    let revision_conflict = owner
        .call_error(
            "command",
            json!({"route":"engineering.advisory.request","params":changed_revision}),
        )
        .await;
    assert_eq!(revision_conflict["error"]["code"], "input_conflict");
    let mut changed_task = advisory_params;
    changed_task["task_id"] = json!(Uuid::new_v4());
    let task_conflict = owner
        .call_error(
            "command",
            json!({"route":"engineering.advisory.request","params":changed_task}),
        )
        .await;
    assert_eq!(task_conflict["error"]["code"], "input_conflict");
    let stale_advisory = owner
        .call_error(
            "command",
            json!({"route":"engineering.advisory.request","params":{
                "task_id":task_id,"expected_task_revision":2,
                "request_key":format!("matrix-stale-{}", Uuid::new_v4())
            }}),
        )
        .await;
    assert_eq!(stale_advisory["error"]["code"], "stale_revision");
    let wrong_key = owner
        .call_error(
            "query",
            json!({"route":"engineering.advisory.get","params":{
                "task_id":task_id,"request_key":format!("matrix-missing-{}", Uuid::new_v4())
            }}),
        )
        .await;
    assert_eq!(wrong_key["error"]["code"], "not_found");

    let other_key = format!("matrix-other-{}", Uuid::new_v4());
    let mut other = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &other_key).await;
    let other_open = other.call("open_workspace", json!({})).await;
    assert_ne!(other_open["workspace"]["id"], opened["workspace"]["id"]);
    let hidden = other
        .call_error(
            "query",
            json!({"route":"task.source.get","params":{"task_id":task_id}}),
        )
        .await;
    assert_eq!(hidden["error"]["code"], "not_found");
    let hidden_advisory = other
        .call_error(
            "query",
            json!({"route":"engineering.advisory.get","params":{
                "task_id":task_id,"request_key":advisory_key
            }}),
        )
        .await;
    assert_eq!(hidden_advisory["error"]["code"], "not_found");

    other.finish().await;
    owner.finish().await;
}
