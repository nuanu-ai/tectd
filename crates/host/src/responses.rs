use serde_json::{Value, json};
use tect_domain::Error;

pub(crate) const INTROS: [&str; 8] = [
    "This native session has no open workspace. Open it to continue.",
    "No Programs are registered in this workspace. Create one from your narrative.",
    "Continue an existing Program or start another using the actions below.",
    "Program is saved. Read its formation skill and required original input before composing the next revision.",
    "Program and pending question are saved. Ask that question, then record the original reply.",
    "Program is open. Present the whole saved PRD; record corrections when provided.",
    "Use this method with the current Program calls.",
    "Current data and available calls are below.",
];

pub(crate) fn action(tool: &str, arguments: Value) -> Value {
    json!({"tool":tool,"arguments":arguments})
}

pub(crate) fn with_actions(
    mut data: Value,
    actions: Vec<Value>,
    recommended: Option<usize>,
) -> Value {
    data["actions"] = json!(actions);
    data["recommended_action"] = json!(recommended);
    data
}

fn intro(data: &Value) -> &'static str {
    if data["name"] == "tectd-program" && data.get("body").is_some() {
        return INTROS[6];
    }
    if let Some(step) = data["program"]["current_step"].as_str() {
        return match step {
            "compose" => INTROS[3],
            "waiting_input" => INTROS[4],
            "ready" => INTROS[5],
            _ => INTROS[7],
        };
    }
    if data["status"] == "uninitialized" {
        return INTROS[0];
    }
    if let Some(programs) = data["programs"].as_array() {
        return if programs.is_empty() {
            INTROS[1]
        } else {
            INTROS[2]
        };
    }
    INTROS[7]
}

pub(crate) fn success(data: Value) -> Value {
    content(intro(&data), data, false)
}

fn content(intro: &'static str, data: Value, is_error: bool) -> Value {
    json!({
        "content":[{"type":"text","text":intro},
            {"type":"text","text":serde_json::to_string(&data).expect("JSON value")}],
        "isError":is_error
    })
}

pub(crate) fn encoded_len(data: &Value) -> tect_domain::Result<usize> {
    serde_json::to_vec(&success(data.clone()))
        .map(|bytes| bytes.len())
        .map_err(|_| Error::TransportUnavailable)
}

pub(crate) fn failure(error: Error, call: Option<(&str, &Value)>) -> Value {
    let mut actions = Vec::new();
    let reload = call
        .and_then(|(_, args)| args.get("program_id"))
        .filter(|id| id.is_string())
        .map(|id| action("get_program", json!({"program_id":id})));
    match error {
        Error::WorkspaceNotOpen => actions.push(action("open_workspace", json!({}))),
        Error::StaleRevision
        | Error::InputPending
        | Error::ProgramIncomplete
        | Error::InputConflict
        | Error::RequestTooLarge => {
            actions.push(reload.unwrap_or_else(|| action("get_state", json!({}))));
        }
        Error::StorageUnavailable | Error::TransportUnavailable => {
            if let Some((name, arguments)) = call {
                if name == "save_program" {
                    actions.push(reload.unwrap_or_else(|| action("get_state", json!({}))));
                } else {
                    actions.push(action(name, arguments.clone()));
                }
            }
        }
        Error::Unauthorized
        | Error::InvalidNativeSession
        | Error::SessionRevoked
        | Error::SessionWorkspaceMismatch
        | Error::Forbidden
        | Error::InvalidConfiguration => {}
        _ => actions.push(action("get_state", json!({}))),
    }
    let recommended = (!actions.is_empty()).then_some(0);
    let data = with_actions(json!({"error":{"code":error.code()}}), actions, recommended);
    content(error_intro(error), data, true)
}

pub(crate) fn error_intro(error: Error) -> &'static str {
    match error {
        Error::StaleRevision => {
            "A newer revision exists. Reload the Program and merge before saving."
        }
        Error::InputPending => {
            "The input cursor is not current. Read the remaining original input before completing."
        }
        Error::ProgramIncomplete => {
            "Keep every required PRD field meaningful and resolve the pending question before completion."
        }
        Error::InputConflict => {
            "This request identity already belongs to different original input. Reload; use a new request identity for a new input."
        }
        Error::WorkspaceNotOpen => "Open this native session's workspace before continuing.",
        Error::RequestTooLarge => {
            "The encoded request or response exceeds transport capacity. Reload current state before retrying."
        }
        Error::StorageUnavailable | Error::TransportUnavailable => {
            "The result is uncertain. Recover with the exact call below; do not create a replacement Program."
        }
        Error::InvalidArguments => {
            "The arguments do not match the current tool schema. Read current state and use the live schema."
        }
        _ => {
            "The request cannot proceed with the current identity or access. No protected data is included."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_intro_has_a_budget_and_json_is_not_mirrored() {
        for intro in INTROS {
            assert!(intro.len() <= 2000);
        }
        for error in [
            Error::StaleRevision,
            Error::InputPending,
            Error::ProgramIncomplete,
            Error::InputConflict,
            Error::WorkspaceNotOpen,
            Error::RequestTooLarge,
            Error::StorageUnavailable,
            Error::TransportUnavailable,
            Error::InvalidArguments,
            Error::Unauthorized,
        ] {
            assert!(error_intro(error).len() <= 2000);
        }
        let large = "narrative".repeat(10_000);
        let response = success(json!({"large":large}));
        assert!(response.get("structuredContent").is_none());
        assert_eq!(response["content"].as_array().unwrap().len(), 2);
        let parsed: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["large"], large);
    }
}
