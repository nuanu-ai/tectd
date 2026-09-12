use super::recovery_support::{Mcp, action_name};
use super::{create_program, id, planning_ref, rows, success_ref};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub(super) async fn run(first: &mut Mcp, pool: &PgPool) {
    let covered_program = create_program(
        first,
        "The delivery migration is already accepted and covers the requested outcome.",
        "Covered migration",
    )
    .await;
    let covered = first
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),"program_id":covered_program,"program_revision":2,
                "boundary":"finite","input":"Treat the accepted migration as completed work."}),
        )
        .await;
    let covered_set = id(&covered["context"]["candidate_set"]["id"]);
    let covered_input = planning_ref(&covered["context"], 1);
    let covered_success = success_ref(&covered["context"]);
    let all_covered = first.call("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":covered_set,"revision":1,
        "snapshot_id":id(&covered["context"]["snapshot"]["id"]),"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"finite","goals":[{"identity":{"local":"done_goal"},
            "text":"Users can inspect email preferences and tests pass","source_ref_id":covered_success,
            "resolution":{"kind":"evidence","reference":{"local":"accepted_migration"}}}],
            "evidence":[{"identity":{"local":"accepted_migration"},"kind":"accepted_work",
                "summary":"The accepted migration already delivers the outcome","source_ref_id":covered_input,
                "authority_input_sequence":1}],"candidates":[],"blockers":[],
            "empty_disposition":{"kind":"all_covered","reason":"The accepted migration covers the finite success outcome",
                "source_ref_id":covered_success}}
    })).await;
    let covered_evidence = id(&all_covered["draft"]["evidence"][0]["id"]);
    let covered_goal = id(&all_covered["draft"]["goals"][0]["id"]);
    let covered_ready = first.call("save_candidate_set", json!({
        "kind":"review","candidate_set_id":covered_set,"revision":2,
        "snapshot_id":id(&covered["context"]["snapshot"]["id"]),"input_cursor":1,"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"The finite outcome is traceably covered by accepted work.",
            "findings":[],"candidate_decisions":[]}
    })).await;
    assert_eq!(covered_ready["context"]["candidate_set"]["status"], "ready");
    let covered_recorded = first
        .call(
            "record_candidate_input",
            json!({
        "candidate_set_id":covered_set,"revision":3,"request_id":Uuid::new_v4(),
        "input":"Re-evaluate coverage without deleting accepted provenance."}),
        )
        .await;
    assert_eq!(
        action_name(&covered_recorded["actions"][0]),
        Some("scope.candidates.refresh")
    );
    let covered_refresh = first.call("refresh_candidate_set", json!({
        "candidate_set_id":covered_set,"revision":4,"request_id":Uuid::new_v4(),"program_revision":2
    })).await;
    let before_remove = rows(pool, covered_set).await;
    let remove = first.call_error("save_candidate_set", json!({
        "kind":"draft","candidate_set_id":covered_set,"revision":5,
        "snapshot_id":id(&covered_refresh["context"]["snapshot"]["id"]),"input_cursor":2,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"finite","goals":[{"identity":{"id":covered_goal,"revision":1},
            "text":"Users can inspect email preferences and tests pass","source_ref_id":success_ref(&covered_refresh["context"]),
            "resolution":{"kind":"evidence","reference":{"local":"replacement"}}}],
            "evidence":[{"identity":{"local":"replacement"},"kind":"verified_evidence",
                "summary":"Replacement claim","source_ref_id":planning_ref(&covered_refresh["context"],2)}],
            "candidates":[],"blockers":[],"empty_disposition":{"kind":"all_covered",
                "reason":"Claimed replacement","source_ref_id":success_ref(&covered_refresh["context"])}}
    })).await;
    assert_eq!(remove["error"]["code"], "forbidden");
    assert_eq!(rows(pool, covered_set).await, before_remove);
    assert!(covered_evidence != Uuid::nil());

    for (kind, pending) in [
        ("needs_input", Some("Which authority applies?")),
        ("out_of_boundary", None),
    ] {
        let program = create_program(
            first,
            "Classify an honest empty planning result.",
            &format!("Empty {kind}"),
        )
        .await;
        let created = first.call("begin_candidate_set", json!({
            "request_id":Uuid::new_v4(),"program_id":program,"program_revision":2,
            "boundary":"ongoing","input":"Use the captured request as the disposition source."
        })).await;
        let set = id(&created["context"]["candidate_set"]["id"]);
        let source = planning_ref(&created["context"], 1);
        let mut draft = json!({"boundary":"ongoing","goals":[],"evidence":[],
            "candidates":[],"blockers":[],"empty_disposition":{"kind":kind,
                "reason":format!("The captured request is classified as {kind}"),"source_ref_id":source}});
        if let Some(question) = pending {
            draft["pending_question"] = json!(question);
        }
        let saved = first
            .call(
                "save_candidate_set",
                json!({
                    "kind":"draft","candidate_set_id":set,"revision":1,
                    "snapshot_id":id(&created["context"]["snapshot"]["id"]),"input_cursor":1,
                    "request_id":Uuid::new_v4(),"draft":draft
                }),
            )
            .await;
        let blocked = first
            .call(
                "save_candidate_set",
                json!({
                    "kind":"review","candidate_set_id":set,"revision":2,
                    "snapshot_id":id(&created["context"]["snapshot"]["id"]),"input_cursor":1,
                    "request_id":Uuid::new_v4(),"review":{"verdict":"blocked",
                        "summary":"The typed empty disposition explains why no candidate is ready.",
                        "findings":[],"candidate_decisions":[]}
                }),
            )
            .await;
        assert_eq!(saved["draft"]["empty_disposition"]["kind"], kind);
        assert_eq!(blocked["context"]["candidate_set"]["status"], "blocked");
    }
}
