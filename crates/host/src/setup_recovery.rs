use crate::{responses, transport};
use serde_json::Value;
use std::path::Path;
use tect_domain::{Error, RequestContext};

/// Read-only context recovery fills known directory arguments after an authorized
/// setup refusal. It never inspects files or returns a protected draft in an error.
pub(crate) async fn response(
    error: Error,
    name: &str,
    arguments: &Value,
    socket: &Path,
    context: &RequestContext,
    capacity: usize,
) -> Value {
    let call = Some((name, arguments));
    let setup_tool = matches!(
        name,
        "inspect_setup"
            | "begin_setup"
            | "get_setup"
            | "save_setup"
            | "record_setup_input"
            | "apply_setup"
    );
    if !setup_tool || access_denial(error) || error == Error::WorkspaceNotOpen {
        return responses::failure(error, call);
    }
    match transport::call_tool_bounded(
        socket,
        context,
        "get_state",
        serde_json::json!({}),
        capacity,
    )
    .await
    {
        Ok(state) if state["status"] == "ready" => {
            responses::failure_with_state(error, call, Some(&state))
        }
        Err(current) if access_denial(current) => responses::failure(current, call),
        _ => responses::failure(error, call),
    }
}

fn access_denial(error: Error) -> bool {
    matches!(
        error,
        Error::Unauthorized
            | Error::InvalidNativeSession
            | Error::SessionRevoked
            | Error::SessionWorkspaceMismatch
            | Error::Forbidden
            | Error::InvalidConfiguration
    )
}
