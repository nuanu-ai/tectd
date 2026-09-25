use super::*;
use crate::{
    Error, PipelineCheckpointRef, PipelineInquiryContract, PipelineInstructionSnapshot, Refusal,
    RefusalCode, ResearchCheckpointDraft, Result,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginPipelineRun {
    pub request_id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    #[serde(default)]
    pub delivery_mode: Option<PipelineDeliveryMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry: Option<PipelineInquiryContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_checkpoint: Option<PipelineCheckpointRef>,
    pub qualification_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunContextQuery {
    pub run_id: Uuid,
    #[serde(default)]
    pub view: PipelineRunContextView,
    #[serde(default)]
    pub output_id: Option<Uuid>,
    #[serde(default)]
    pub digest: Option<String>,
    /// Explicitly request the pinned manifest again. The backend resolves the
    /// receipt and digest; this flag is never treated as consumption proof.
    #[serde(default)]
    pub refresh: bool,
}

/// Read one immutable method/skill/resource body from the definition pinned to
/// a run.  The version and digest are caller supplied pins; `refresh` is an
/// explicit request for the body and is never treated as delivery proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInstructionQuery {
    pub run_id: Uuid,
    pub instruction_id: String,
    pub version: String,
    pub digest: String,
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineInstructionSection {
    Method,
    Instruction,
    Skill,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInstructionResponse {
    pub run_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    pub section: PipelineInstructionSection,
    pub instruction: PipelineInstructionSnapshot,
}

impl PipelineInstructionQuery {
    pub fn validate(&self) -> Result<()> {
        if self.run_id.is_nil()
            || self.instruction_id.trim().is_empty()
            || self.version.trim().is_empty()
            || self.digest.trim().is_empty()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }

    pub fn resolve(&self, context: &PipelineRunContext) -> Result<PipelineInstructionResponse> {
        self.validate()?;
        if !self.refresh {
            return Err(Error::refused_at(
                RefusalCode::DeliveryRefreshRequired,
                "WP6-INSTRUCTION-REFRESH-01",
                "arguments.params.refresh",
                "true",
                "false",
                "set_refresh_true",
                "refresh",
            ));
        }

        let mut matches = Vec::new();
        if context.definition.overview.id == self.instruction_id {
            matches.push((
                None,
                PipelineInstructionSection::Method,
                &context.definition.overview,
            ));
        }
        for phase in &context.definition.phases {
            for instruction in &phase.instructions {
                if instruction.id == self.instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Instruction,
                        instruction,
                    ));
                }
            }
            for skill in &phase.skills {
                if skill.id == self.instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Skill,
                        skill,
                    ));
                }
            }
            for resource in &phase.resources {
                if resource.id == self.instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Resource,
                        resource,
                    ));
                }
            }
        }
        let Some((phase_id, section, instruction)) = matches.first() else {
            return Err(method_version_unavailable(
                &self.instruction_id,
                &self.version,
                &self.digest,
                None,
            ));
        };
        if matches.len() != 1
            || instruction.version != self.version
            || instruction.digest != self.digest
        {
            return Err(method_version_unavailable(
                &self.instruction_id,
                &self.version,
                &self.digest,
                Some(instruction),
            ));
        }
        Ok(PipelineInstructionResponse {
            run_id: context.run.id,
            phase_id: phase_id.clone(),
            section: *section,
            instruction: (*instruction).clone(),
        })
    }
}

fn method_version_unavailable(
    instruction_id: &str,
    version: &str,
    digest: &str,
    actual: Option<&PipelineInstructionSnapshot>,
) -> Error {
    let actual = actual
        .map(|instruction| format!("{}@{}", instruction.version, instruction.digest))
        .unwrap_or_else(|| "unavailable".to_owned());
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::MethodVersionUnavailable)
            .with_next_action("select_pinned_instruction_version")
            .with_required("instruction_id_version_digest")
            .with_path(format!("pipeline.instruction.{instruction_id}"))
            .with_expected(format!("{version}@{digest}"))
            .with_actual(actual),
    ))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunContextView {
    #[default]
    Current,
    Output,
    DeliveryReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineContextResponse {
    Current(Box<PipelineRunContext>),
    Output(Box<PipelinePhaseOutput>),
    DeliveryReceipt(Box<PipelineDeliveryReceipt>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineTerminalResultDraft {
    pub summary: String,
    pub evidence: Vec<SliceResultEvidence>,
    pub scope_impact: String,
    pub remaining_work: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletePipelinePhase {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub output: PipelinePhaseOutputDraft,
    #[serde(default)]
    pub consumed_outputs: Vec<PipelineConsumedOutput>,
    #[serde(default)]
    pub consumed_inputs: Vec<PipelineConsumedInput>,
    #[serde(default)]
    pub revisit_phase_id: Option<String>,
    #[serde(default)]
    pub escalation_target: Option<PipelineKind>,
    #[serde(default)]
    pub terminal_result: Option<PipelineTerminalResultDraft>,
    #[serde(default)]
    pub publish_blocked_result: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<ConsumedKnowledgeManifestRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_checkpoint: Option<ResearchCheckpointDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordPipelineInput {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_amendment: Option<PipelineSourceAmendment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSourceAmendment {
    pub target_phase_id: String,
    pub predecessor: PipelineSourcePredecessor,
    pub successor: PipelineSourceSuccessor,
    pub authorization_scope: String,
    pub authorization_provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSourcePredecessor {
    pub output_id: Uuid,
    pub output_revision: i64,
    pub output_digest: String,
    pub artifact_name: String,
    pub artifact_digest: String,
    pub source_path: String,
    pub source_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSourceSuccessor {
    pub path: String,
    pub artifact: PipelineSourceArtifactDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSourceArtifactDraft {
    pub name: String,
    pub media_type: String,
    pub body: String,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalatePipelineDelivery {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineMutationOutcome {
    pub context: PipelineRunContext,
    pub result: Option<SliceResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, version: &str, digest: &str) -> PipelineInstructionSnapshot {
        PipelineInstructionSnapshot {
            id: id.into(),
            version: version.into(),
            digest: digest.into(),
            body: format!("body:{id}"),
            origin_refs: vec![format!("origin:{id}")],
        }
    }

    fn context() -> PipelineRunContext {
        let run_id = Uuid::new_v4();
        let method = snapshot("method", "0.6.0", "method-digest");
        let skill = snapshot("skill", "0.6.0", "skill-digest");
        let phase = PipelinePhaseDefinition {
            id: "phase-1".into(),
            ordinal: 1,
            title: "Phase 1".into(),
            required: true,
            disposition_required: false,
            instructions: vec![snapshot("instruction", "0.6.0", "instruction-digest")],
            skills: vec![skill],
            resources: Vec::new(),
            required_artifacts: Vec::new(),
            validator_contracts: Vec::new(),
            required_fields: Vec::new(),
            allowed_verdicts: Vec::new(),
            required_dispositions: Vec::new(),
            allowed_dispositions: Vec::new(),
            output_constraints: Vec::new(),
            verdict_routes: Vec::new(),
            followup_contracts: Vec::new(),
            allowed_backward_to: Vec::new(),
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "contract".into(),
        };
        PipelineRunContext {
            run: PipelineRun {
                id: run_id,
                scope_id: Uuid::new_v4(),
                slice_id: Uuid::new_v4(),
                slice_revision: 1,
                revision: 1,
                definition_kind: PipelineKind::LightweightTddDevelopment,
                definition_version: "0.6.0".into(),
                definition_digest: "definition-digest".into(),
                selected_option_id: None,
                verification_plan_id: None,
                verification_plan_version: None,
                verification_plan_digest: None,
                delivery_mode: PipelineDeliveryMode::Phasewise,
                qualification_reason: "fixture".into(),
                status: PipelineRunStatus::Active,
                current_phase_id: Some("phase-1".into()),
                current_phase_ordinal: Some(1),
            },
            definition: PipelineDefinitionSnapshot {
                kind: PipelineKind::LightweightTddDevelopment,
                version: "0.6.0".into(),
                digest: "definition-digest".into(),
                overview: method,
                default_mode: PipelineDeliveryMode::Phasewise,
                allowed_modes: vec![PipelineDeliveryMode::Phasewise],
                phases: vec![phase.clone()],
                completion_contract: "completion".into(),
                escalation_contract: "escalation".into(),
                forbidden_claims: Vec::new(),
            },
            inquiry: None,
            source_checkpoint: None,
            checkpoints: Vec::new(),
            delivered_phases: vec![phase],
            attempts: Vec::new(),
            bindings: Vec::new(),
            outputs: Vec::new(),
            outputs_complete: true,
            inputs: Vec::new(),
            result: None,
            knowledge: None,
            knowledge_status: None,
            knowledge_resources: None,
            knowledge_resource_status: None,
            delivery_receipt: None,
            delivery_fresh: false,
        }
    }

    fn query(
        context: &PipelineRunContext,
        id: &str,
        version: &str,
        digest: &str,
    ) -> PipelineInstructionQuery {
        PipelineInstructionQuery {
            run_id: context.run.id,
            instruction_id: id.into(),
            version: version.into(),
            digest: digest.into(),
            refresh: true,
        }
    }

    #[test]
    fn instruction_query_requires_explicit_refresh() {
        let context = context();
        let mut query = query(&context, "skill", "0.6.0", "skill-digest");
        query.refresh = false;
        let error = query.resolve(&context).unwrap_err();
        assert_eq!(
            error.refusal().unwrap().code,
            RefusalCode::DeliveryRefreshRequired
        );
    }

    #[test]
    fn run_plan_identity_is_optional_for_historical_contexts_and_visible_when_pinned() {
        let mut run = context().run;
        let mut legacy = serde_json::to_value(&run).unwrap();
        assert!(legacy.get("verification_plan_id").is_none());
        assert_eq!(
            serde_json::from_value::<PipelineRun>(legacy.clone()).unwrap(),
            run
        );

        let digest = "a".repeat(64);
        run.selected_option_id = Some(format!(
            "{}+verification-plan:{digest}",
            run.definition_kind.as_str()
        ));
        run.verification_plan_id = Some(format!("verification-plan:{digest}"));
        run.verification_plan_version = Some(run.definition_version.clone());
        run.verification_plan_digest = Some(digest.clone());
        let pinned = serde_json::to_value(&run).unwrap();
        assert_eq!(pinned["verification_plan_digest"], digest);
        assert_eq!(serde_json::from_value::<PipelineRun>(pinned).unwrap(), run);
        legacy["selected_option_id"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PipelineRun>(legacy).is_ok());
    }

    #[test]
    fn instruction_query_returns_only_the_exact_pinned_snapshot() {
        let context = context();
        let response = query(&context, "skill", "0.6.0", "skill-digest")
            .resolve(&context)
            .unwrap();
        assert_eq!(response.run_id, context.run.id);
        assert_eq!(response.phase_id.as_deref(), Some("phase-1"));
        assert_eq!(response.section, PipelineInstructionSection::Skill);
        assert_eq!(response.instruction.body, "body:skill");
    }

    #[test]
    fn instruction_query_mismatch_is_typed_and_legacy_versions_still_work() {
        let context = context();
        let error = query(&context, "skill", "0.7.0", "skill-digest")
            .resolve(&context)
            .unwrap_err();
        assert_eq!(
            error.refusal().unwrap().code,
            RefusalCode::MethodVersionUnavailable
        );

        let response = query(&context, "method", "0.6.0", "method-digest")
            .resolve(&context)
            .unwrap();
        assert_eq!(response.section, PipelineInstructionSection::Method);
        assert_eq!(response.phase_id, None);
    }
}
