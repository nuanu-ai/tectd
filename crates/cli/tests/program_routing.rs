//! Stdio schema, state routing, Program ordering, and pagination acceptance.
mod recovery_support;

use recovery_support::{
    Daemon, Mcp, host_file, private_temp, public_call, tagged_url, tool_payload,
};
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;
use tect_postgres::admin;
use uuid::Uuid;

#[path = "program_routing/support.rs"]
mod support;
use support::{action_tools, assert_rejected, complete, summary_ids_and_actions};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn schemas_and_state_route_uninitialized_empty_one_and_many_programs() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("routing.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-route-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-routing-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;

    let listed = client.exchange("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: BTreeSet<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["command", "execute", "get_state", "help", "query"])
    );
    for name in ["get_state", "query", "command", "execute", "help"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    let query = tools.iter().find(|tool| tool["name"] == "query").unwrap();
    assert_eq!(query["inputSchema"]["required"], json!(["route", "params"]));
    assert_eq!(
        query["inputSchema"]["properties"]["route"]["enum"],
        json!([
            "workspace_state",
            "program.get",
            "program.list",
            "source.list",
            "setup.get",
            "scope.candidates.context",
            "scope.candidates.delta.status",
            "scope.context",
            "slice.pipelines",
            "slice.candidates.context",
            "slice.context",
            "slice.pipeline.context",
            "slice.pipeline.instruction",
            "knowledge.context",
            "knowledge.change",
            "knowledge.lifecycle",
            "knowledge.unit",
            "knowledge.maintenance",
            "knowledge.search",
            "engineering.advisory.get",
            "workspace.advisory.config",
            "session.advisory.preference",
            "workspace.advisory.audit",
            "scope.advisory.get",
            "scope.advisory.audit",
            "candidate.advisory.get",
            "candidate.advisory.audit",
            "pipeline.open_effect.get",
            "pipeline.phase_effect.get",
            "slice.pipeline.evidence_artifact.read",
            "engineering.matrix.context.effective.get",
            "task.source.get",
            "engineering.matrix.cards.get",
            "engineering.matrix.disposition.get"
        ])
    );
    let command = tools.iter().find(|tool| tool["name"] == "command").unwrap();
    assert_eq!(
        command["inputSchema"]["properties"]["route"]["enum"],
        json!([
            "workspace.open",
            "source.register",
            "session.select_worktrees",
            "program.begin",
            "program.save",
            "program.record_input",
            "program.knowledge.refresh",
            "setup.inspect",
            "setup.begin",
            "setup.save",
            "setup.record_input",
            "scope.candidates.begin",
            "scope.candidates.save",
            "scope.candidates.record_input",
            "scope.candidates.refresh",
            "scope.candidates.delta",
            "scope.open",
            "slice.candidates.save",
            "slice.candidates.input",
            "slice.candidates.refresh",
            "slice.open",
            "slice.result.record",
            "slice.pipeline.begin",
            "slice.pipeline.run.migrate",
            "slice.pipeline.phase.complete",
            "slice.pipeline.input",
            "slice.pipeline.delivery.escalate",
            "slice.pipeline.checkpoint.resolve",
            "knowledge.change_prepare",
            "knowledge.change_review",
            "knowledge.change_publish",
            "pipeline.knowledge_refresh",
            "knowledge.change_begin",
            "knowledge.change_phase_complete",
            "knowledge.change_record_input",
            "knowledge.change_commit",
            "knowledge.change_settle_effects",
            "knowledge.maintenance_observe",
            "knowledge.maintenance_begin",
            "engineering.advisory.request",
            "scope.advisory.request",
            "scope.advisory.disposition",
            "session.advisory.preference.set",
            "workspace.advisory.configure",
            "candidate.advisory.verify",
            "pipeline.open_effect.verify",
            "pipeline.phase_effect.verify",
            "pipeline.recommendation.prepare",
            "pipeline.recommendation.run",
            "pipeline.recommendation.disposition",
            "slice.pipeline.evidence_artifact.register",
            "slice.pipeline.evidence_artifact.finalize",
            "engineering.matrix.context.propose",
            "engineering.matrix.context.confirm",
            "task.source.record",
            "engineering.matrix.verify",
            "engineering.matrix.disposition.record"
        ])
    );
    let help = tools.iter().find(|tool| tool["name"] == "help").unwrap();
    assert_eq!(help["inputSchema"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(
        help["inputSchema"]["properties"]["method"]["enum"],
        json!([
            "tectd-program",
            "tectd-setup",
            "tectd-scope-candidates",
            "tectd-slice-candidates"
        ])
    );

    let before_help: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workspaces WHERE tenant_id=$1) + \
         (SELECT count(*) FROM agent_sessions WHERE tenant_id=$1) + \
         (SELECT count(*) FROM workspace_events WHERE tenant_id=$1)",
    )
    .bind(enrollment.tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before_help, 0);
    let searched = client
        .call(
            "help",
            json!({"mode":"search","text":"создать программу","tool":"command"}),
        )
        .await;
    assert_eq!(searched["hits"][0]["route"], "program.begin");
    let described = recovery_support::help_reads::describe(
        async |arguments| {
            client
                .exchange(
                    "tools/call",
                    recovery_support::public_call("help", arguments),
                )
                .await
        },
        json!({"mode":"describe","tool":"command","route":"program.begin"}),
    )
    .await
    .unwrap()
    .value;
    assert_eq!(
        described["params_schema"]["required"],
        json!(["request_id", "input"])
    );
    let knowledge = recovery_support::help_reads::describe(
        async |arguments| {
            client
                .exchange(
                    "tools/call",
                    recovery_support::public_call("help", arguments),
                )
                .await
        },
        json!({"mode":"describe","tool":"command","route":"knowledge.change_phase_complete"}),
    )
    .await
    .unwrap()
    .value;
    let output = &knowledge["params_schema"]["oneOf"][1]["properties"]["output"];
    assert_eq!(
        output["properties"]["data"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    let prepare = &output["properties"]["data"]["oneOf"][4]["properties"]["data"];
    let planned = &prepare["properties"]["operations"]["items"]["oneOf"];
    assert_eq!(planned.as_array().unwrap().len(), 5);
    assert_eq!(
        planned[0]["properties"]["document"]["properties"]["sections"]["properties"]["runbook"]["additionalProperties"],
        false
    );
    let method = recovery_support::help_reads::describe(
        async |arguments| {
            client
                .exchange(
                    "tools/call",
                    recovery_support::public_call("help", arguments),
                )
                .await
        },
        json!({"mode":"describe","method":"tectd-program"}),
    )
    .await
    .unwrap()
    .value;
    assert!(method["body"].as_str().unwrap().contains("# TectD Program"));
    let after_help: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workspaces WHERE tenant_id=$1) + \
         (SELECT count(*) FROM agent_sessions WHERE tenant_id=$1) + \
         (SELECT count(*) FROM workspace_events WHERE tenant_id=$1)",
    )
    .bind(enrollment.tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_help, 0, "static help must not bootstrap entity state");

    let unopened = client.call("get_state", json!({})).await;
    assert_eq!(unopened["status"], "uninitialized");
    assert_eq!(action_tools(&unopened), ["workspace.open"]);
    assert_eq!(unopened["recommended_action"], 0);
    assert_eq!(unopened["actions"][0]["kind"], "ready_call");
    assert!(unopened["programs"].as_array().unwrap().is_empty());
    let empty = client.call("open_workspace", json!({})).await;
    assert_eq!(empty["status"], "ready");
    assert!(empty["programs"].as_array().unwrap().is_empty());
    assert_eq!(action_tools(&empty).last(), Some(&"program.begin"));
    assert_eq!(empty["recommended_action"], 0);
    assert_eq!(
        empty["actions"].as_array().unwrap().last().unwrap()["kind"],
        "needs_input"
    );
    assert!(
        empty["actions"].as_array().unwrap().last().unwrap()["arguments"]["params"]["request_id"]
            .as_str()
            .is_some_and(|value| Uuid::parse_str(value).is_ok())
    );

    let mut ids = Vec::new();
    let first = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Program 00"}),
        )
        .await;
    let ready_id = Uuid::parse_str(first["program"]["id"].as_str().unwrap()).unwrap();
    ids.push(ready_id);
    let one = client.call("get_state", json!({})).await;
    assert_eq!(one["programs"].as_array().unwrap().len(), 1);
    assert_eq!(
        action_tools(&one),
        ["program.get", "setup.inspect", "program.begin"]
    );
    assert_eq!(one["recommended_action"], 0);
    complete(&mut client, ready_id).await;
    for index in 1..27 {
        let created = client
            .call(
                "begin_program",
                json!({
                    "request_id":Uuid::new_v4(),
                    "input":format!("Program {index:02}")
                }),
            )
            .await;
        ids.push(Uuid::parse_str(created["program"]["id"].as_str().unwrap()).unwrap());
    }

    let mut expected_ids: Vec<_> = ids.iter().copied().filter(|id| *id != ready_id).collect();
    expected_ids.sort();
    expected_ids.push(ready_id);
    let populated = client.call("get_state", json!({})).await;
    let n = populated["programs"].as_array().unwrap().len();
    assert!((1..=25).contains(&n));
    let mut delivered = summary_ids_and_actions(&populated, ready_id);
    assert_eq!(delivered, expected_ids[..n]);
    let tools = action_tools(&populated);
    assert_eq!(tools.len(), n + 3);
    assert_eq!(
        &tools[n..],
        ["program.list", "setup.inspect", "program.begin"]
    );
    assert_eq!(populated["recommended_action"], 0);
    let cursor = populated["next_after"].as_str().unwrap();
    assert!(cursor.starts_with("w:") && cursor.len() > 2);
    let mut continuation = populated["actions"][n].clone();
    assert_eq!(
        continuation["arguments"]["params"],
        json!({"after":cursor,"limit":25})
    );
    let mut previous_cursor = cursor.to_owned();
    let mut terminal = false;
    for _ in 0..27 {
        let response = client
            .exchange(
                "tools/call",
                json!({
                    "name":continuation["tool"],"arguments":continuation["arguments"]
                }),
            )
            .await;
        assert!(response.get("error").is_none() && response["result"]["isError"] != true);
        let page = tool_payload(&response);
        let page_ids = summary_ids_and_actions(&page, ready_id);
        assert!((1..=25).contains(&page_ids.len()));
        let count = page_ids.len();
        delivered.extend(page_ids);
        assert!(delivered.len() <= expected_ids.len());
        assert_eq!(delivered, expected_ids[..delivered.len()]);
        assert_eq!(page["recommended_action"], 0);
        let tools = action_tools(&page);
        if page["next_after"].is_null() {
            assert_eq!(&tools[count..], ["program.begin"]);
            terminal = true;
            break;
        }
        assert_eq!(&tools[count..], ["program.list", "program.begin"]);
        let next = page["next_after"].as_str().unwrap();
        assert!(next.starts_with("w:") && next.len() > 2);
        assert_ne!(next, previous_cursor);
        continuation = page["actions"][count].clone();
        assert_eq!(
            continuation["arguments"]["params"],
            json!({"after":next,"limit":25})
        );
        previous_cursor = next.to_owned();
    }
    assert!(terminal, "continuation did not terminate");
    assert_eq!(delivered, expected_ids);
    assert_eq!(delivered.iter().collect::<BTreeSet<_>>().len(), 27);

    let mut after: Option<String> = None;
    let mut ordered = Vec::new();
    loop {
        let mut arguments = Map::new();
        arguments.insert("limit".into(), json!(2));
        if let Some(cursor) = &after {
            arguments.insert("after".into(), json!(cursor));
        }
        let page = client.call("list_programs", Value::Object(arguments)).await;
        let programs = page["programs"].as_array().unwrap();
        ordered.extend(
            programs
                .iter()
                .map(|program| Uuid::parse_str(program["id"].as_str().unwrap()).unwrap()),
        );
        assert_eq!(action_tools(&page).last(), Some(&"program.begin"));
        if page["next_after"].is_null() {
            break;
        }
        let next = page["next_after"].as_str().unwrap().to_owned();
        let actions = action_tools(&page);
        assert_eq!(actions[programs.len()], "program.list");
        after = Some(next);
    }
    assert_eq!(ordered.len(), ids.len());
    assert_eq!(ordered.last(), Some(&ready_id));
    assert_eq!(ordered, expected_ids);

    let before_invalid: i64 =
        sqlx::query_scalar("SELECT count(*) FROM programs WHERE tenant_id=$1")
            .bind(enrollment.tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    for (name, arguments) in [
        (
            "begin_program",
            json!({"request_id":Uuid::nil(),"input":"x"}),
        ),
        (
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":" \t"}),
        ),
        (
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"actual\0nul"}),
        ),
        (
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"x","description":"forged"}),
        ),
        ("get_program", json!({"program_id":Uuid::nil()})),
        (
            "get_program",
            json!({"program_id":ready_id,"after_input":-1}),
        ),
        ("get_program", json!({"program_id":ready_id,"limit":0})),
        ("get_program", json!({"program_id":ready_id,"limit":"one"})),
        (
            "save_program",
            json!({"program_id":ready_id,"revision":2,"input_cursor":1,"status":"draft"}),
        ),
        (
            "record_program_input",
            json!({"program_id":ready_id,"request_id":Uuid::new_v4(),"input":"x","workspace":"forged"}),
        ),
        ("list_programs", json!({"after":"bad-cursor"})),
        ("read_skill", json!({"name":"../AGENTS.md"})),
    ] {
        let response = client
            .exchange("tools/call", public_call(name, arguments))
            .await;
        assert_rejected(&response);
    }
    for legacy in ["open_workspace", "get_program", "read_skill", "apply_setup"] {
        let response = client
            .exchange("tools/call", json!({"name":legacy,"arguments":{}}))
            .await;
        assert_rejected(&response);
    }
    let after_invalid: i64 = sqlx::query_scalar("SELECT count(*) FROM programs WHERE tenant_id=$1")
        .bind(enrollment.tenant_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after_invalid, before_invalid);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
