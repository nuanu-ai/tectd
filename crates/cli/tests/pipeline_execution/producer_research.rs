use super::*;

fn research_draft() -> Value {
    json!({"coverage_summary":"Research promotion producer receipt fixture","nodes":[{
        "kind":"work","identity":{"local":"research-producer"},
        "title":"Research an exact durable source",
        "outcome":"The exact research promotion edge is published by Knowledge Change",
        "includes":["source provenance","promotion edge","publisher receipt"],
        "excludes":["automatic publication"],"dependencies":[],
        "proof":["Exact publisher receipt"],"pipeline":"slice.research-to-durable-knowledge",
        "pipeline_reason":"Exercise the Research producer publication handoff",
        "source_result_ids":[]}],"supersessions":[]})
}

async fn begin_research(client: &mut Mcp, repo: &std::path::Path) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope = route(
        client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(client, &scope["created"]["planning"], research_draft()).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":opened["created"]["id"],
        "slice_revision":opened["created"]["revision"],"delivery_mode":"phasewise",
        "qualification_reason":"Exact Research publication handoff integration fixture."}),
    )
    .await["created"]
        .clone()
}

pub(super) async fn prove(client: &mut Mcp, repo: &std::path::Path) {
    let mut current = begin_research(client, repo).await;
    assert_eq!(
        current["run"]["definition_kind"],
        "slice.research-to-durable-knowledge"
    );
    while current["run"]["current_phase_ordinal"].as_u64().unwrap() <= 19 {
        current = advance(client, current).await;
    }
    let source = current["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["phase_ordinal"] == 19)
        .unwrap();
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    fixture["document"]["sources"] = json!([{"kind":"pipeline_output","output":{
        "run_id":current["run"]["id"],"output_id":source["id"],"digest":source["digest"],
        "evidence_kind":"research","evidence_scope":"Exact Research promotion edge output."}}]);
    let committed = commit_create(client, fixture["document"].clone()).await;
    current = refresh(client, &current).await;
    while current["run"]["current_phase_ordinal"].as_u64().unwrap() < 22 {
        current = advance(client, current).await;
    }
    let mut request = completion(
        &current,
        "promoted_to_durable_kb",
        "completed",
        "complete",
        None,
        Some(
            json!({"summary":"Research publication handoff completed with exact backend receipt.",
            "evidence":[{"kind":"integration_test","reference":"pipeline_execution_knowledge_publication.rs",
                "observation":"Research phase 19 source lineage and publisher receipt were verified."}],
            "scope_impact":"The Research result records the separate canonical publication.",
            "remaining_work":"None."}),
        ),
    );
    request["output"]["fields"]["external_promotion_evidence"] =
        json!("The publisher receipt covers the exact Research phase 19 promotion edge output.");
    request["output"]["fields"]["external_promotion_owner"] = json!("knowledge.change");
    request["output"]["fields"]["external_promotion_reference"] = committed.receipt["id"].clone();
    request["output"]["fields"]["external_promotion_digest"] = committed.receipt["digest"].clone();
    request["output"]["fields"]["external_promotion_authority_evidence"] = json!(format!(
        "sealed-command:{}",
        committed.receipt["sealed_command_digest"].as_str().unwrap()
    ));
    request["output"]["knowledge_publication"] = json!({
        "change_id":committed.receipt["change_id"],"publisher_receipt_id":committed.receipt["id"],
        "publisher_receipt_digest":committed.receipt["digest"],
        "operation_ids":[committed.receipt["applied_operations"][0]["operation_id"]]});
    acknowledge_knowledge(&mut request, &current);
    let completed =
        route(client, "command", "slice.pipeline.phase.complete", request).await["context"].clone();
    assert_eq!(completed["run"]["status"], "completed");
    let output = completed["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["phase_ordinal"] == 22)
        .unwrap();
    assert_eq!(
        output["knowledge_publication"]["publisher_receipt_id"],
        committed.receipt["id"]
    );
    assert_eq!(
        output["knowledge_publication"]["publisher_receipt_digest"],
        committed.receipt["digest"]
    );
}
