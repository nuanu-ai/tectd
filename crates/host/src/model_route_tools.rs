use serde::Deserialize;
use serde_json::Value;
use tect_application::{ModelRouteDispositionAction, PrepareModelRouteRecommendation};
use tect_domain::{AdvisoryRequestPreference, Error, Result};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) enum ModelRouteInvocation {
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
    session_preference: AdvisoryRequestPreference,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
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
                    session_preference: a.session_preference,
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
        for field in ["workspace_id", "ranked_route_ids", "actual_route_id"] {
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
