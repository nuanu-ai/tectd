use super::*;

fn payload(error: Error) -> Value {
    let arguments = json!({"route":"candidate.advisory.verify","params":{}});
    let value = failure(error, Some(("command", &arguments)));
    serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn verifier_access_refusals_do_not_attach_argument_route_contracts() {
    for error in [
        Error::Forbidden,
        Error::Unauthorized,
        Error::SessionRevoked,
        Error::SessionWorkspaceMismatch,
        Error::WorkspaceNotOpen,
        Error::OperationTimeout,
    ] {
        let code = error.code();
        let value = payload(error);
        assert_eq!(value["error"]["code"], code);
        assert!(value["error"].get("route_contract").is_none());
        assert!(value["error"].get("route").is_none());
        assert!(value["error"].get("tool").is_none());
        assert!(value["error"].get("schema_help").is_none());
    }
}

#[test]
fn verifier_argument_failures_expose_only_the_strict_six_field_contract() {
    for error in [
        Error::InvalidArguments,
        Error::invalid_arguments_from("invalid target"),
    ] {
        let value = payload(error);
        assert!(value["error"].get("route_contract").is_none());
        let help = &value["error"]["schema_help"];
        assert_eq!(help["tool"], "help");
        assert_eq!(help["arguments"]["tool"], "command");
        assert_eq!(help["arguments"]["route"], "candidate.advisory.verify");
        crate::api::decode_public_call("help", help["arguments"].clone()).unwrap();
        let described =
            crate::api::help(crate::api::parse_help(help["arguments"].clone()).unwrap()).unwrap();
        let schema = &described["params_schema"];
        assert_eq!(value["error"]["route"], "candidate.advisory.verify");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"].as_array().unwrap().len(), 6);
        assert_eq!(schema["properties"].as_object().unwrap().len(), 6);
        assert!(schema["properties"].get("actor_id").is_none());
        assert!(schema["properties"].get("session_id").is_none());
    }
}
