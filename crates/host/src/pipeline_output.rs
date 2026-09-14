use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use tect_domain::{
    BeginPipelineRunOutcome, Error, PipelineContextResponse, PipelineKnowledgeResourceState,
    PipelineKnowledgeState, PipelineMutationOutcome, PipelineOutputConstraint, PipelineRunContext,
    PipelineRunStatus, Result,
};

pub(crate) fn begin(mut value: BeginPipelineRunOutcome, capacity: usize) -> Result<Value> {
    let actions = actions(match &value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    })?;
    let context = match &mut value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    };
    restrict_definition_delivery(context, true);
    encode(value, actions, capacity)
}

pub(crate) fn context(value: PipelineContextResponse, capacity: usize) -> Result<Value> {
    match value {
        PipelineContextResponse::Current(mut context) => {
            let actions = actions(&context)?;
            restrict_definition_delivery(&mut context, true);
            encode_context(*context, actions, capacity)
        }
        PipelineContextResponse::Output(output) => {
            let actions = vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":output.run_id}),
            )?];
            encode(*output, actions, capacity)
        }
    }
}

pub(crate) fn mutation(mut value: PipelineMutationOutcome, capacity: usize) -> Result<Value> {
    let actions = actions(&value.context)?;
    restrict_definition_delivery(&mut value.context, false);
    encode_mutation(value, actions, capacity)
}

fn restrict_definition_delivery(context: &mut PipelineRunContext, explicit_reread: bool) {
    match context.run.delivery_mode {
        tect_domain::PipelineDeliveryMode::Phasewise => {
            context.definition.phases = context.delivered_phases.clone();
        }
        tect_domain::PipelineDeliveryMode::Whole if !explicit_reread => {
            context.definition.phases.clear();
            context.delivered_phases.clear();
        }
        tect_domain::PipelineDeliveryMode::Whole => {}
    }
}

fn actions(context: &PipelineRunContext) -> Result<Vec<Value>> {
    let run = &context.run;
    let Some(phase_id) = &run.current_phase_id else {
        return Ok(vec![responses::action(
            "slice_context",
            json!({"slice_id":run.slice_id}),
        )?]);
    };
    if matches!(
        context.knowledge_status.as_ref().map(|status| status.state),
        Some(PipelineKnowledgeState::Stale | PipelineKnowledgeState::NeedsContext)
    ) || matches!(
        context
            .knowledge_resource_status
            .as_ref()
            .map(|status| status.state),
        Some(PipelineKnowledgeResourceState::Stale | PipelineKnowledgeResourceState::NeedsContext)
    ) {
        return Ok(vec![
            responses::action(
                "pipeline_knowledge_refresh",
                json!({"request_id":request_id(run.id,run.revision,"knowledge-refresh"),
                    "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id}),
            )?,
            responses::action("slice_pipeline_context", json!({"run_id":run.id}))?,
        ]);
    }
    let action = match run.status {
        PipelineRunStatus::Active => {
            let ordinal = run.current_phase_ordinal.ok_or(Error::InternalInvariant)?;
            let consumed_outputs = context
                .bindings
                .iter()
                .filter(|binding| binding.phase_ordinal < ordinal && !binding.stale)
                .map(|binding| {
                    json!({"phase_id":binding.phase_id,
                    "output_revision":binding.output_revision,"digest":binding.output_digest})
                })
                .collect::<Vec<_>>();
            let consumed_inputs = context
                .inputs
                .iter()
                .filter(|input| &input.phase_id == phase_id)
                .map(|input| {
                    json!({"input_id":input.id,"sequence":input.sequence,
                    "digest":input.digest})
                })
                .collect::<Vec<_>>();
            let mut params = json!({"request_id":request_id(run.id,run.revision,"complete"),
                "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id,
                "consumed_outputs":consumed_outputs,"consumed_inputs":consumed_inputs});
            let legacy = context
                .knowledge
                .as_ref()
                .filter(|manifest| !manifest.selected.is_empty())
                .map(|manifest| (manifest.id, manifest.digest.as_str()));
            let generic = context
                .knowledge_resources
                .as_ref()
                .filter(|manifest| !manifest.selected.is_empty())
                .map(|manifest| (manifest.id, manifest.digest.as_str()));
            if let Some((manifest_id, digest)) = legacy.or(generic) {
                params["consumed_knowledge"] = json!({"manifest_id":manifest_id,"digest":digest});
            }
            let mut fields = vec![
                json!({"path":"arguments.params.outcome","format":"Caller-reported phase outcome allowed by the current verdict route."}),
                json!({"path":"arguments.params.transition","format":"Transition allowed by the current verdict route."}),
                json!({"path":"arguments.params.output","format":"Complete phase output body, producer context, typed fields, verdict, exact route dispositions, pinned skill/resource reads, artifacts, validator receipts, any route-required follow-up proposal, and reviewer attestation/reference."}),
                json!({"path":"arguments.params.terminal_result","format":"Required only for a terminal complete, published block, or pipeline-kind escalation."}),
            ];
            let current = context
                .delivered_phases
                .iter()
                .find(|phase| &phase.id == phase_id)
                .or_else(|| {
                    context
                        .definition
                        .phases
                        .iter()
                        .find(|phase| &phase.id == phase_id)
                });
            if current.is_some_and(|phase| {
                phase.output_constraints.iter().any(|constraint| {
                    matches!(
                        constraint,
                        PipelineOutputConstraint::ResolvedKnowledgePublication { .. }
                    )
                })
            }) {
                fields.push(json!({"path":"arguments.params.output.knowledge_publication",
                    "format":"For promoted verdicts only: exact backend-issued change_id, publisher_receipt_id, publisher_receipt_digest, and covered operation_ids. The backend resolves producer lineage; unrelated or forged receipts fail."}));
            }
            crate::api::needs_action(
                "needs_context",
                "slice_pipeline_phase_complete",
                params,
                "context_input",
                json!({"fields":fields}),
            )?
        }
        PipelineRunStatus::WaitingInput | PipelineRunStatus::Blocked => crate::api::needs_action(
            "needs_input",
            "slice_pipeline_input",
            json!({"request_id":request_id(run.id,run.revision,"input"),
                "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id}),
            "input",
            json!({"fields":[{"path":"arguments.params.input","format":"Exact phase-local operator answer, context, authority evidence, or resume input."}]}),
        )?,
        PipelineRunStatus::Completed | PipelineRunStatus::Escalated => unreachable!(),
    };
    Ok(vec![
        action,
        responses::action("slice_pipeline_context", json!({"run_id":run.id}))?,
    ])
}

fn encode<T: Serialize>(value: T, actions: Vec<Value>, capacity: usize) -> Result<Value> {
    let data = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    let result = responses::with_actions(data, actions, Some(0));
    if responses::encoded_len(&result)? > capacity {
        Err(Error::RequestTooLarge)
    } else {
        Ok(result)
    }
}

fn encode_context(
    mut value: PipelineRunContext,
    mut actions: Vec<Value>,
    capacity: usize,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity) {
        return Ok(result);
    }
    add_output_actions(&value, &mut actions)?;
    value.outputs.clear();
    value.outputs_complete = false;
    encode(value, actions, capacity)
}

fn encode_mutation(
    mut value: PipelineMutationOutcome,
    mut actions: Vec<Value>,
    capacity: usize,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity) {
        return Ok(result);
    }
    add_output_actions(&value.context, &mut actions)?;
    value.context.outputs.clear();
    value.context.outputs_complete = false;
    encode(value, actions, capacity)
}

fn add_output_actions(context: &PipelineRunContext, actions: &mut Vec<Value>) -> Result<()> {
    for binding in &context.bindings {
        actions.push(responses::action(
            "slice_pipeline_context",
            json!({"run_id":context.run.id,"view":"output","output_id":binding.output_id,
                "digest":binding.output_digest}),
        )?);
    }
    Ok(())
}

fn request_id(id: uuid::Uuid, revision: i64, operation: &str) -> uuid::Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("tectd-pipeline:{id}:{revision}:{operation}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        KnowledgeAccessScope, KnowledgeBindingPurpose, KnowledgeBindingTarget,
        KnowledgeBindingVersion, KnowledgeEpistemicState, KnowledgeKind, KnowledgeLifecycleState,
        KnowledgeProfileId, KnowledgeProfileSections, PipelineDefinitionSnapshot,
        PipelineDeliveryMode, PipelineInstructionSnapshot, PipelineKnowledgeBindingPin,
        PipelineKnowledgeResource, PipelineKnowledgeResourceManifest,
        PipelineKnowledgeResourceStatus, PipelinePhaseDefinition, PipelinePhaseRetryPolicy,
        PipelineRun,
    };

    fn instruction() -> PipelineInstructionSnapshot {
        PipelineInstructionSnapshot {
            id: "fixture:instruction".into(),
            version: "1".into(),
            digest: "instruction-digest".into(),
            body: "fixture instruction".into(),
            origin_refs: vec!["fixture".into()],
        }
    }

    fn phase() -> PipelinePhaseDefinition {
        PipelinePhaseDefinition {
            id: "fixture-phase".into(),
            ordinal: 1,
            title: "Fixture phase".into(),
            required: true,
            disposition_required: false,
            instructions: vec![instruction()],
            skills: Vec::new(),
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
            output_contract: "fixture output".into(),
        }
    }

    fn resource() -> PipelineKnowledgeResource {
        PipelineKnowledgeResource {
            unit_id: uuid::Uuid::new_v4(),
            revision: 1,
            lifecycle: KnowledgeLifecycleState::Active,
            access_scope: KnowledgeAccessScope::WorkspaceMembers,
            rdf_digest: "rdf-digest".into(),
            unit_iri: "urn:fixture:unit".into(),
            revision_iri: "urn:fixture:revision".into(),
            title: "Fixture knowledge".into(),
            canonical_text: "Fixture canonical text.".into(),
            knowledge_kind: KnowledgeKind::Constraint,
            epistemic_state: KnowledgeEpistemicState::Normative,
            target_iris: vec!["urn:fixture:target".into()],
            profiles: vec![KnowledgeProfileId::General],
            conditions: Vec::new(),
            exceptions: Vec::new(),
            sections: KnowledgeProfileSections::default(),
            source_pins: Vec::new(),
            latest_validation: None,
            binding: PipelineKnowledgeBindingPin {
                binding_iri: "urn:fixture:binding".into(),
                target: KnowledgeBindingTarget::Workspace,
                purpose: KnowledgeBindingPurpose::Required,
                version_resolution: KnowledgeBindingVersion::CurrentAccepted,
                definition_kind: None,
                definition_version: None,
                definition_digest: None,
            },
            why_included: "workspace_binding".into(),
        }
    }

    fn context(state: PipelineKnowledgeResourceState, selected: bool) -> PipelineRunContext {
        let run_id = uuid::Uuid::new_v4();
        let phase = phase();
        PipelineRunContext {
            run: PipelineRun {
                id: run_id,
                scope_id: uuid::Uuid::new_v4(),
                slice_id: uuid::Uuid::new_v4(),
                slice_revision: 1,
                revision: 7,
                definition_kind: tect_domain::PipelineKind::LightweightTddDevelopment,
                definition_version: "fixture-version".into(),
                definition_digest: "definition-digest".into(),
                delivery_mode: PipelineDeliveryMode::Phasewise,
                qualification_reason: "fixture".into(),
                status: PipelineRunStatus::Active,
                current_phase_id: Some(phase.id.clone()),
                current_phase_ordinal: Some(phase.ordinal),
            },
            definition: PipelineDefinitionSnapshot {
                kind: tect_domain::PipelineKind::LightweightTddDevelopment,
                version: "fixture-version".into(),
                digest: "definition-digest".into(),
                overview: instruction(),
                default_mode: PipelineDeliveryMode::Phasewise,
                allowed_modes: vec![PipelineDeliveryMode::Phasewise],
                phases: vec![phase.clone()],
                completion_contract: "fixture completion".into(),
                escalation_contract: "fixture escalation".into(),
                forbidden_claims: Vec::new(),
            },
            delivered_phases: vec![phase],
            attempts: Vec::new(),
            bindings: Vec::new(),
            outputs: Vec::new(),
            outputs_complete: true,
            inputs: Vec::new(),
            result: None,
            knowledge: None,
            knowledge_status: None,
            knowledge_resources: Some(PipelineKnowledgeResourceManifest {
                id: uuid::Uuid::new_v4(),
                digest: "manifest-digest".into(),
                semantic_digest: "semantic-digest".into(),
                workspace_generation: 1,
                run_id,
                run_revision: if matches!(
                    state,
                    PipelineKnowledgeResourceState::Stale
                        | PipelineKnowledgeResourceState::NeedsContext
                ) {
                    6
                } else {
                    7
                },
                phase_id: "fixture-phase".into(),
                definition_version: "fixture-version".into(),
                definition_digest: "definition-digest".into(),
                method_requirements: Vec::new(),
                selected: selected.then(resource).into_iter().collect(),
                unresolved_needs: Vec::new(),
                freshness_warnings: Vec::new(),
            }),
            knowledge_resource_status: Some(PipelineKnowledgeResourceStatus {
                state,
                current_generation: 1,
                changed_unit_ids: Vec::new(),
                freshness_warnings: Vec::new(),
                access_changed: false,
            }),
        }
    }

    fn action<'a>(values: &'a [Value], route: &str) -> &'a Value {
        values
            .iter()
            .find(|value| value["arguments"]["route"] == route)
            .expect("expected action route")
    }

    #[test]
    fn generic_stale_manifest_advertises_only_exact_refresh_before_completion() {
        let context = context(PipelineKnowledgeResourceState::Stale, true);
        let values = actions(&context).unwrap();
        assert_eq!(values.len(), 2);
        assert!(
            !values
                .iter()
                .any(|value| value["arguments"]["route"] == "slice.pipeline.phase.complete")
        );
        let refresh = action(&values, "pipeline.knowledge_refresh");
        assert_eq!(
            refresh["arguments"]["params"]["run_id"],
            context.run.id.to_string()
        );
        assert_eq!(refresh["arguments"]["params"]["run_revision"], 7);
        assert_eq!(refresh["arguments"]["params"]["phase_id"], "fixture-phase");
    }

    #[test]
    fn generic_current_selection_supplies_exact_consumed_manifest_guard() {
        let current = context(PipelineKnowledgeResourceState::Current, true);
        let values = actions(&current).unwrap();
        let complete = action(&values, "slice.pipeline.phase.complete");
        let manifest = current.knowledge_resources.as_ref().unwrap();
        assert_eq!(
            complete["arguments"]["params"]["consumed_knowledge"],
            json!({"manifest_id":manifest.id,"digest":manifest.digest})
        );

        let inactive = context(PipelineKnowledgeResourceState::Inactive, false);
        let values = actions(&inactive).unwrap();
        assert!(
            action(&values, "slice.pipeline.phase.complete")["arguments"]["params"]
                .get("consumed_knowledge")
                .is_none()
        );
    }
}
