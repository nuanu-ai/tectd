use super::*;

pub(super) fn consumed_outputs(context: &Value) -> Vec<Value> {
    let current = context["run"]["current_phase_ordinal"].as_u64().unwrap();
    context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|binding| {
            binding["stale"] == false && binding["phase_ordinal"].as_u64().unwrap() < current
        })
        .map(|binding| {
            json!({"phase_id":binding["phase_id"],
            "output_revision":binding["output_revision"],"digest":binding["output_digest"]})
        })
        .collect()
}

fn consumed_inputs(context: &Value) -> Vec<Value> {
    context["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|input| input["phase_id"] == context["run"]["current_phase_id"])
        .map(|input| {
            json!({"input_id":input["id"],"sequence":input["sequence"],
            "digest":input["digest"]})
        })
        .collect()
}

pub(crate) fn completion(
    context: &Value,
    verdict: &str,
    outcome: &str,
    transition: &str,
    revisit_phase_id: Option<&str>,
    terminal_result: Option<Value>,
) -> Value {
    let phase = &context["definition"]["phases"][0];
    let consumed = consumed_outputs(context);
    let artifacts = artifacts(phase, verdict, outcome, &consumed);
    let producer = format!("full-producer:{}", phase["id"].as_str().unwrap());
    let body = format!(
        "Full lifecycle receipt for {}.",
        phase["id"].as_str().unwrap()
    );
    let mut output = json!({
        "body":body,
        "producer_context_id":producer,"fields":fields(phase,verdict),"verdict":verdict,
        "dispositions":phase["verdict_routes"].as_array().unwrap().iter()
            .find(|route| route["verdict"]==verdict && route["outcome"]==outcome
                && route["transition"]==transition && revisit_phase_id.map_or(
                    route["revisit_to"].as_array().is_none_or(Vec::is_empty),
                    |target| route["revisit_to"].as_array().is_some_and(
                        |items| items.iter().any(|item| item==target))))
            .unwrap()["dispositions"].clone(),
        "skill_reads":receipts(phase["skills"].as_array().map(Vec::as_slice).unwrap_or_default()),
        "resource_reads":receipts(phase["resources"].as_array().map(Vec::as_slice).unwrap_or_default()),
        "artifacts":artifacts,
        "validator_receipts":validator_receipts(phase,verdict,&artifacts),
        "reference":format!("full-fixture/{}.md",phase["id"].as_str().unwrap())
    });
    if phase["fresh_reviewer_input"] == true {
        let producers = context["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["stale"] == false)
            .map(|item| item["producer_context_id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        output["reviewer_context"] = json!({"reviewer_identity":"reported-independent-reviewer",
            "reviewer_context_id":producer,"producer_context_ids":producers,"fresh_input":true});
    }
    let mut request = json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":phase["id"],
        "outcome":outcome,"transition":transition,"output":output,
        "consumed_outputs":consumed,"consumed_inputs":consumed_inputs(context),
        "publish_blocked_result":false});
    if let Some(target) = revisit_phase_id {
        request["revisit_phase_id"] = json!(target);
    }
    if let Some(result) = terminal_result {
        request["terminal_result"] = result;
    }
    request
}

pub(crate) fn successful_route(context: &Value) -> (&str, &str, &str) {
    let phase = &context["definition"]["phases"][0];
    let route = phase["verdict_routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| {
            route["outcome"] == "completed"
                && (route["transition"] == "continue" || route["transition"] == "complete")
                && route["revisit_to"].as_array().is_none_or(Vec::is_empty)
        })
        .unwrap();
    (
        route["verdict"].as_str().unwrap(),
        route["outcome"].as_str().unwrap(),
        route["transition"].as_str().unwrap(),
    )
}

pub(crate) async fn refresh_knowledge(client: &mut Mcp, context: &Value) -> Value {
    let stale = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context["run"]["id"]}}),
        )
        .await;
    assert_eq!(stale["run"]["id"], context["run"]["id"]);
    assert_eq!(stale["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        stale["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    if stale["knowledge_resource_status"]["state"] == "inactive" {
        assert!(stale["knowledge_resources"].is_null());
        assert!(stale["knowledge"].is_null());
        assert!(find_action(&stale, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    let resource_state = stale["knowledge_resource_status"]["state"]
        .as_str()
        .unwrap();
    assert!(
        matches!(resource_state, "stale" | "needs_context"),
        "{stale}"
    );
    if resource_state == "stale" {
        assert_eq!(
            stale["run"]["revision"].as_i64().unwrap(),
            stale["knowledge_resources"]["run_revision"]
                .as_i64()
                .unwrap()
                + 1
        );
    }
    let action = find_action(&stale, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    client
        .call(
            "command",
            json!({"route":"pipeline.knowledge_refresh","params":action_params(action)}),
        )
        .await;
    let current = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context["run"]["id"]}}),
        )
        .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    current
}
