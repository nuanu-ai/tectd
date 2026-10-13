use super::*;
use serde::Deserialize;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use tect_domain::{CompleteKnowledgeChangePhase, HostAuth};
use uuid::Uuid;

fn context() -> RequestContext {
    RequestContext {
        auth: HostAuth {
            host_id: Uuid::new_v4(),
            credential: "0".repeat(64),
        },
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "wire-version-test".into(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWireRequest {
    #[allow(dead_code)]
    context: RequestContext,
    #[allow(dead_code)]
    tool_name: String,
    #[allow(dead_code)]
    arguments: Value,
    #[allow(dead_code)]
    output_capacity: usize,
}

#[test]
fn new_daemon_rejects_old_or_wrong_wire_before_operation_decode() {
    let legacy = json!({
        "context":context(),"tool_name":"open_workspace","arguments":{},
        "output_capacity":MAX_FRAME_BYTES
    });
    let request: WireRequest = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        validate_wire_version(&request),
        Err(Error::InvalidConfiguration)
    );

    let wrong = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION + 1),
        context: context(),
        tool_name: "open_workspace".into(),
        arguments: json!({}),
        output_capacity: MAX_FRAME_BYTES,
    };
    assert_eq!(
        validate_wire_version(&wrong),
        Err(Error::InvalidConfiguration)
    );
}

#[test]
fn old_daemon_shape_rejects_new_bridge_request_before_operation_decode() {
    let current = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION),
        context: context(),
        tool_name: "open_workspace".into(),
        arguments: json!({}),
        output_capacity: MAX_FRAME_BYTES,
    };
    let encoded = serde_json::to_value(current).unwrap();
    assert!(serde_json::from_value::<LegacyWireRequest>(encoded).is_err());
}

#[tokio::test]
async fn daemon_deadline_is_typed_and_bridge_budget_has_write_margin() {
    assert_eq!(OPERATION_TIMEOUT, Duration::from_secs(45));
    assert_eq!(response_read_timeout_for(None), Duration::from_secs(55));
    assert!(response_read_timeout_for(None) > OPERATION_TIMEOUT + IO_TIMEOUT);
    let expired = timeout(
        Duration::from_millis(1),
        std::future::pending::<Result<Value>>(),
    )
    .await;
    let response = operation_response(expired, MAX_FRAME_BYTES);
    let encoded = encode_line(&response).unwrap();
    assert_eq!(decode_wire_response(&encoded), Err(Error::OperationTimeout));
    assert_eq!(Error::OperationTimeout.code(), "operation_timeout");
}

#[test]
fn validated_routes_select_bounded_operation_and_bridge_budgets() {
    let default = parse_invocation("get_state", json!({})).unwrap();
    assert_eq!(operation_timeout_for(&default), Duration::from_secs(45));
    assert_eq!(
        response_read_timeout_for(Some(&default)),
        Duration::from_secs(55)
    );

    let lexical = parse_invocation(
        "knowledge_search",
        json!({"mode":"lexical","query":"current evidence","purpose":"lookup"}),
    )
    .unwrap();
    assert_eq!(operation_timeout_for(&lexical), OPERATION_TIMEOUT);
    let super_wide = parse_invocation(
        "knowledge_search",
        json!({"mode":"super_wide","query":"current evidence","purpose":"lookup"}),
    )
    .unwrap();
    assert_eq!(operation_timeout_for(&super_wide), Duration::from_secs(60));
    assert_eq!(
        response_read_timeout_for(Some(&super_wide)),
        Duration::from_secs(70)
    );

    let id = Uuid::new_v4();
    let commit = parse_invocation(
        "knowledge_change_commit",
        json!({"request_id":id,"change_id":id,"run_id":id,"run_revision":1,
            "seal_id":id,"plan_revision":1,"plan_digest":"digest",
            "sealed_command_digest":"digest"}),
    )
    .unwrap();
    let publish = parse_invocation(
        "knowledge_change_publish",
        json!({"request_id":id,"change_id":id,"change_revision":1,
            "proposal_digest":"digest"}),
    )
    .unwrap();
    for invocation in [&commit, &publish] {
        assert_eq!(operation_timeout_for(invocation), Duration::from_secs(120));
        assert_eq!(
            response_read_timeout_for(Some(invocation)),
            Duration::from_secs(130)
        );
    }

    // Production supplies this variant only after validation; construct it here
    // to isolate the budget choice from the full phase output contract.
    let phase = |phase_id| {
        Invocation::KnowledgeLifecycle(KnowledgeLifecycleInvocation::PhaseComplete(Box::new(
            CompleteKnowledgeChangePhase {
                request_id: id,
                change_id: id,
                run_id: id,
                run_revision: 1,
                phase_id,
                output: None,
                revisit_phase_id: None,
            },
        )))
    };
    let impact = phase(KnowledgeChangePhaseId::KcImpactPlan);
    assert_eq!(operation_timeout_for(&impact), Duration::from_secs(120));
    assert_eq!(
        response_read_timeout_for(Some(&impact)),
        Duration::from_secs(130)
    );
    let other = phase(KnowledgeChangePhaseId::KcPublicationGate);
    assert_eq!(operation_timeout_for(&other), OPERATION_TIMEOUT);
    assert_eq!(
        response_read_timeout_for(Some(&other)),
        Duration::from_secs(55)
    );
}

/// One wait for Jev at the largest `TECT_JEV_TIMEOUT_MS` ends inside the operation
/// deadline of the commands that record input. Each of them asks Jev twice, `intake`
/// and then `model`, and two such waits do not: a Jev that answers neither call makes
/// the command, which has recorded its input by then, answer `operation_timeout`.
#[test]
fn two_jev_waits_at_the_largest_timeout_outlast_the_deadline_of_an_input_command() {
    let id = Uuid::new_v4();
    let scope_input = parse_invocation(
        "record_candidate_input",
        json!({"candidate_set_id":id,"revision":1,"request_id":id,"input":"i"}),
    )
    .unwrap();
    let slice_input = parse_invocation(
        "record_slice_candidate_input",
        json!({"scope_id":id,"candidate_set_id":id,"revision":1,"request_id":id,"input":"i"}),
    )
    .unwrap();
    let wait = Duration::from_millis(crate::jev_transport::MAX_TIMEOUT_MS);
    for command in [&scope_input, &slice_input] {
        let deadline = operation_timeout_for(command);
        assert_eq!(deadline, OPERATION_TIMEOUT);
        assert!(wait < deadline);
        assert!(2 * wait > deadline);
    }
    // Two waits fit the deadline up to this setting of the timeout.
    assert_eq!(OPERATION_TIMEOUT.as_millis() / 2, 22_500);
}

#[tokio::test]
async fn bridge_accepts_success_from_daemon_after_previous_fifteen_second_limit() {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().canonicalize().unwrap().join("tectd.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = FrameReader::new(read);
        assert!(matches!(reader.next().await.unwrap(), Some(Frame::Data(_))));
        tokio::time::sleep(Duration::from_secs(16)).await;
        write
            .write_all(
                &encode_line(&WireResponse::Ok {
                    result: json!({"delivered":"after 16 seconds"}),
                })
                .unwrap(),
            )
            .await
            .unwrap();
    });
    assert_eq!(
        call_tool(&socket, &context(), "get_state", json!({})).await,
        Ok(json!({"delivered":"after 16 seconds"}))
    );
    daemon.await.unwrap();
}

#[tokio::test]
async fn delayed_daemon_response_preserves_wire_error_and_socket_absence_is_transport_error() {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().canonicalize().unwrap().join("tectd.sock");
    assert_eq!(
        call_tool(&socket, &context(), "get_state", json!({})).await,
        Err(Error::TransportUnavailable)
    );

    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = FrameReader::new(read);
        assert!(matches!(reader.next().await.unwrap(), Some(Frame::Data(_))));
        tokio::time::sleep(Duration::from_millis(30)).await;
        let bytes = encode_line(&WireResponse::Error {
            error: Error::OperationTimeout,
        })
        .unwrap();
        write.write_all(&bytes).await.unwrap();
    });
    assert_eq!(
        call_tool(&socket, &context(), "get_state", json!({})).await,
        Err(Error::OperationTimeout)
    );
    daemon.await.unwrap();
}
