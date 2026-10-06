use super::*;
use serde_json::json;

fn arguments() -> Value {
    let id = uuid::Uuid::new_v4();
    json!({"request_id":id,"opportunity_id":id,"candidate_set_id":id,
        "caller_link_id":id,"caller_receipt_request_id":id,"target_revision":1})
}

#[test]
fn verifier_parser_binds_only_six_target_fields() {
    let input = arguments();
    let Ok(AdvisoryInvocation::VerifySelectedSave(parsed)) =
        parse("candidate_advisory_verify", input.clone())
    else {
        panic!("valid verifier target rejected")
    };
    assert_eq!(
        serde_json::to_value(parsed.request_id).unwrap(),
        input["request_id"]
    );
    assert_eq!(parsed.target_revision, 1);
    for field in [
        "actor_id",
        "session_id",
        "workspace_id",
        "qualification",
        "status",
        "evidence_digest",
        "execution_state",
        "approval_granted",
        "forged",
    ] {
        let mut changed = input.clone();
        changed[field] = json!("client-controlled");
        assert!(
            parse("candidate_advisory_verify", changed).is_err(),
            "{field}"
        );
    }
    for field in [
        "request_id",
        "opportunity_id",
        "candidate_set_id",
        "caller_link_id",
        "caller_receipt_request_id",
    ] {
        let mut changed = input.clone();
        changed[field] = json!(uuid::Uuid::nil());
        assert!(
            parse("candidate_advisory_verify", changed).is_err(),
            "{field}"
        );
    }
    for revision in [json!(0), json!(-1), json!(null), json!("1")] {
        let mut changed = input.clone();
        changed["target_revision"] = revision;
        assert!(parse("candidate_advisory_verify", changed).is_err());
    }
    assert!(parse("candidate_advisory_verify", json!({})).is_err());
}

#[test]
fn verifier_route_is_one_command_inside_five_tools() {
    let definitions = crate::api::definitions();
    let tools = definitions["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    let command = tools.iter().find(|tool| tool["name"] == "command").unwrap();
    let query = tools.iter().find(|tool| tool["name"] == "query").unwrap();
    let routes = command["inputSchema"]["properties"]["route"]["enum"]
        .as_array()
        .unwrap();
    assert_eq!(routes.len(), 50);
    assert_eq!(
        routes
            .iter()
            .filter(|route| **route == "candidate.advisory.verify")
            .count(),
        1
    );
    assert_eq!(
        query["inputSchema"]["properties"]["route"]["enum"]
            .as_array()
            .unwrap()
            .len(),
        28
    );
    let input = json!({"route":"candidate.advisory.verify","params":arguments()});
    let call = crate::api::decode_public_call("command", input.clone()).unwrap();
    assert_eq!(call.name, "candidate_advisory_verify");
    assert!(matches!(
        crate::tools::parse_invocation(call.name, call.arguments),
        Ok(crate::tools::Invocation::Advisory(
            AdvisoryInvocation::VerifySelectedSave(_)
        ))
    ));
    assert!(crate::api::decode_public_call("query", input).is_err());
}
