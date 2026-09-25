//! Public recommendation reads and caller disposition; neither executes a model.
use super::*;
use std::path::Path;

pub(super) async fn assert_different_session_conflicts(
    socket: &Path,
    config: &Path,
    workspace_key: &str,
    key: &str,
    calls: &Arc<AtomicUsize>,
) {
    let mut other_session =
        Mcp::start(socket, config, &Uuid::new_v4().to_string(), workspace_key).await;
    other_session.call("open_workspace", json!({})).await;
    let denied = route_error(
        &mut other_session,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(denied["error"]["code"], "input_conflict");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    other_session.finish().await;
}

pub(super) async fn assert_public_get_disposition_and_denial(
    owner: &mut Mcp,
    verifier: &mut Mcp,
    key: &str,
    run: &Value,
    calls: &Arc<AtomicUsize>,
) {
    let get_args = json!({"preparation_request_key":key});
    let before = route(owner, "query", "model.route.get", get_args.clone()).await;
    assert_eq!(before["decision"]["id"], run["decision"]["id"]);
    assert_eq!(before["attempt"]["state"], "parsed");
    assert_eq!(
        before["decision"]["routes"]["requested_route_id"],
        "route-a"
    );
    assert_eq!(
        before["decision"]["routes"]["recommended_route_id"],
        "route-b"
    );
    assert!(before["decision"]["routes"]["observed_actual"].is_null());
    assert!(before["disposition"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // The verifier is intentionally a different principal, not a second Owner.
    for (tool, name, args) in [
        ("query", "model.route.get", get_args.clone()),
        ("command", "model.route.run", get_args.clone()),
    ] {
        let denied = route_error(verifier, tool, name, args).await;
        assert_eq!(denied["error"]["code"], "forbidden");
    }

    let disposition_id = Uuid::new_v4();
    let disposition_args = json!({
        "disposition_id":disposition_id,"decision_id":run["decision"]["id"],
        "action":"accept","rationale":"Synthetic recommendation accepted without execution"
    });
    let denied = route_error(
        verifier,
        "command",
        "model.route.disposition",
        disposition_args.clone(),
    )
    .await;
    assert_eq!(denied["error"]["code"], "forbidden");

    let accepted = route(
        owner,
        "command",
        "model.route.disposition",
        disposition_args.clone(),
    )
    .await;
    assert_eq!(accepted["id"], json!(disposition_id));
    assert_eq!(accepted["decision_id"], run["decision"]["id"]);
    assert_eq!(accepted["action"], "Accept");
    assert_eq!(
        route(
            owner,
            "command",
            "model.route.disposition",
            disposition_args.clone()
        )
        .await,
        accepted
    );
    let mut conflict = disposition_args;
    conflict["action"] = json!("reject");
    let denied = route_error(owner, "command", "model.route.disposition", conflict).await;
    assert_eq!(denied["error"]["code"], "input_conflict");

    let after = route(owner, "query", "model.route.get", get_args).await;
    for field in [
        "id",
        "decision_id",
        "workspace_id",
        "actor_id",
        "action",
        "rationale",
    ] {
        assert_eq!(after["disposition"][field], accepted[field]);
    }
    assert_eq!(after["decision"]["id"], run["decision"]["id"]);
    assert!(after["decision"]["routes"]["observed_actual"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
