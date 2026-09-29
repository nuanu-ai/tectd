use super::*;
use std::path::{Path, PathBuf};

pub(super) async fn run(
    client: Mcp,
    mut daemon: Daemon,
    pool: &PgPool,
    runtime: &str,
    socket: PathBuf,
    config: &Path,
    native: &str,
    key: &str,
    amendment: Value,
    persisted_session_id: Uuid,
    definition_digest_before_amendment: Value,
    phase_five_binding: Value,
    phase_five_output: Value,
) {
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    let mut context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":amendment["run_id"]}),
    )
    .await;
    let cold_input = context["inputs"].as_array().unwrap().last().unwrap();
    let cold_amendment = &cold_input["source_amendment"];
    assert_eq!(cold_input["input"], amendment["input"]);
    assert_eq!(
        cold_amendment["authorization_provenance"],
        "exact direct operator instruction persisted in input"
    );
    assert_eq!(
        cold_amendment["successor"]["artifact"]["body"],
        amendment["source_amendment"]["successor"]["artifact"]["body"]
    );
    let successor_path = cold_amendment["successor"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor_digest = cold_amendment["successor"]["artifact"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor_artifact = cold_amendment["successor"]["artifact"].clone();
    assert_eq!(
        context["run"]["definition_digest"],
        definition_digest_before_amendment
    );
    for binding in context["bindings"].as_array().unwrap() {
        if binding["phase_ordinal"].as_u64().unwrap() >= 5 {
            assert_eq!(binding["stale"], true);
            assert_eq!(binding["stale_reason"], "source_amendment");
        }
    }
    let old_phase_five = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"view":"output",
            "output_id":phase_five_binding["output_id"],"digest":phase_five_binding["output_digest"]}),
    )
    .await;
    assert_eq!(old_phase_five["stale"], true);
    for field in [
        "id",
        "run_id",
        "phase_id",
        "phase_ordinal",
        "revision",
        "body",
        "producer_context_id",
        "digest",
        "reference",
        "fields",
        "verdict",
        "dispositions",
        "skill_reads",
        "resource_reads",
        "artifacts",
        "validator_receipts",
        "followup_proposal",
    ] {
        assert_eq!(old_phase_five[field], phase_five_output[field], "{field}");
    }

    let mut conflict = amendment.clone();
    conflict["input"] = json!("Conflicting replay text");
    assert_eq!(
        route_error(&mut client, "command", "slice.pipeline.input", conflict).await["error"]["code"],
        "input_conflict"
    );
    let mut stale_predecessor = amendment.clone();
    stale_predecessor["request_id"] = json!(Uuid::new_v4());
    stale_predecessor["run_revision"] = context["run"]["revision"].clone();
    stale_predecessor["phase_id"] = context["run"]["current_phase_id"].clone();
    let stale_predecessor_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        stale_predecessor,
    )
    .await;
    assert_eq!(
        stale_predecessor_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        stale_predecessor_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let (verdict, outcome, transition) = successful_route(&context);
    let wrong_lineage_phase_five = completion(&context, verdict, outcome, transition, None, None);
    let wrong_lineage_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        wrong_lineage_phase_five,
    )
    .await;
    assert_eq!(
        wrong_lineage_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        wrong_lineage_error["error"]["refusal"]["rule"],
        "WP6-COMPLETE-01"
    );
    let after_wrong_lineage = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
    )
    .await;
    for collection in ["run", "attempts", "outputs", "bindings", "inputs"] {
        assert_eq!(after_wrong_lineage[collection], context[collection]);
    }
    context = after_wrong_lineage;
    context = refresh_knowledge(&mut client, &context).await;
    let (verdict, outcome, transition) = successful_route(&context);

    let mut amended_phase_five = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger_source(
        &mut amended_phase_five["output"],
        &successor_path,
        &successor_digest,
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        amended_phase_five,
    )
    .await["context"]
        .clone();
    context = advance(&mut client, context).await;
    let (verdict, outcome, transition) = successful_route(&context);
    let mut amended_phase_seven = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger_source(
        &mut amended_phase_seven["output"],
        &successor_path,
        &successor_digest,
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        amended_phase_seven,
    )
    .await["context"]
        .clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-implementation-spec-synthesizer"
    );
    let fresh_phase_five = context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| {
            output["phase_id"] == "slice-component-decision-interrogator"
                && output["stale"] == false
        })
        .unwrap();
    let fresh_ledger: Value = serde_json::from_str(
        fresh_phase_five["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["name"] == "requirements-ledger.json")
            .unwrap()["body"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fresh_ledger["source"]["path"], successor_path);
    assert_eq!(fresh_ledger["source"]["digest"], successor_digest);

    let fresh_phase_five_binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap();
    let fresh_phase_five_artifact = fresh_phase_five["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let no_op = json!({
        "request_id":Uuid::new_v4(),
        "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "input":"Direct operator instruction for a no-op amendment rejection check.",
        "source_amendment":{
            "target_phase_id":"slice-component-decision-interrogator",
            "predecessor":{
                "output_id":fresh_phase_five_binding["output_id"],
                "output_revision":fresh_phase_five_binding["output_revision"],
                "output_digest":fresh_phase_five_binding["output_digest"],
                "artifact_name":"requirements-ledger.json",
                "artifact_digest":fresh_phase_five_artifact["digest"],
                "source_path":fresh_ledger["source"]["path"],
                "source_digest":fresh_ledger["source"]["digest"]
            },
            "successor":{"path":successor_path,"artifact":successor_artifact},
            "authorization_scope":"amend the current Full Design source",
            "authorization_provenance":"exact direct operator input"
        }
    });
    let before_no_op = context.clone();
    let no_op_error = route_error(&mut client, "command", "slice.pipeline.input", no_op).await;
    assert_eq!(no_op_error["error"]["refusal"]["code"], "INVALID_OUTPUT");
    assert_eq!(no_op_error["error"]["refusal"]["rule"], "WP6-INPUT-01");
    let after_no_op = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
    )
    .await;
    for collection in ["run", "inputs", "bindings"] {
        assert_eq!(after_no_op[collection], before_no_op[collection]);
    }
    assert_eq!(
        after_no_op["run"]["definition_digest"],
        definition_digest_before_amendment
    );

    let synthesis = advance(&mut client, after_no_op).await;
    let synthesis_output = synthesis["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["phase_id"] == "slice-implementation-spec-synthesizer")
        .unwrap()
        .clone();
    assert_eq!(synthesis_output["artifacts"].as_array().unwrap().len(), 6);
    assert_eq!(
        synthesis_output["validator_receipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    context = synthesis;

    let run_id = context["run"]["id"].clone();
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let _restarted_daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(context["outputs_complete"], true);
    let binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-implementation-spec-synthesizer")
        .unwrap();
    let exact = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":binding["output_id"],
            "digest":binding["output_digest"]}),
    )
    .await;
    assert_eq!(exact["artifacts"], synthesis_output["artifacts"]);
    assert_eq!(
        exact["validator_receipts"],
        synthesis_output["validator_receipts"]
    );
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":run_id,"view":"output","output_id":binding["output_id"],
            "digest":"wrong"})
        )
        .await["error"]["code"],
        "not_found"
    );

    while context["run"]["current_phase_id"] != "slice-human-decision-queue-manager" {
        context = advance(&mut client, context).await;
    }
    let waiting = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "blocked_missing_authority",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await;
    context = waiting["context"].clone();
    assert_eq!(context["run"]["status"], "waiting_input");
    context = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
            "input":"Recorded authority and bounded resume evidence."}),
    )
    .await["context"]
        .clone();
    context = refresh_knowledge(&mut client, &context).await;
    context = advance(&mut client, context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 21 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let completed = route(&mut client,"command","slice.pipeline.phase.complete",
        completion(&context,verdict,outcome,transition,None,Some(json!({
            "summary":"Caller reports Full Slice completion after exact phase contracts.",
            "evidence":[{"kind":"integration_test","reference":"pipeline_execution_full.rs",
                "observation":"Twenty-one phases, rework, review, validators and cold retrieval completed."}],
            "scope_impact":"Refresh future planning once.","remaining_work":"No remaining work in this Slice."
        })))).await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed["result"]["pipeline_definition_digest"],
        "1274c531dfd433bf01e6b2354adcd0082c906749e1c8e34a158604f77e77a9a5"
    );
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        32
    );
    assert_eq!(
        completed["context"]["run"]["definition_digest"],
        definition_digest_before_amendment
    );
    admin::revoke_session(pool, persisted_session_id)
        .await
        .unwrap();
    assert_eq!(
        route_error(&mut client, "command", "slice.pipeline.input", amendment).await["error"]["code"],
        "session_revoked"
    );
}
