use serde::Deserialize;
use serde_json::{Value, json};
use tect_domain::{Error, Result};
use uuid::Uuid;

pub(crate) enum Invocation {
    Program(crate::program_tools::ProgramInvocation),
    Setup(crate::setup_tools::SetupInvocation),
    ScopeCandidate(crate::scope_candidate_tools::ScopeCandidateInvocation),
    Slice(crate::slice_tools::SliceInvocation),
    Pipeline(crate::pipeline_tools::PipelineInvocation),
    Knowledge(crate::knowledge_tools::KnowledgeInvocation),
    KnowledgeLifecycle(crate::knowledge_lifecycle_tools::KnowledgeLifecycleInvocation),
    KnowledgeMaintenance(crate::knowledge_maintenance_tools::KnowledgeMaintenanceInvocation),
    KnowledgeSearch(tect_domain::KnowledgeSearchQuery),
    Advisory(crate::advisory_tools::AdvisoryInvocation),
    PipelineRecommendationPrepare(tect_application::PreparePipelineRecommendation),
    PipelineRecommendationRun(tect_application::RunPipelineRecommendation),
    PipelineRecommendationDisposition(tect_domain::PipelineDispositionRequest),
    MatrixTask(crate::matrix_task_tools::MatrixTaskInvocation),
    MatrixVerification(tect_application::VerifyMatrixTask),
    MatrixPlanningEffect(crate::matrix_planning_effect_tools::MatrixPlanningEffectInvocation),
    PipelineOpenEffect(crate::pipeline_open_effect_tools::PipelineOpenEffectInvocation),
    MatrixAdvisory(crate::matrix_advisory_tools::MatrixAdvisoryInvocation),
    MatrixDisposition(crate::matrix_disposition_tools::MatrixDispositionInvocation),
    Help(crate::api::HelpRequest),
    OpenWorkspace,
    GetState,
    RegisterSource { path: String },
    SelectWorktrees { worktree_ids: Vec<Uuid> },
    ListSources { after: Option<Uuid>, limit: u32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterSourceArguments {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectWorktreesArguments {
    worktree_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListSourcesArguments {
    #[serde(default)]
    after: Option<Uuid>,
    limit: u32,
}

pub(crate) fn parse_invocation(name: &str, arguments: Value) -> Result<Invocation> {
    match name {
        "open_workspace" if empty_object(&arguments) => Ok(Invocation::OpenWorkspace),
        "get_state" if empty_object(&arguments) => Ok(Invocation::GetState),
        "register_source" => {
            let arguments = serde_json::from_value::<RegisterSourceArguments>(arguments)
                .map_err(Error::invalid_arguments_from)?;
            Ok(Invocation::RegisterSource {
                path: arguments.path,
            })
        }
        "select_worktrees" => {
            let arguments = serde_json::from_value::<SelectWorktreesArguments>(arguments)
                .map_err(Error::invalid_arguments_from)?;
            Ok(Invocation::SelectWorktrees {
                worktree_ids: arguments.worktree_ids,
            })
        }
        "list_sources" => {
            if arguments.get("after").is_some_and(Value::is_null) {
                return Err(Error::InvalidArguments);
            }
            let arguments = serde_json::from_value::<ListSourcesArguments>(arguments)
                .map_err(Error::invalid_arguments_from)?;
            Ok(Invocation::ListSources {
                after: arguments.after,
                limit: arguments.limit,
            })
        }
        "help" => crate::api::parse_help(arguments).map(Invocation::Help),
        "pipeline_recommendation_prepare" => crate::pipeline_recommendation_tools::parse(arguments)
            .map(Invocation::PipelineRecommendationPrepare),
        "pipeline_recommendation_run" => crate::pipeline_recommendation_tools::parse_run(arguments)
            .map(Invocation::PipelineRecommendationRun),
        "pipeline_recommendation_disposition" => {
            crate::pipeline_recommendation_tools::parse_disposition(arguments)
                .map(Invocation::PipelineRecommendationDisposition)
        }
        "knowledge_search" => {
            crate::knowledge_search_tools::parse(name, arguments).map(Invocation::KnowledgeSearch)
        }
        "record_matrix_task" | "get_matrix_task" => {
            crate::matrix_task_tools::parse(name, arguments).map(Invocation::MatrixTask)
        }
        "verify_matrix_task" => {
            crate::matrix_verification_tools::parse(arguments).map(Invocation::MatrixVerification)
        }
        "get_matrix_planning_effect" | "verify_matrix_planning_effect" => {
            crate::matrix_planning_effect_tools::parse(name, arguments)
                .map(Invocation::MatrixPlanningEffect)
        }
        "get_pipeline_open_effect" | "verify_pipeline_open_effect" => {
            crate::pipeline_open_effect_tools::parse(name, arguments).map(Invocation::PipelineOpenEffect)
        }
        "request_engineering_advisory" | "get_engineering_advisory" => {
            crate::matrix_advisory_tools::parse(name, arguments).map(Invocation::MatrixAdvisory)
        }
        "record_matrix_disposition" | "get_matrix_disposition" => {
            crate::matrix_disposition_tools::parse(name, arguments)
                .map(Invocation::MatrixDisposition)
        }
        _ if matches!(
            name,
            "scope_advisory_request"
                | "candidate_advisory_verify"
                | "scope_advisory_disposition"
                | "get_advisory_config"
                | "configure_advisory"
                | "workspace_advisory_audit"
                | "scope_advisory_get"
                | "scope_advisory_card"
                | "scope_advisory_audit"
                | "candidate_advisory_get"
                | "candidate_advisory_audit"
        ) =>
        {
            crate::advisory_tools::parse(name, arguments).map(Invocation::Advisory)
        }
        _ if matches!(
            name,
            "knowledge_maintenance"
                | "knowledge_maintenance_observe"
                | "knowledge_maintenance_begin"
        ) =>
        {
            crate::knowledge_maintenance_tools::parse(name, arguments)
                .map(Invocation::KnowledgeMaintenance)
        }
        _ if matches!(
            name,
            "knowledge_lifecycle"
                | "knowledge_unit"
                | "knowledge_change_begin"
                | "knowledge_change_phase_complete"
                | "knowledge_change_record_input"
                | "knowledge_change_commit"
                | "knowledge_change_settle_effects"
        ) =>
        {
            crate::knowledge_lifecycle_tools::parse(name, arguments)
                .map(Invocation::KnowledgeLifecycle)
        }
        _ if matches!(
            name,
            "knowledge_context"
                | "knowledge_change"
                | "knowledge_change_prepare"
                | "knowledge_change_review"
                | "knowledge_change_publish"
                | "pipeline_knowledge_refresh"
        ) =>
        {
            crate::knowledge_tools::parse(name, arguments).map(Invocation::Knowledge)
        }
        _ if name.starts_with("slice_pipeline_") => {
            crate::pipeline_tools::parse(name, arguments).map(Invocation::Pipeline)
        }
        _ if matches!(
            name,
            "scope_context"
                | "slice_pipelines"
                | "slice_candidate_context"
                | "scope_open"
                | "save_slice_candidate_set"
                | "record_slice_candidate_input"
                | "refresh_slice_candidate_set"
                | "slice_open"
                | "slice_context"
                | "slice_result_record"
        ) =>
        {
            crate::slice_tools::parse(name, arguments).map(Invocation::Slice)
        }
        _ if name.contains("candidate") => {
            crate::scope_candidate_tools::parse(name, arguments).map(Invocation::ScopeCandidate)
        }
        _ if name.ends_with("_setup")
            || name == "record_setup_input"
            || name == "read_skill" && arguments["name"] == "tectd-setup" =>
        {
            crate::setup_tools::parse(name, arguments).map(Invocation::Setup)
        }
        _ => crate::program_tools::parse(name, arguments).map(Invocation::Program),
    }
}

fn empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

pub(crate) fn object_schema(properties: Value, required: Value) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

pub(crate) fn annotations(read_only: bool) -> Value {
    json!({
        "readOnlyHint": read_only,
        "idempotentHint": true,
        "destructiveHint": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_expose_five_bounded_tools() {
        let definitions = crate::api::definitions();
        let tools = definitions["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
        );
        assert_eq!(
            tools[2]["inputSchema"]["properties"]["route"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            48
        );
    }

    #[test]
    fn argument_decoding_rejects_unknown_and_wrongly_typed_fields() {
        assert!(matches!(
            parse_invocation("register_source", json!({"path": "/repo"})),
            Ok(Invocation::RegisterSource { .. })
        ));
        for invalid in [
            json!({"path": "/repo", "workspace_key": "spoofed"}),
            json!({"path": "/repo", "native_session_id": Uuid::new_v4()}),
            json!({"path": 7}),
            json!([]),
        ] {
            assert!(matches!(
                parse_invocation("register_source", invalid),
                Err(Error::InvalidArguments | Error::InvalidArgumentsDetail(_))
            ));
        }
        assert!(matches!(
            parse_invocation("list_sources", json!({"after": null, "limit": 25})),
            Err(Error::InvalidArguments | Error::InvalidArgumentsDetail(_))
        ));
        assert!(matches!(
            parse_invocation("list_sources", json!({"limit": 1.5})),
            Err(Error::InvalidArguments | Error::InvalidArgumentsDetail(_))
        ));
    }
}
