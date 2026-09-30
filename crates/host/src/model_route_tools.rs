use serde::Deserialize;
use serde_json::Value;
use tect_application::{ModelRouteDispositionAction, PrepareModelRouteRecommendation};
use tect_domain::{AdvisoryRequestPreference, Error, Result};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) enum ModelRouteInvocation {
    HostSelection(tect_application::PrepareModelRouteHostSelection),
    Prepare(PrepareModelRouteRecommendation),
    Run {
        preparation_request_key: String,
    },
    Get {
        preparation_request_key: String,
    },
    Disposition {
        disposition_id: Uuid,
        decision_id: Uuid,
        action: ModelRouteDispositionAction,
        rationale: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareArguments {
    disposition_id: Uuid,
    expected_task_id: Uuid,
    expected_task_revision: i64,
    expected_candidate_set_id: Uuid,
    expected_caller_request_id: Uuid,
    expected_mapped_work_node_id: Uuid,
    expected_mapped_work_node_revision: i64,
    request_key: String,
    requested_route_id: Option<String>,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSelectionArguments {
    preparation_request_key: String,
    decision_id: Uuid,
    disposition_id: Uuid,
    expected_task_id: Uuid,
    expected_task_revision: i64,
    expected_work_context_digest: String,
    expected_catalogue_digest: String,
    selected_route_id: String,
    input_sha256: String,
    invocation_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyArguments {
    preparation_request_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Accept,
    Reject,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispositionArguments {
    disposition_id: Uuid,
    decision_id: Uuid,
    action: Action,
    rationale: String,
}

fn valid_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0') && value.trim() == value
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<ModelRouteInvocation> {
    match name {
        "prepare_model_route_host_selection" => {
            let a: HostSelectionArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let key = |s: &str, limit: usize| {
                !s.is_empty()
                    && s.len() <= limit
                    && s.trim() == s
                    && !s.chars().any(char::is_control)
            };
            let sha = |s: &str| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            };
            if [a.decision_id, a.disposition_id, a.expected_task_id]
                .iter()
                .any(Uuid::is_nil)
                || a.expected_task_revision < 1
                || !key(&a.preparation_request_key, 256)
                || !key(&a.selected_route_id, 128)
                || !key(&a.invocation_key, 256)
                || !sha(&a.expected_work_context_digest)
                || !sha(&a.expected_catalogue_digest)
                || !sha(&a.input_sha256)
            {
                return Err(Error::InvalidArguments);
            }
            Ok(ModelRouteInvocation::HostSelection(
                tect_application::PrepareModelRouteHostSelection {
                    preparation_request_key: a.preparation_request_key,
                    decision_id: a.decision_id,
                    disposition_id: a.disposition_id,
                    expected_task_id: a.expected_task_id,
                    expected_task_revision: a.expected_task_revision,
                    expected_work_context_digest: a.expected_work_context_digest,
                    expected_catalogue_digest: a.expected_catalogue_digest,
                    selected_route_id: a.selected_route_id,
                    input_sha256: a.input_sha256,
                    invocation_key: a.invocation_key,
                },
            ))
        }
        "model_route_prepare" => {
            if arguments
                .get("requested_route_id")
                .is_some_and(Value::is_null)
            {
                return Err(Error::InvalidArguments);
            }
            let a: PrepareArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if [
                a.disposition_id,
                a.expected_task_id,
                a.expected_candidate_set_id,
                a.expected_caller_request_id,
                a.expected_mapped_work_node_id,
            ]
            .iter()
            .any(Uuid::is_nil)
                || a.expected_task_revision < 1
                || a.expected_mapped_work_node_revision < 1
                || !valid_key(&a.request_key)
                || a.requested_route_id.as_ref().is_some_and(|r| !valid_key(r))
            {
                return Err(Error::InvalidArguments);
            }
            Ok(ModelRouteInvocation::Prepare(
                PrepareModelRouteRecommendation {
                    workspace_id: Uuid::nil(), // filled only from authenticated session
                    disposition_id: a.disposition_id,
                    expected_task_id: a.expected_task_id,
                    expected_task_revision: a.expected_task_revision,
                    expected_candidate_set_id: a.expected_candidate_set_id,
                    expected_caller_request_id: a.expected_caller_request_id,
                    expected_mapped_work_node_id: a.expected_mapped_work_node_id,
                    expected_mapped_work_node_revision: a.expected_mapped_work_node_revision,
                    request_key: a.request_key,
                    requested_route_id: a.requested_route_id,
                    origin_session_id: None,
                    session_preference: AdvisoryRequestPreference::UseWorkspace,
                    request_preference: a.request_preference,
                },
            ))
        }
        "model_route_run" | "model_route_get" => {
            let a: KeyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if !valid_key(&a.preparation_request_key) {
                return Err(Error::InvalidArguments);
            }
            if name == "model_route_run" {
                Ok(ModelRouteInvocation::Run {
                    preparation_request_key: a.preparation_request_key,
                })
            } else {
                Ok(ModelRouteInvocation::Get {
                    preparation_request_key: a.preparation_request_key,
                })
            }
        }
        "model_route_disposition" => {
            let a: DispositionArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if a.disposition_id.is_nil()
                || a.decision_id.is_nil()
                || a.rationale.trim().is_empty()
                || a.rationale.len() > 4096
                || a.rationale.contains('\0')
            {
                return Err(Error::InvalidArguments);
            }
            Ok(ModelRouteInvocation::Disposition {
                disposition_id: a.disposition_id,
                decision_id: a.decision_id,
                action: match a.action {
                    Action::Accept => ModelRouteDispositionAction::Accept,
                    Action::Reject => ModelRouteDispositionAction::Reject,
                },
                rationale: a.rationale,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_selection_is_strict_pins_only_and_never_accepts_sender_material() {
        let id = Uuid::new_v4();
        let args = json!({"preparation_request_key":"route-1","decision_id":id,"disposition_id":id,
            "expected_task_id":id,"expected_task_revision":1,"expected_work_context_digest":"a".repeat(64),
            "expected_catalogue_digest":"b".repeat(64),"selected_route_id":"route-a","input_sha256":"c".repeat(64),"invocation_key":"invocation-1"});
        assert!(matches!(
            parse("prepare_model_route_host_selection", args.clone()),
            Ok(ModelRouteInvocation::HostSelection(_))
        ));
        for field in [
            "workspace_id",
            "native_session_id",
            "provider",
            "model",
            "effort",
            "material",
            "authorized_selection",
            "sender_path",
        ] {
            let mut bad = args.clone();
            bad[field] = json!("forged");
            assert!(parse("prepare_model_route_host_selection", bad).is_err());
        }
        for (field, value) in [
            ("expected_task_revision", json!(0)),
            ("input_sha256", json!("A".repeat(64))),
            ("selected_route_id", json!("route\n")),
            ("decision_id", json!(Uuid::nil())),
        ] {
            let mut bad = args.clone();
            bad[field] = value;
            assert!(parse("prepare_model_route_host_selection", bad).is_err());
        }
    }

    #[test]
    fn public_route_parser_rejects_ranks_unknowns_and_forged_workspace() {
        let id = Uuid::new_v4();
        let prepared = json!({"disposition_id":id,"expected_task_id":id,
            "expected_task_revision":1,"expected_candidate_set_id":id,
            "expected_caller_request_id":id,"expected_mapped_work_node_id":id,
            "expected_mapped_work_node_revision":1,"request_key":"route-1"});
        assert!(matches!(
            parse("model_route_prepare", prepared.clone()),
            Ok(ModelRouteInvocation::Prepare(_))
        ));
        for field in [
            "workspace_id",
            "session_preference",
            "ranked_route_ids",
            "actual_route_id",
        ] {
            let mut bad = prepared.clone();
            bad[field] = json!(id);
            assert!(parse("model_route_prepare", bad).is_err());
        }
        assert!(matches!(
            parse(
                "model_route_run",
                json!({"preparation_request_key":"route-1"})
            ),
            Ok(ModelRouteInvocation::Run { .. })
        ));
        for bad in [
            json!({"preparation_request_key":"route-1","ranked_route_ids":["route-a"]}),
            json!({"preparation_request_key":" "}),
            json!({"preparation_request_key":null}),
        ] {
            assert!(parse("model_route_run", bad).is_err());
        }
        assert!(matches!(
            parse(
                "model_route_disposition",
                json!({"disposition_id":id,
            "decision_id":id,"action":"reject","rationale":"not needed"})
            ),
            Ok(ModelRouteInvocation::Disposition { .. })
        ));
        assert!(
            parse(
                "model_route_disposition",
                json!({"disposition_id":id,
            "decision_id":id,"action":"accept","rationale":"x","ranked_route_ids":["route-a"]})
            )
            .is_err()
        );
    }
}
