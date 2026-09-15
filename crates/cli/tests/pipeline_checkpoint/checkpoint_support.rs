use super::*;

pub(super) fn decision_inquiry() -> Value {
    json!({"topic_level":"program",
        "task_context":{"target_iris":["urn:fixture:public-decision"]},
        "completion":{"kind":"decision","requested_outcome":"decision"}})
}

pub(super) fn research_inquiry() -> Value {
    json!({"topic_level":"program",
        "task_context":{"target_iris":["urn:fixture:private-research"]},
        "completion":{"kind":"research","allow_inconclusive":true}})
}

pub(super) fn knowledge_document(label: &str, target: &str, access: &str, program: Uuid) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    let document = &mut fixture["document"];
    document["title"] = json!(label);
    document["canonical_text"] = json!(format!("Exact inquiry brief for {label}."));
    document["access_scope"] = json!(access);
    let purpose = if access == "owners_only" {
        "reference"
    } else {
        "required"
    };
    document["bindings"] = json!([
        {"target":{"kind":"workspace"},"purpose":purpose,
            "version_resolution":{"kind":"current_accepted"}},
        {"target":{"kind":"program","program_id":program},"purpose":purpose,
            "version_resolution":{"kind":"current_accepted"}}
    ]);
    document["sources"][0]["snapshot"]["uri"] = json!(format!("urn:fixture:{label}:source"));
    document["sources"][0]["snapshot"]["text"] = json!(format!("Source for {label}."));
    document["planning_briefs"] = json!([{
        "local_id":format!("{label}-program"),"stage":"program",
        "instruction":format!("Use the exact {label} inquiry evidence."),
        "conditions":[],"exceptions":[],"purpose":"Bound the inquiry fixture.",
        "selectors":{"target_iris":[target]}
    }]);
    document.clone()
}

pub(super) fn erase(unit: Value) -> SingleOperation {
    SingleOperation {
        operation: "erase",
        unit_id: Some(unit),
        expected_revision: Some(1),
        expected_lifecycle: Some("active"),
        document: None,
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources: json!([]),
        knowledge_kind: json!("procedure"),
        profiles: json!(["general", "runbook"]),
        erasure: "owned_live_copies",
        authored_followup: false,
    }
}

pub(super) fn contains_unit(context: &Value, unit: &Value) -> bool {
    context["knowledge_resources"]["selected"]
        .as_array()
        .is_some_and(|selected| selected.iter().any(|value| value["unit_id"] == *unit))
}

pub(super) fn completion(
    context: &Value,
    verdict: &str,
    outcome: &str,
    transition: &str,
    revisit_phase_id: Option<&str>,
    terminal_result: Option<Value>,
) -> Value {
    let mut request = base_completion(
        context,
        verdict,
        outcome,
        transition,
        revisit_phase_id,
        terminal_result,
    );
    if context["knowledge_resources"]["selected"]
        .as_array()
        .is_some_and(|selected| !selected.is_empty())
    {
        request["consumed_knowledge"] = json!({
            "manifest_id":context["knowledge_resources"]["id"],
            "digest":context["knowledge_resources"]["digest"]
        });
    }
    request
}

pub(super) fn brainstorming_draft() -> Value {
    json!({"coverage_summary":"A material decision may require one bounded research handoff.",
        "nodes":[{"kind":"work","identity":{"local":"decision"},
            "title":"Select the evidence-backed operating decision",
            "outcome":"The exact decision or its unresolved condition is explicit",
            "includes":["alternatives","criteria","research handoff"],
            "excludes":["automatic implementation","automatic publication"],
            "dependencies":[],"proof":["Exact decision disposition"],
            "pipeline":"slice.deep-brainstorming",
            "pipeline_reason":"The primary uncertainty is a consequential choice among alternatives.",
            "source_result_ids":[]}],"supersessions":[]})
}

pub(super) fn existing_work(node: &Value) -> Value {
    let mut value = json!({"kind":"work","identity":{"candidate_id":node["id"],"revision":node["revision"]},
        "title":node["title"],"outcome":node["outcome"],"includes":node["includes"],
        "excludes":node["excludes"],"dependencies":node["dependencies"],"proof":node["proof"],
        "pipeline":node["pipeline"],"pipeline_reason":node["pipeline_reason"],
        "source_result_ids":node["source_result_ids"]});
    for field in [
        "why_lightweight_insufficient",
        "why_further_vertical_split_not_viable",
        "source_checkpoint",
    ] {
        if !node[field].is_null() {
            value[field] = node[field].clone();
        }
    }
    value
}

pub(super) fn research_work(local: &str, source: &Value, dependency: Option<&Value>) -> Value {
    let dependencies = dependency.map_or_else(Vec::new, |node| {
        vec![json!({"candidate_id":node["id"],"revision":node["revision"]})]
    });
    json!({"kind":"work","identity":{"local":local},
        "title":format!("Research the decisive unknown {local}"),
        "outcome":"The exact checkpoint question is answered with traceable evidence",
        "includes":["checkpoint question","answer criteria","limitations"],
        "excludes":["decision authority","automatic publication"],
        "dependencies":dependencies,"proof":["Exact terminal Research result"],
        "pipeline":"slice.research",
        "pipeline_reason":"The checkpoint requires substantial evidence collection and synthesis.",
        "source_result_ids":[],"source_checkpoint":source})
}

pub(super) fn draft_request(context: &Value, draft: Value) -> Value {
    let mut request = json!({"kind":"draft","scope_id":context["scope"]["id"],
        "candidate_set_id":context["candidate_set"]["id"],
        "revision":context["candidate_set"]["revision"],
        "snapshot_id":context["snapshot"]["id"],
        "input_cursor":context["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),"draft":draft});
    if context["planning_knowledge"]["manifest"]["id"].is_string() {
        request["consumed_knowledge"] = json!({
            "manifest_id":context["planning_knowledge"]["manifest"]["id"],
            "digest":context["planning_knowledge"]["manifest"]["digest"],
            "workspace_generation":context["planning_knowledge"]["manifest"]["workspace_generation"]
        });
    }
    request
}

pub(super) async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    let mut request = completion(&context, verdict, outcome, transition, None, None);
    match context["run"]["current_phase_id"].as_str() {
        Some("B01") => {
            request["output"]["fields"]["topic_level"] = context["inquiry"]["topic_level"].clone();
            request["output"]["fields"]["requested_outcome"] = json!("decision");
        }
        Some("R01") => {
            request["output"]["fields"]["topic_level"] = context["inquiry"]["topic_level"].clone();
            request["output"]["fields"]["allow_inconclusive"] =
                context["inquiry"]["completion"]["allow_inconclusive"]
                    .as_bool()
                    .unwrap()
                    .to_string()
                    .into();
        }
        _ => {}
    }
    route(client, "command", "slice.pipeline.phase.complete", request).await["context"].clone()
}

pub(super) async fn create_checkpoint(client: &mut Mcp, context: &Value) -> Value {
    let mut request = completion(
        context,
        "waiting_research",
        "waiting_input",
        "continue",
        None,
        None,
    );
    request["research_checkpoint"] = json!({
        "question":"Which operating constraint decides between the two viable alternatives?",
        "answer_criteria":"Identify the current constraint, its source, and the bounded limitation.",
        "inquiry":research_inquiry(),
        "reason":"The decisive unknown needs a separate bounded evidence synthesis."
    });
    if context["knowledge_resources"]["selected"]
        .as_array()
        .is_some_and(|selected| !selected.is_empty())
    {
        request["consumed_knowledge"] = json!({
            "manifest_id":context["knowledge_resources"]["id"],
            "digest":context["knowledge_resources"]["digest"]
        });
    }
    let completed = route(client, "command", "slice.pipeline.phase.complete", request).await;
    completed["context"]["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["status"] == "open")
        .unwrap()
        .clone()
}

pub(super) async fn refresh_or_capture_knowledge(client: &mut Mcp, context: &Value) -> Value {
    let current = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert!(matches!(
        current["knowledge_resource_status"]["state"].as_str(),
        Some("needs_context" | "stale")
    ));
    let refresh = find_action(&current, "pipeline.knowledge_refresh").unwrap();
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let current = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    current
}

pub(super) fn begin_params(
    scope: &Value,
    slice: &Value,
    inquiry: Value,
    source: Option<&Value>,
) -> Value {
    let mut request = json!({"request_id":Uuid::new_v4(),"scope_id":scope["id"],
        "slice_id":slice["id"],"slice_revision":slice["revision"],"delivery_mode":"phasewise",
        "qualification_reason":"Exercise exact inquiry and checkpoint lifecycle contracts.",
        "inquiry":inquiry});
    if let Some(source) = source {
        request["source_checkpoint"] = source.clone();
    }
    request
}

pub(super) async fn begin_run(
    client: &mut Mcp,
    scope: &Value,
    slice: &Value,
    inquiry: Value,
    source: Option<&Value>,
) -> Value {
    route(
        client,
        "command",
        "slice.pipeline.begin",
        begin_params(scope, slice, inquiry, source),
    )
    .await["created"]
        .clone()
}
