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
    advance_create_to_review_with_identity, begin_create_request, commit_create, complete_review,
    method_reads, omit_nulls, settle_and_finish, settle_and_finish_receipt,
};
use knowledge_operation_support::{SingleOperation, commit_single, ready_single};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{repository, route, route_error};
use tect_postgres::admin;
use uuid::Uuid;

fn revalidation(source: Value) -> SingleOperation {
    SingleOperation {
        operation: "revalidate",
        unit_id: None,
        expected_revision: Some(1),
        expected_lifecycle: Some("active"),
        document: None,
        revalidation: Some(json!({"sources":[source.clone()],
            "evidence_basis":"Fresh exact publication observation for the unchanged fixture declaration.",
            "valid_until":"2030-09-14T09:00:00Z","review_due_at":"2027-09-14T09:00:00Z"})),
        successor: None,
        replacement_bindings: json!([]),
        sources: json!([source]),
        knowledge_kind: json!("constraint"),
        profiles: json!(["general"]),
        erasure: "not_required",
        authored_followup: false,
    }
}

fn no_change_params(current: &Value) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    params["output"]["method_reads"] = method_reads(action);
    params["output"]["body"] = json!("Exact no-change result after current-state revalidation.");
    params["output"]["data"] = json!({"phase":"kc-result-handoff","data":{
        "canonical":"no_change","user_outcome":"achieved",
        "summary":"The exact canonical document already satisfies the requested outcome.",
        "remaining_work":[],"effects":[]}});
    params["output"]["verdict"] = json!("complete");
    params["output"]["outcome"] = json!("completed");
    params["output"]["transition"] = json!("complete");
    params["output"]["findings"] = json!([]);
    params["output"]["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

#[tokio::test]
async fn canonical_operation_guards_reject_stale_or_unverified_truth() {
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
    let socket = root.join("dk2-operation-guards.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk2-operation-guards-{}", Uuid::new_v4()),
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
        &format!("dk2-operation-guards-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let current = commit_create(&mut client, fixture["document"].clone()).await;
    settle_and_finish(&mut client, &current).await;
    let unit = current.receipt["applied_operations"][0]["unit_id"].clone();

    let mut no_change_document = fixture["document"].clone();
    no_change_document["title"] = json!("Exact existing no-change guard fixture");
    no_change_document["canonical_text"] = json!(
        "This distinct fixture must remain unchanged between reviewed identity match and terminal handoff."
    );
    no_change_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:guards:no-change-current");
    no_change_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner declares the distinct current no-change document.");
    let no_change_source = commit_create(&mut client, no_change_document.clone()).await;
    settle_and_finish(&mut client, &no_change_source).await;
    let no_change_unit = no_change_source.receipt["applied_operations"][0]["unit_id"].clone();
    let begun = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_create_request(
            &no_change_document,
            json!({"kind":"workspace"}),
            Uuid::new_v4(),
        ),
    )
    .await;
    let reviewable = advance_create_to_review_with_identity(
        &mut client,
        &no_change_document,
        begun,
        vec![json!({"client_label":"knowledge-document","unit_id":no_change_unit,
            "revision":1,"basis":"Exact prior canonical document read and compared field-for-field.",
            "ambiguous":false})],
    )
    .await;
    let no_change = complete_review(&mut client, &reviewable, "no_change").await;
    let mut drifted_document = no_change_document;
    drifted_document["title"] = json!("No-change target advanced after review");
    drifted_document["canonical_text"] = json!(
        "The target changed after the no-change review and must invalidate its terminal claim."
    );
    drifted_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:guards:no-change-drift");
    drifted_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner records a distinct post-review target revision.");
    let drift = commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(no_change_unit),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(drifted_document.clone()),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: drifted_document["sources"].clone(),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    settle_and_finish_receipt(&mut client, &drift["applied"]).await;
    let old_no_change = no_change_params(&no_change);
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        old_no_change.clone(),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "context_changed",
        "a reviewed no-change result must refuse when its exact matched unit advances"
    );
    assert!(old_no_change["change_id"].is_string());
    assert!(old_no_change["run_id"].is_string());
    assert!(old_no_change["run_revision"].is_i64());
    let same_phase_refused = route_error(
        &mut client,
        "command",
        "knowledge.change_record_input",
        json!({"request_id":Uuid::new_v4(),"change_id":old_no_change["change_id"],
            "run_id":old_no_change["run_id"],"run_revision":old_no_change["run_revision"],
            "revisit_phase_id":"kc-result-handoff",
            "reason":"The terminal review needs correction before another handoff.",
            "input":"A same-phase input cannot clear the only review basis for bounded rework."}),
    )
    .await;
    assert_eq!(same_phase_refused["error"]["code"], "forbidden");
    let unchanged_terminal: (i64, Option<String>) = sqlx::query_as(
        "SELECT revision,terminal_review_outcome FROM knowledge_change_runs WHERE id=$1",
    )
    .bind(Uuid::parse_str(old_no_change["run_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        unchanged_terminal.0,
        old_no_change["run_revision"].as_i64().unwrap()
    );
    assert_eq!(unchanged_terminal.1.as_deref(), Some("no_change"));
    let resume_request = json!({"request_id":Uuid::new_v4(),"change_id":old_no_change["change_id"],
            "run_id":old_no_change["run_id"],"run_revision":old_no_change["run_revision"],
            "revisit_phase_id":"kc-resolve-baseline",
            "reason":"The exact identity match changed after review and must be requalified.",
            "input":"The owner acknowledges the new current revision without silently repinning it."});
    let decoded: tect_domain::RecordKnowledgeChangeInput =
        serde_json::from_value(resume_request.clone()).unwrap();
    decoded.validate().unwrap();
    let resumed = route(
        &mut client,
        "command",
        "knowledge.change_record_input",
        resume_request,
    )
    .await;
    assert!(resumed.get("advanced").is_some());
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_phase_complete",
            old_no_change.clone()
        )
        .await["error"]["code"],
        "stale_revision"
    );
    let reset: (String, String, bool, bool, bool) = sqlx::query_as(
        "SELECT current_phase_id,delivery_mode,terminal_review_outcome IS NULL, \
         result IS NULL AND erased_result IS NULL, \
         EXISTS(SELECT 1 FROM knowledge_change_output_bindings b \
          WHERE b.run_id=r.id AND b.phase_id='kc-review-reconcile' AND b.stale) \
         FROM knowledge_change_runs r WHERE r.id=$1",
    )
    .bind(Uuid::parse_str(old_no_change["run_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reset,
        (
            "kc-resolve-baseline".into(),
            "phasewise".into(),
            true,
            true,
            true
        )
    );

    let mut impact_document = fixture["document"].clone();
    impact_document["title"] = json!("Post-review machine impact guard");
    impact_document["canonical_text"] =
        json!("Publication must stop if a new backend-owned copy appears after impact review.");
    impact_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:guards:impact-growth");
    impact_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner declares the exact impact-growth revision.");
    let impact_ready = ready_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(impact_document.clone()),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: impact_document["sources"].clone(),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: true,
        },
    )
    .await;
    let external_change =
        Uuid::parse_str(no_change_source.receipt["change_id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind, \
         relation_name,row_id) SELECT pg_catalog.gen_random_uuid(),tenant_id,workspace_id,$1, \
         'change_control','knowledge_lifecycle_changes',$2 FROM knowledge_lifecycle_changes \
         WHERE id=$2",
    )
    .bind(Uuid::parse_str(unit.as_str().unwrap()).unwrap())
    .bind(external_change)
    .execute(&pool)
    .await
    .unwrap();
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&impact_ready["actions"][0]).clone(),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "needs_context",
        "a new external backend-owned copy after review must invalidate the sealed impact subset"
    );

    let observed = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit,"revision":1}),
    )
    .await;
    let evidence_text = format!(
        "Fresh exact read of unit {} revision {} with event {}.",
        observed["document"]["unit_id"].as_str().unwrap(),
        observed["document"]["revision"].as_i64().unwrap(),
        current.receipt["applied_operations"][0]["event_id"]
            .as_str()
            .unwrap()
    );
    let first_source = json!({"kind":"snapshot","snapshot":{"title":"Fresh native exact read",
        "uri":"urn:tect:dk2:source:guards:revalidation","text":evidence_text,
        "observed_at":"2026-09-14T10:00:00Z","evidence_kind":"runtime_verification"}});
    let mut first_revalidation = revalidation(first_source.clone());
    first_revalidation.unit_id = Some(unit.clone());
    let accepted = commit_single(&mut client, first_revalidation).await;
    settle_and_finish_receipt(&mut client, &accepted["applied"]).await;
    let mut date_only_source = first_source;
    date_only_source["snapshot"]["observed_at"] = json!("2026-09-14T10:01:00Z");
    let mut date_only = revalidation(date_only_source);
    date_only.unit_id = Some(unit.clone());
    let ready = ready_single(&mut client, date_only).await;
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&ready["actions"][0]).clone(),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "needs_context",
        "changing only revalidation wrapper time must not count as genuinely new evidence bytes"
    );

    let mut predecessor_document = fixture["document"].clone();
    predecessor_document["title"] = json!("Expired-successor predecessor");
    predecessor_document["canonical_text"] =
        json!("This current fixture must not be superseded by an expired unit.");
    predecessor_document["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk2:source:guards:predecessor");
    predecessor_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner declares the current predecessor.");
    let predecessor = commit_create(&mut client, predecessor_document.clone()).await;
    settle_and_finish(&mut client, &predecessor).await;
    let predecessor_unit = predecessor.receipt["applied_operations"][0]["unit_id"].clone();
    let mut expired_document = fixture["document"].clone();
    expired_document["title"] = json!("Expired successor fixture");
    expired_document["canonical_text"] =
        json!("This fixture document is already outside its validity interval.");
    expired_document["valid_from"] = json!("2020-01-01T00:00:00Z");
    expired_document["valid_until"] = json!("2021-01-01T00:00:00Z");
    expired_document["sources"][0]["snapshot"]["uri"] = json!("urn:tect:dk2:source:guards:expired");
    expired_document["sources"][0]["snapshot"]["text"] =
        json!("The fixture owner records an already expired successor candidate.");
    let expired = commit_create(&mut client, expired_document).await;
    settle_and_finish(&mut client, &expired).await;
    let expired_unit = expired.receipt["applied_operations"][0]["unit_id"].clone();
    let ready = ready_single(
        &mut client,
        SingleOperation {
            operation: "supersede",
            unit_id: Some(predecessor_unit),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: Some(json!({"unit_id":expired_unit})),
            replacement_bindings: predecessor_document["bindings"].clone(),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&ready["actions"][0]).clone(),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "needs_context",
        "an expired active unit must not be accepted as a supersession successor"
    );

    let run = Uuid::parse_str(current.receipt["run_id"].as_str().unwrap()).unwrap();
    let output:(Uuid,String)=sqlx::query_as("SELECT id,digest FROM knowledge_change_outputs WHERE run_id=$1 AND phase_id='kc-domain-checks' ORDER BY revision DESC LIMIT 1")
        .bind(run).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE knowledge_change_outputs SET output=jsonb_set(output,'{body}','\"tampered retained digest\"'::jsonb) WHERE id=$1")
        .bind(output.0).execute(&pool).await.unwrap();
    let tampered_source = json!({"kind":"pipeline_output","output":{"run_id":run,"output_id":output.0,
        "digest":output.1,"evidence_kind":"runtime_verification","evidence_scope":"Tampered Knowledge output guard."}});
    let mut tampered_document = fixture["document"].clone();
    tampered_document["title"] = json!("Tampered output source proposal");
    tampered_document["sources"] = json!([tampered_source]);
    let refused = route_error(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_create_request(
            &tampered_document,
            json!({"kind":"workspace"}),
            Uuid::new_v4(),
        ),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "invalid_source",
        "a stored Knowledge output body changed under its retained digest must be refused"
    );

    let event = Uuid::parse_str(
        current.receipt["applied_operations"][0]["event_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    sqlx::query(
        "UPDATE knowledge_publication_events SET rdf_digest='bogus-projection-digest' WHERE id=$1",
    )
    .bind(event)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE knowledge_revisions SET rdf_digest='bogus-projection-digest' WHERE publication_event_id=$1")
        .bind(event).execute(&pool).await.unwrap();
    let refused = route_error(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit,"revision":1}),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "internal_invariant",
        "matching bogus SQL event/revision digests must not override intact native and unit-owned receipt truth"
    );
    client.finish().await;
}
