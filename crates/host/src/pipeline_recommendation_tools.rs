use serde::Deserialize;
use serde_json::Value;
use tect_application::{PreparePipelineRecommendation, RunPipelineRecommendation};
use tect_domain::{AdvisoryRequestPreference, Error, PipelineDispositionRequest, Result};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareArguments {
    candidate_set_id: Uuid,
    expected_candidate_set_revision: i64,
    work_node_id: Uuid,
    expected_work_node_revision: i64,
    request_key: String,
    #[serde(default)]
    session_preference: AdvisoryRequestPreference,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
}

pub(crate) fn parse(arguments: Value) -> Result<PreparePipelineRecommendation> {
    let arguments: PrepareArguments =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    if arguments.candidate_set_id.is_nil()
        || arguments.work_node_id.is_nil()
        || arguments.expected_candidate_set_revision < 2
        || arguments.expected_work_node_revision < 1
        || arguments.request_key.is_empty()
        || arguments.request_key.len() > 256
        || arguments.request_key.contains('\0')
        || arguments.request_key.trim() != arguments.request_key
    {
        return Err(Error::InvalidArguments);
    }
    Ok(PreparePipelineRecommendation {
        candidate_set_id: arguments.candidate_set_id,
        expected_candidate_set_revision: arguments.expected_candidate_set_revision,
        work_node_id: arguments.work_node_id,
        expected_work_node_revision: arguments.expected_work_node_revision,
        request_key: arguments.request_key,
        session_preference: arguments.session_preference,
        request_preference: arguments.request_preference,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArguments {
    opportunity_id: Uuid,
}

pub(crate) fn parse_run(arguments: Value) -> Result<RunPipelineRecommendation> {
    let arguments: RunArguments =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    if arguments.opportunity_id.is_nil() {
        return Err(Error::InvalidArguments);
    }
    Ok(RunPipelineRecommendation {
        opportunity_id: arguments.opportunity_id,
    })
}

pub(crate) fn parse_disposition(arguments: Value) -> Result<PipelineDispositionRequest> {
    let request: PipelineDispositionRequest =
        serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
    request.validate()?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_prepare_arguments() {
        let id = Uuid::new_v4();
        let valid = json!({"candidate_set_id":id,"expected_candidate_set_revision":2,
            "work_node_id":id,"expected_work_node_revision":1,"request_key":"work-1"});
        assert!(parse(valid.clone()).is_ok());
        for (field, value) in [
            ("candidate_set_id", json!(Uuid::nil())),
            ("expected_candidate_set_revision", json!(1)),
            ("work_node_id", json!(Uuid::nil())),
            ("expected_work_node_revision", json!(0)),
            ("request_key", json!(" bad ")),
            ("session_preference", json!("unknown")),
            ("request_preference", json!(null)),
            ("actor_id", json!(id)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(parse(invalid).is_err(), "accepted {field}");
        }
    }

    #[test]
    fn strict_run_arguments() {
        let id = Uuid::new_v4();
        assert!(parse_run(json!({"opportunity_id": id})).is_ok());
        for invalid in [
            json!({}),
            json!({"opportunity_id": Uuid::nil()}),
            json!({"opportunity_id": id, "actor_id": id}),
            json!({"opportunity_id": id, "request_key": "again"}),
        ] {
            assert!(parse_run(invalid).is_err());
        }
    }

    #[test]
    fn strict_disposition_arguments() {
        let id = Uuid::new_v4();
        let valid = json!({"request_id":id,"opportunity_id":id,
            "expected_work_revision":1,"manifest_digest":"a".repeat(64),
            "action":"reject_recommendation","rationale":"Reviewed"});
        assert!(parse_disposition(valid.clone()).is_ok());
        for (field, value) in [
            ("request_id", json!(Uuid::nil())),
            ("expected_work_revision", json!(0)),
            ("manifest_digest", json!("bad")),
            ("action", json!("approve")),
            ("rationale", json!(" ")),
            ("actor_id", json!(id)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(parse_disposition(invalid).is_err(), "accepted {field}");
        }
    }
}
