use serde_json::Value;
use tect_domain::{
    BeginPipelineRun, CompletePipelinePhase, Error, EscalatePipelineDelivery,
    FinalizePipelineEvidenceArtifact, PipelineInstructionQuery, PipelineRunContextQuery,
    PipelineRunMigrationCommand, ReadPipelineEvidenceArtifact, RecordPipelineInput,
    RegisterPipelineEvidenceArtifact, ResolvePipelineCheckpoint, Result,
};

pub(crate) enum PipelineInvocation {
    Context(PipelineRunContextQuery),
    Instruction(PipelineInstructionQuery),
    Begin(BeginPipelineRun),
    Migrate(PipelineRunMigrationCommand),
    Complete(Box<CompletePipelinePhase>),
    Input(Box<RecordPipelineInput>),
    EscalateDelivery(EscalatePipelineDelivery),
    ResolveCheckpoint(ResolvePipelineCheckpoint),
    EvidenceRegister(RegisterPipelineEvidenceArtifact),
    EvidenceFinalize(FinalizePipelineEvidenceArtifact),
    EvidenceRead(ReadPipelineEvidenceArtifact),
}

#[derive(Clone, Copy)]
pub(crate) struct PipelineRefusalBoundary {
    pub rule: &'static str,
    pub path: &'static str,
    pub expected: &'static str,
    pub next_action: &'static str,
    pub required: &'static str,
}

impl PipelineInvocation {
    pub(crate) const fn refusal_boundary(&self) -> PipelineRefusalBoundary {
        match self {
            Self::Context(_) => boundary(
                "WP6-CONTEXT-01",
                "arguments.params",
                "a current run id and a valid context view",
                "refresh_pipeline_context",
                "current_pipeline_context",
            ),
            Self::Instruction(_) => boundary(
                "WP6-INSTRUCTION-01",
                "arguments.params",
                "an instruction bound to the current definition snapshot",
                "read_current_instruction",
                "current_instruction_snapshot",
            ),
            Self::Begin(_) => boundary(
                "WP6-BEGIN-01",
                "arguments.params",
                "an open authorized Slice and supported pipeline definition",
                "refresh_slice_and_begin",
                "open_slice_and_supported_definition",
            ),
            Self::Migrate(_) => boundary(
                "WP6-MIGRATION-01",
                "arguments.params",
                "an explicit successor definition and complete migration mapping",
                "provide_explicit_successor_mapping",
                "successor_mapping",
            ),
            Self::Complete(_) => boundary(
                "WP6-COMPLETE-01",
                "arguments.params",
                "output satisfying the current phase, evidence, transition, review, and authority contracts",
                "read_phase_contract_and_retry",
                "valid_phase_completion",
            ),
            Self::Input(_) => boundary(
                "WP6-INPUT-01",
                "arguments.params",
                "input for the current run revision and waiting phase",
                "refresh_context_and_record_input",
                "current_waiting_phase",
            ),
            Self::EscalateDelivery(_) => boundary(
                "WP6-DELIVERY-01",
                "arguments.params",
                "a current bounded delivery eligible for escalation",
                "refresh_delivery_and_retry",
                "current_delivery",
            ),
            Self::ResolveCheckpoint(_) => boundary(
                "WP6-CHECKPOINT-01",
                "arguments.params",
                "a current open checkpoint and permitted resolution",
                "refresh_checkpoint_and_resolve",
                "current_open_checkpoint",
            ),
            Self::EvidenceRegister(_) => boundary(
                "WP6-EVIDENCE-REGISTER-01",
                "arguments.params",
                "a bounded evidence artifact in the run scope",
                "correct_and_register_evidence",
                "valid_evidence_artifact",
            ),
            Self::EvidenceFinalize(_) => boundary(
                "WP6-EVIDENCE-FINALIZE-01",
                "arguments.params",
                "the current artifact revision with complete content",
                "refresh_and_finalize_evidence",
                "current_complete_artifact",
            ),
            Self::EvidenceRead(_) => boundary(
                "WP6-EVIDENCE-READ-01",
                "arguments.params",
                "an evidence artifact visible in the current run scope",
                "refresh_and_read_evidence",
                "visible_evidence_artifact",
            ),
        }
    }
}

const fn boundary(
    rule: &'static str,
    path: &'static str,
    expected: &'static str,
    next_action: &'static str,
    required: &'static str,
) -> PipelineRefusalBoundary {
    PipelineRefusalBoundary {
        rule,
        path,
        expected,
        next_action,
        required,
    }
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<PipelineInvocation> {
    reject_optional_nulls(&arguments).map_err(|error| normalize_parse_error(error, name))?;
    match name {
        "slice_pipeline_context" => decode(arguments).map(PipelineInvocation::Context),
        "slice_pipeline_instruction" => decode(arguments).map(PipelineInvocation::Instruction),
        "slice_pipeline_begin" => decode(arguments).map(PipelineInvocation::Begin),
        "slice_pipeline_run_migrate" => decode(arguments).map(PipelineInvocation::Migrate),
        "slice_pipeline_phase_complete" => decode_complete(arguments)
            .map(Box::new)
            .map(PipelineInvocation::Complete),
        "slice_pipeline_input" => decode(arguments)
            .map(Box::new)
            .map(PipelineInvocation::Input),
        "slice_pipeline_delivery_escalate" => {
            decode(arguments).map(PipelineInvocation::EscalateDelivery)
        }
        "slice_pipeline_checkpoint_resolve" => {
            decode(arguments).map(PipelineInvocation::ResolveCheckpoint)
        }
        "slice_pipeline_evidence_artifact_register" => {
            decode(arguments).map(PipelineInvocation::EvidenceRegister)
        }
        "slice_pipeline_evidence_artifact_finalize" => {
            decode(arguments).map(PipelineInvocation::EvidenceFinalize)
        }
        "slice_pipeline_evidence_artifact_read" => {
            decode(arguments).map(PipelineInvocation::EvidenceRead)
        }
        _ => Err(Error::InvalidArguments),
    }
    .map_err(|error| normalize_parse_error(error, name))
}

fn normalize_parse_error(error: Error, name: &str) -> Error {
    let rule = match name {
        "slice_pipeline_context" => "WP6-SCHEMA-CONTEXT-01",
        "slice_pipeline_instruction" => "WP6-SCHEMA-INSTRUCTION-01",
        "slice_pipeline_begin" => "WP6-SCHEMA-BEGIN-01",
        "slice_pipeline_run_migrate" => "WP6-SCHEMA-MIGRATION-01",
        "slice_pipeline_phase_complete" => "WP6-SCHEMA-COMPLETE-01",
        "slice_pipeline_input" => "WP6-SCHEMA-INPUT-01",
        "slice_pipeline_delivery_escalate" => "WP6-SCHEMA-DELIVERY-01",
        "slice_pipeline_checkpoint_resolve" => "WP6-SCHEMA-CHECKPOINT-01",
        "slice_pipeline_evidence_artifact_register" => "WP6-SCHEMA-EVIDENCE-REGISTER-01",
        "slice_pipeline_evidence_artifact_finalize" => "WP6-SCHEMA-EVIDENCE-FINALIZE-01",
        "slice_pipeline_evidence_artifact_read" => "WP6-SCHEMA-EVIDENCE-READ-01",
        _ => "WP6-SCHEMA-ROUTE-01",
    };
    error.normalize_pipeline_refusal(
        rule,
        "arguments.params",
        "arguments matching the selected pipeline route schema",
        "read_schema_and_retry",
        "valid_pipeline_arguments",
    )
}

fn decode<T: for<'de> serde::Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
}

fn decode_complete(mut value: Value) -> Result<CompletePipelinePhase> {
    // These receipts are absent from the current public schema, but older
    // pinned definitions still require them. Keep them out of the generic
    // schema decoder so their legacy wire representation reaches the
    // definition-aware domain validator, which rejects supplied proof for v0.7.
    let object = value.as_object_mut().ok_or(Error::InvalidArguments)?;
    let consumed_outputs = object.remove("consumed_outputs");
    let consumed_inputs = object.remove("consumed_inputs");
    let mut request: CompletePipelinePhase = decode(value)?;
    if let Some(value) = consumed_outputs {
        request.consumed_outputs = decode(value)?;
    }
    if let Some(value) = consumed_inputs {
        request.consumed_inputs = decode(value)?;
    }
    Ok(request)
}

pub(crate) fn normalize_complete_arguments(value: Value) -> Result<Value> {
    reject_optional_nulls(&value)?;
    let mut value =
        serde_json::to_value(decode_complete(value)?).map_err(|_| Error::InternalInvariant)?;
    remove_serialized_nulls(&mut value);
    Ok(value)
}

fn remove_serialized_nulls(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|_, value| !value.is_null());
            for value in object.values_mut() {
                remove_serialized_nulls(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_serialized_nulls(value);
            }
        }
        _ => {}
    }
}

fn reject_optional_nulls(value: &Value) -> Result<()> {
    const OPTIONAL: &[&str] = &[
        "delivery_mode",
        "definition_version",
        "view",
        "output_id",
        "digest",
        "verdict",
        "reviewer_context",
        "followup_proposal",
        "reference",
        "revisit_phase_id",
        "escalation_target",
        "terminal_result",
        "consumed_knowledge",
        "inquiry",
        "source_checkpoint",
        "research_checkpoint",
        "source_amendment",
        "terminal",
    ];
    match value {
        Value::Object(object) => {
            if object
                .iter()
                .any(|(key, value)| OPTIONAL.contains(&key.as_str()) && value.is_null())
            {
                return Err(Error::InvalidArguments);
            }
            for value in object.values() {
                reject_optional_nulls(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_optional_nulls(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod refusal_tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn every_pipeline_schema_failure_is_a_complete_route_specific_refusal() {
        let routes = [
            ("slice_pipeline_context", "WP6-SCHEMA-CONTEXT-01"),
            ("slice_pipeline_instruction", "WP6-SCHEMA-INSTRUCTION-01"),
            ("slice_pipeline_begin", "WP6-SCHEMA-BEGIN-01"),
            ("slice_pipeline_run_migrate", "WP6-SCHEMA-MIGRATION-01"),
            ("slice_pipeline_phase_complete", "WP6-SCHEMA-COMPLETE-01"),
            ("slice_pipeline_input", "WP6-SCHEMA-INPUT-01"),
            ("slice_pipeline_delivery_escalate", "WP6-SCHEMA-DELIVERY-01"),
            (
                "slice_pipeline_checkpoint_resolve",
                "WP6-SCHEMA-CHECKPOINT-01",
            ),
            (
                "slice_pipeline_evidence_artifact_register",
                "WP6-SCHEMA-EVIDENCE-REGISTER-01",
            ),
            (
                "slice_pipeline_evidence_artifact_finalize",
                "WP6-SCHEMA-EVIDENCE-FINALIZE-01",
            ),
            (
                "slice_pipeline_evidence_artifact_read",
                "WP6-SCHEMA-EVIDENCE-READ-01",
            ),
        ];
        let mut rules = BTreeSet::new();
        for (route, rule) in routes {
            let error = parse(route, json!({}))
                .err()
                .expect("empty schema must fail");
            let refusal = error.refusal().expect("pipeline schema refusal");
            assert!(refusal.is_complete_pipeline_refusal(), "{route}");
            assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
            assert_eq!(refusal.rule.as_deref(), Some(rule), "{route}");
            assert_eq!(refusal.path.as_deref(), Some("arguments.params"), "{route}");
            assert!(rules.insert(rule), "duplicate rule {rule}");
        }
    }

    #[test]
    fn pipeline_normalization_keeps_infrastructure_failures_generic() {
        for error in [
            Error::StorageUnavailable,
            Error::TransportUnavailable,
            Error::Unauthorized,
        ] {
            let code = error.code();
            let normalized = normalize_parse_error(error, "slice_pipeline_begin");
            assert_eq!(normalized.code(), code);
            assert!(!matches!(
                normalized,
                Error::Refused(_) | Error::PipelineRefused { .. }
            ));
        }
    }
}
