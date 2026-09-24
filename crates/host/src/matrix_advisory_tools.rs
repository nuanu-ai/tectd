use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::RequestEngineeringAdvisory;
use tect_domain::{AdvisoryOpportunity, AdvisoryRequestPreference, Error, Result};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestArguments {
    task_id: Uuid,
    expected_task_revision: i64,
    request_key: String,
    #[serde(default)]
    session_preference: AdvisoryRequestPreference,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    task_id: Uuid,
    request_key: String,
}

pub(crate) enum MatrixAdvisoryInvocation {
    Request(RequestEngineeringAdvisory),
    Get { task_id: Uuid, request_key: String },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixAdvisoryInvocation> {
    match name {
        "request_engineering_advisory" => {
            let args: RequestArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil()
                || args.expected_task_revision < 1
                || !valid_request_key(&args.request_key)
            {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixAdvisoryInvocation::Request(
                RequestEngineeringAdvisory {
                    task_id: args.task_id,
                    expected_task_revision: args.expected_task_revision,
                    request_key: args.request_key,
                    session_preference: args.session_preference,
                    request_preference: args.request_preference,
                },
            ))
        }
        "get_engineering_advisory" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil() || !valid_request_key(&args.request_key) {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixAdvisoryInvocation::Get {
                task_id: args.task_id,
                request_key: args.request_key,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn valid_request_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 256 && !key.contains('\0') && key.trim() == key
}

pub(crate) fn receipt(value: AdvisoryOpportunity) -> Value {
    json!({
        "task_id": value.target_id,
        "task_revision": value.matrix_task_revision,
        "choice_set_digest": value.matrix_choice_set_digest,
        "request_key": value.workflow_occurrence_key,
        "opportunity_id": value.id,
        "state": value.state,
        "reason": value.primary_reason,
        "config_revision": value.config_revision,
        "material_digest": value.material_digest,
        "provider_called": value.provider_called,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_matrix_advisory_arguments() {
        let id = Uuid::new_v4();
        let base = json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1"});
        assert!(matches!(
            parse("request_engineering_advisory", base.clone()),
            Ok(MatrixAdvisoryInvocation::Request(_))
        ));
        assert!(matches!(
            parse(
                "get_engineering_advisory",
                json!({"task_id":id,"request_key":"task-1"})
            ),
            Ok(MatrixAdvisoryInvocation::Get { .. })
        ));
        for invalid in [
            json!({"task_id":id,"expected_task_revision":0,"request_key":"task-1"}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":" task-1"}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"x".repeat(257)}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","principal_id":id}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","request_preference":"force"}),
        ] {
            assert!(parse("request_engineering_advisory", invalid).is_err());
        }
        assert!(
            parse(
                "get_engineering_advisory",
                json!({"task_id":id,"request_key":"task-1","workspace_id":id})
            )
            .is_err()
        );
    }
}
