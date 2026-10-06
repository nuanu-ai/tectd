use super::*;

pub(super) fn assert_rejected(response: &Value) {
    if response.get("error").is_some() {
        return;
    }
    assert_eq!(response["result"]["isError"], true, "{response}");
    let payload = tool_payload(response);
    assert_eq!(payload["error"]["code"], "invalid_arguments", "{payload}");
}

pub(super) fn action_tools(payload: &Value) -> Vec<&str> {
    payload["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|action| {
            action["arguments"]["route"]
                .as_str()
                .or_else(|| action["arguments"]["method"].as_str())
                .unwrap_or_else(|| action["tool"].as_str().unwrap())
        })
        .collect()
}

pub(super) fn summary_ids_and_actions(payload: &Value, ready_id: Uuid) -> Vec<Uuid> {
    let programs = payload["programs"].as_array().unwrap();
    let tools = action_tools(payload);
    programs
        .iter()
        .enumerate()
        .map(|(index, summary)| {
            let keys: BTreeSet<_> = summary
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                BTreeSet::from(["current_step", "id", "name", "revision", "status"])
            );
            let id = Uuid::parse_str(summary["id"].as_str().unwrap()).unwrap();
            let status = if id == ready_id { "open" } else { "draft" };
            let step = if id == ready_id { "ready" } else { "compose" };
            assert_eq!(summary["status"], status);
            assert_eq!(summary["current_step"], step);
            assert_eq!(tools[index], "program.get");
            assert_eq!(payload["actions"][index]["kind"], "ready_call");
            let params = &payload["actions"][index]["arguments"]["params"];
            assert_eq!(params, &json!({"program_id":id}));
            id
        })
        .collect()
}

pub(super) async fn complete(client: &mut Mcp, program_id: Uuid) -> Value {
    client
        .call(
            "save_program",
            json!({
                "program_id":program_id,"revision":1,"input_cursor":1,
                "name":"Ready Program","intent":"Exercise ready ordering",
                "basis":"Original input","boundaries":"Program routing only",
                "constraints":"Keep creation last","success":"Deterministic pages",
                "pending_question":null,"complete":true
            }),
        )
        .await
}
