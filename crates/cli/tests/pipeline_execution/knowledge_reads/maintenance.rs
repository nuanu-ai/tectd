//! Only the documented maintenance begin shape; no task freshness inference.
use super::*;

pub(super) fn inline_context(raw: &Value) -> Option<&Value> {
    let bodies: Vec<_> = ["created", "replay"]
        .iter()
        .filter_map(|key| raw.get(*key))
        .filter(|body| body.get("task").is_some() && body.get("change").is_some())
        .collect();
    assert!(bodies.len() <= 1, "ambiguous maintenance begin variant");
    let body = bodies.first()?;
    let contexts: Vec<_> = ["created", "replay"]
        .iter()
        .filter_map(|key| body["change"].get(*key))
        .collect();
    assert_eq!(
        contexts.len(),
        1,
        "one actual nested lifecycle begin variant required"
    );
    Some(contexts[0])
}
pub(super) fn recognizes(raw: &Value) -> bool {
    raw.get("task_id").is_some() || inline_context(raw).is_some()
}
pub(super) async fn resolve(client: &mut Mcp, raw: Value) -> ResolvedKnowledgeView {
    metadata(&raw);
    let nested = inline_context(&raw);
    let (pins, task_pins) = if let Some(context) = nested {
        let body = raw.get("created").or_else(|| raw.get("replay")).unwrap();
        let task: tect_domain::KnowledgeMaintenanceTask =
            serde_json::from_value(body["task"].clone()).unwrap();
        assert!(task.revision > 0);
        uuid(&context["change_id"]);
        uuid(&context["run"]["id"]);
        assert!(context["run"]["revision"].as_i64().is_some_and(|n| n > 0));
        serde_json::from_value::<tect_domain::PipelineRunStatus>(context["run"]["status"].clone())
            .unwrap();
        let phase = context["run"]
            .get("current_phase_id")
            .expect("actual nested phase pin required");
        let _: Option<tect_domain::KnowledgeChangePhaseId> =
            serde_json::from_value(phase.clone()).unwrap();
        (
            json!({"change_id":context["change_id"],"run":context["run"]}),
            body["task"].clone(),
        )
    } else {
        assert_eq!(
            keys(&raw),
            BTreeSet::from([
                "outcome",
                "changed",
                "task_id",
                "task_revision",
                "task_state",
                "change_id",
                "run_id",
                "run_revision",
                "run_status",
                "current_phase_id",
                "actions",
                "recommended_action"
            ])
        );
        let outcome = raw["outcome"].as_str().unwrap();
        assert!(matches!(outcome, "created" | "replay"));
        assert!(raw["changed"].is_boolean());
        assert_eq!(raw["changed"], outcome == "created");
        uuid(&raw["task_id"]);
        assert!(raw["task_revision"].as_i64().is_some_and(|n| n > 0));
        serde_json::from_value::<tect_domain::KnowledgeMaintenanceTaskState>(
            raw["task_state"].clone(),
        )
        .unwrap();
        uuid(&raw["change_id"]);
        uuid(&raw["run_id"]);
        assert!(raw["run_revision"].as_i64().is_some_and(|n| n > 0));
        serde_json::from_value::<tect_domain::PipelineRunStatus>(raw["run_status"].clone())
            .unwrap();
        let phase = raw
            .get("current_phase_id")
            .expect("actual compact phase pin required");
        let _: Option<tect_domain::KnowledgeChangePhaseId> =
            serde_json::from_value(phase.clone()).unwrap();
        (
            json!({"change_id":raw["change_id"],"run":{"id":raw["run_id"],"revision":raw["run_revision"],
            "status":raw["run_status"],"current_phase_id":phase}}),
            json!({"id":raw["task_id"],"revision":raw["task_revision"],"state":raw["task_state"]}),
        )
    };
    let actions = raw["actions"].as_array().unwrap();
    assert_eq!(
        actions.len(),
        1,
        "actual maintenance begin has one Current navigation"
    );
    let action = &actions[0];
    assert_eq!(keys(action), BTreeSet::from(["kind", "tool", "arguments"]));
    assert_eq!(action["kind"], "ready_call");
    assert_eq!(action["tool"], "query");
    assert_eq!(
        action["arguments"],
        json!({"route":"knowledge.lifecycle","params":{"change_id":pins["change_id"],"view":"current"}})
    );
    let mut resolved = read(client, action["arguments"].clone(), Some(action.clone())).await;
    let current = &resolved.value["current"];
    assert_eq!(current["change_id"], pins["change_id"]);
    if nested.is_some() {
        assert_eq!(
            current["run"], pins["run"],
            "all actual nested run pins must remain identical"
        );
    } else {
        for field in ["id", "revision", "status", "current_phase_id"] {
            assert_eq!(
                current["run"][field], pins["run"][field],
                "compact maintenance run pin drift: {field}"
            );
        }
    }
    let tasks: Vec<_> = current["maintenance_tasks"]
        .as_array()
        .expect("actual linked task collection")
        .iter()
        .filter(|task| task["id"] == task_pins["id"])
        .collect();
    assert_eq!(
        tasks.len(),
        1,
        "one actual linked task must match the maintenance receipt"
    );
    let task = tasks[0];
    for field in ["revision", "state"] {
        assert_eq!(
            task[field], task_pins[field],
            "immediate maintenance task pin drift: {field}"
        );
    }
    for (field, expected) in [
        ("change_id", &current["change_id"]),
        ("run_id", &current["run"]["id"]),
    ] {
        assert_eq!(
            &task[field], expected,
            "actual task must link to verified Current"
        );
        if let Some(pin) = task_pins.get(field) {
            assert_eq!(
                pin, expected,
                "inline task link differs from verified Current"
            );
        }
    }
    resolved.raw_response = raw;
    resolved
}
