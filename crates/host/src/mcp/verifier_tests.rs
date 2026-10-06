use super::*;

#[test]
fn verifier_malformed_public_routes_keep_their_authentication_target() {
    for (tool, route, expected) in [
        (
            "command",
            "candidate.advisory.verify",
            "candidate_advisory_verify",
        ),
        ("query", "candidate.advisory.get", "candidate_advisory_get"),
        (
            "query",
            "candidate.advisory.audit",
            "candidate_advisory_audit",
        ),
        ("command", "engineering.matrix.verify", "verify_matrix_task"),
        (
            "query",
            "engineering.matrix.disposition.get",
            "get_matrix_disposition",
        ),
        (
            "query",
            "pipeline.open_effect.get",
            "get_pipeline_open_effect",
        ),
        (
            "command",
            "pipeline.open_effect.verify",
            "verify_pipeline_open_effect",
        ),
        (
            "query",
            "pipeline.phase_effect.get",
            "get_pipeline_phase_effect",
        ),
        (
            "command",
            "pipeline.phase_effect.verify",
            "verify_pipeline_phase_effect",
        ),
    ] {
        assert_eq!(
            crate::api::recognized_internal_name(tool, &json!({"route":route})),
            Some(expected)
        );
        for arguments in [
            json!({"route":route,"params":null}),
            json!({"route":route,"params":{},"forged":true}),
        ] {
            assert!(crate::api::decode_public_call(tool, arguments.clone()).is_err());
            assert_eq!(malformed_public_route(tool, &arguments), expected);
        }
    }
    assert_eq!(
        malformed_public_route("query", &json!({"route":"candidate.advisory.verify"})),
        crate::api::INVALID_PUBLIC_CALL
    );
    for (tool, arguments) in [
        ("query", json!({"route":"engineering.matrix.verify"})),
        (
            "command",
            json!({"route":"engineering.matrix.disposition.get"}),
        ),
        ("command", json!({"route":"pipeline.open_effect.get"})),
        ("query", json!({"route":"pipeline.open_effect.verify"})),
        ("command", json!({"route":"pipeline.phase_effect.get"})),
        ("query", json!({"route":"pipeline.phase_effect.verify"})),
        ("command", json!({"route":7})),
        ("command", json!({"route":false})),
        ("command", json!({"route":[]})),
        ("command", json!({"route":{}})),
        ("command", json!({})),
        ("command", json!({"route":"unknown"})),
    ] {
        assert_eq!(
            malformed_public_route(tool, &arguments),
            crate::api::INVALID_PUBLIC_CALL
        );
    }
    assert_eq!(
        malformed_public_route("command", &json!({"route":null})),
        crate::api::INVALID_PUBLIC_CALL
    );
}

#[test]
fn verifier_decode_diagnostic_never_overrides_a_daemon_access_refusal() {
    for daemon in [
        Error::Forbidden,
        Error::Unauthorized,
        Error::SessionRevoked,
        Error::SessionWorkspaceMismatch,
        Error::WorkspaceNotOpen,
        Error::OperationTimeout,
    ] {
        assert_eq!(
            preserve_daemon_refusal(Some(Error::InvalidArguments), daemon.clone()),
            daemon
        );
    }
    assert_eq!(
        preserve_daemon_refusal(
            Some(Error::invalid_arguments_from("decode diagnostic")),
            Error::InvalidArguments
        ),
        Error::invalid_arguments_from("decode diagnostic")
    );
    assert_eq!(
        preserve_daemon_refusal(None, Error::InvalidArguments),
        Error::InvalidArguments
    );
    assert_eq!(
        preserve_daemon_refusal(
            Some(Error::InvalidArguments),
            Error::invalid_arguments_from("daemon diagnostic")
        ),
        Error::invalid_arguments_from("daemon diagnostic")
    );
}
