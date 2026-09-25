use super::*;
use crate::{
    PipelineDispositionBasis, PipelineRecommendationBasis, PipelineRecommendationContext,
    PipelineRecommendationDefinitionProvider, PreparedPipelineRecommendation,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryOpportunity, AdvisoryOpportunityInput, AdvisoryReason, AdvisoryRequestPreference,
    PIPELINE_RECOMMENDATION_SCHEMA, PipelineDefinitionSnapshot, PipelineDeliveryMode,
    PipelineInstructionSnapshot, PipelineKind, PipelinePhaseDefinition, PipelinePhaseRetryPolicy,
    PipelineRecommendationDisposition, PipelineRecommendationManifest,
    PipelineRecommendationOption, PipelineVerificationPlan, SliceCandidateNode,
};

include!("pipeline_recommendation_disposition_tests/definitions.rs");

struct FakeStore {
    basis: PipelineDispositionBasis,
    current: bool,
    saved: Option<PipelineDispositionResult>,
    writes: usize,
    concurrent_receipt: bool,
}

#[async_trait]
impl PipelineRecommendationStore for FakeStore {
    async fn pipeline_recommendation_by_opportunity(
        &mut self,
        _: Uuid,
        _: Uuid,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        Ok(Some(self.basis.prepared.clone()))
    }
    async fn pipeline_recommendation_is_current(
        &mut self,
        _: Uuid,
        _: &PreparedPipelineRecommendation,
    ) -> Result<bool> {
        Ok(self.current)
    }
    async fn pipeline_disposition_is_current(
        &mut self,
        _: Uuid,
        _: &PipelineDispositionBasis,
    ) -> Result<bool> {
        Ok(self.current)
    }
    async fn pipeline_disposition_by_opportunity(
        &mut self,
        _: Uuid,
        _: Uuid,
    ) -> Result<Option<PipelineDispositionResult>> {
        Ok(self.saved.clone())
    }
    async fn load_pipeline_disposition_basis(
        &mut self,
        _: Uuid,
        _: Uuid,
    ) -> Result<Option<PipelineDispositionBasis>> {
        Ok(Some(self.basis.clone()))
    }
    async fn capture_pipeline_disposition(
        &mut self,
        _: Uuid,
        result: &PipelineDispositionResult,
    ) -> Result<PipelineDispositionResult> {
        self.writes += 1;
        if self.saved.is_some() {
            return Err(Error::InputConflict);
        }
        if self.concurrent_receipt {
            let mut first = result.clone();
            first.id = Uuid::new_v4();
            self.saved = Some(first.clone());
            return Ok(first);
        }
        self.saved = Some(result.clone());
        Ok(result.clone())
    }
    async fn load_pipeline_recommendation_basis(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: Uuid,
        _: bool,
    ) -> Result<Option<PipelineRecommendationBasis>> {
        unreachable!()
    }
    async fn pipeline_recommendation_by_request(
        &mut self,
        _: Uuid,
        _: &str,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        unreachable!()
    }
    async fn capture_pipeline_recommendation(
        &mut self,
        _: Uuid,
        _: &AdvisoryOpportunityInput,
        _: &PipelineRecommendationContext,
        _: &PipelineRecommendationManifest,
    ) -> Result<PreparedPipelineRecommendation> {
        unreachable!()
    }
}

fn fixture(
    advice: PipelineDispositionAdvice,
) -> (FakeStore, PipelineDispositionRequest, Uuid, Uuid, Uuid) {
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let work_id = Uuid::new_v4();
    let opportunity_id = Uuid::new_v4();
    let kinds = [
        PipelineKind::LightweightTddDevelopment,
        PipelineKind::FullDesignToExecution,
    ];
    let mut manifest = PipelineRecommendationManifest {
        schema: PIPELINE_RECOMMENDATION_SCHEMA.into(),
        work_id,
        work_revision: 2,
        matrix_task_id: "task".into(),
        matrix_task_revision: "3".into(),
        selected_choice_id: "choice".into(),
        matrix_choice_set_digest: "a".repeat(64),
        matrix_verification_digest: "b".repeat(64),
        matrix_input_digest: "c".repeat(64),
        selected_candidate_digest: "d".repeat(64),
        compatibility_policy_digest: "e".repeat(64),
        mandatory_card_ids: vec!["card".into()],
        deterministic_kind: kinds[0],
        deterministic_option_id: None,
        catalogue_revision: "4".into(),
        catalogue_digest: "catalogue".into(),
        options: kinds
            .into_iter()
            .map(|kind| {
                let plan = PipelineVerificationPlan::from_definition(&definition(kind)).unwrap();
                PipelineRecommendationOption {
                    id: PipelineRecommendationOption::pair_id(kind, &plan.id),
                    kind,
                    definition_version: "1".into(),
                    definition_digest: "definition".into(),
                    completion_contract: "proof".into(),
                    forbidden_claims: vec![],
                    verification_plan: plan,
                }
            })
            .collect(),
        excluded: PipelineKind::CURRENT_SLICE_RUN_KINDS[2..]
            .iter()
            .map(|kind| tect_domain::PipelineExcludedKind {
                kind: *kind,
                reason: tect_domain::PipelineExclusionReason::MissingRule,
            })
            .collect(),
        evidence_refs: vec![],
        digest: String::new(),
    };
    manifest.deterministic_option_id = Some(manifest.options[0].id.clone());
    manifest.digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    let advice = match advice {
        PipelineDispositionAdvice::Ranked {
            dispatch_id,
            ranked_ids,
        } => PipelineDispositionAdvice::Ranked {
            dispatch_id,
            ranked_ids: ranked_ids
                .into_iter()
                .map(|id| {
                    manifest
                        .options
                        .iter()
                        .find(|option| option.kind.as_str() == id)
                        .map(|option| option.id.clone())
                        .unwrap_or(id)
                })
                .collect(),
        },
        other => other,
    };
    let work = SliceCandidateNode::Work {
        model_route_facts: None,
        id: work_id,
        revision: 2,
        title: "work".into(),
        outcome: "outcome".into(),
        includes: vec![],
        excludes: vec![],
        dependencies: vec![],
        proof: vec![],
        pipeline: kinds[0],
        pipeline_reason: "saved".into(),
        why_lightweight_insufficient: None,
        why_further_vertical_split_not_viable: None,
        source_result_ids: vec![],
        source_checkpoint: None,
    };
    let state = if matches!(advice, PipelineDispositionAdvice::NoCall) {
        AdvisoryOpportunityState::NoCall
    } else {
        AdvisoryOpportunityState::AwaitingResponse
    };
    let opportunity = AdvisoryOpportunity {
        id: opportunity_id,
        workspace_id: workspace,
        session_id: session,
        authorized_actor_id: actor,
        capability: AdvisoryCapability::PipelineRecommendation,
        decision_point: AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
        decision_point_version: 1,
        workflow_occurrence_key: "key".into(),
        target_kind: "slice_candidate_node".into(),
        target_id: Some(work_id),
        work_revision: Some(2),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: manifest.digest.clone(),
        state,
        primary_reason: if state == AdvisoryOpportunityState::NoCall {
            AdvisoryReason::WorkspaceDisabled
        } else {
            AdvisoryReason::SendUnknown
        },
        provider_called: state == AdvisoryOpportunityState::AwaitingResponse,
    };
    let context = PipelineRecommendationContext {
        scope_id: Uuid::new_v4(),
        candidate_set_id: Uuid::new_v4(),
        candidate_set_revision: 2,
        planning_snapshot_id: Uuid::new_v4(),
        source_snapshot_id: Uuid::new_v4(),
        source_snapshot_revision: "1".into(),
        source_snapshot_digest: "s".repeat(64),
        work_node_id: work_id,
        work_node_revision: 2,
        matrix_disposition_id: Uuid::new_v4(),
        match_effect_attestation_id: Uuid::new_v4(),
        catalogue_revision: "4".into(),
        catalogue_digest: "catalogue".into(),
        compatibility_policy_digest: manifest.compatibility_policy_digest.clone(),
        eligible_option_ids: manifest
            .options
            .iter()
            .map(|option| option.id.clone())
            .collect(),
        verification_contract_digest: manifest.digest.clone(),
    };
    let request = PipelineDispositionRequest {
        request_id: Uuid::new_v4(),
        opportunity_id,
        expected_work_revision: 2,
        manifest_digest: manifest.digest.clone(),
        action: PipelineRecommendationDisposition::AcceptRecommendation,
        rationale: "reviewed".into(),
    };
    let store = FakeStore {
        basis: PipelineDispositionBasis {
            prepared: PreparedPipelineRecommendation {
                opportunity,
                context,
                manifest,
            },
            saved_work: work,
            advice,
        },
        current: true,
        saved: None,
        writes: 0,
        concurrent_receipt: false,
    };
    (store, request, workspace, session, actor)
}

#[tokio::test]
async fn concurrent_identical_capture_returns_first_receipt() {
    let advice = PipelineDispositionAdvice::Ranked {
        dispatch_id: Uuid::new_v4(),
        ranked_ids: vec![
            PipelineKind::LightweightTddDevelopment.as_str().into(),
            PipelineKind::FullDesignToExecution.as_str().into(),
        ],
    };
    let (mut store, request, workspace, session, actor) = fixture(advice);
    store.concurrent_receipt = true;
    let returned = dispose_in_store(
        &mut store,
        workspace,
        session,
        actor,
        &request,
        &"e".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(store.saved.as_ref(), Some(&returned));
    assert_eq!(store.writes, 1);
}

#[tokio::test]
async fn ranked_accept_reject_replay_and_conflict() {
    let ranked = PipelineDispositionAdvice::Ranked {
        dispatch_id: Uuid::new_v4(),
        ranked_ids: vec![
            PipelineKind::FullDesignToExecution.as_str().into(),
            PipelineKind::LightweightTddDevelopment.as_str().into(),
        ],
    };
    let (mut store, request, workspace, session, actor) = fixture(ranked);
    let result = dispose_in_store(
        &mut store,
        workspace,
        session,
        actor,
        &request,
        &"e".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(
        result.selected_kind,
        Some(PipelineKind::FullDesignToExecution)
    );
    assert_eq!(
        dispose_in_store(
            &mut store,
            workspace,
            session,
            actor,
            &request,
            &"e".repeat(64)
        )
        .await
        .unwrap(),
        result
    );
    assert_eq!(store.writes, 1);
    let mut changed = request.clone();
    changed.action = PipelineRecommendationDisposition::RejectRecommendation;
    assert_eq!(
        dispose_in_store(
            &mut store,
            workspace,
            session,
            actor,
            &changed,
            &"e".repeat(64)
        )
        .await,
        Err(Error::InputConflict)
    );
    let (mut reject_store, _, _, _, _) = fixture(PipelineDispositionAdvice::Ranked {
        dispatch_id: Uuid::new_v4(),
        ranked_ids: vec![
            PipelineKind::LightweightTddDevelopment.as_str().into(),
            PipelineKind::FullDesignToExecution.as_str().into(),
        ],
    });
    let mut reject = request;
    reject.opportunity_id = reject_store.basis.prepared.opportunity.id;
    reject.manifest_digest = reject_store.basis.prepared.manifest.digest.clone();
    reject.action = PipelineRecommendationDisposition::RejectRecommendation;
    let own = &reject_store.basis.prepared.opportunity;
    let (own_workspace, own_session, own_actor) =
        (own.workspace_id, own.session_id, own.authorized_actor_id);
    assert_eq!(
        dispose_in_store(
            &mut reject_store,
            own_workspace,
            own_session,
            own_actor,
            &reject,
            &"e".repeat(64),
        )
        .await
        .unwrap()
        .selected_kind,
        None
    );
}

#[tokio::test]
async fn no_call_abstention_stale_and_unknown_never_write() {
    for advice in [
        PipelineDispositionAdvice::NoCall,
        PipelineDispositionAdvice::Abstained {
            dispatch_id: Uuid::new_v4(),
        },
    ] {
        let (mut store, mut request, workspace, session, actor) = fixture(advice);
        request.action = PipelineRecommendationDisposition::UseDeterministicChoice;
        let result = dispose_in_store(
            &mut store,
            workspace,
            session,
            actor,
            &request,
            &"e".repeat(64),
        )
        .await
        .unwrap();
        assert_eq!(
            result.selected_kind,
            Some(PipelineKind::LightweightTddDevelopment)
        );
    }
    let (mut stale, request, workspace, session, actor) =
        fixture(PipelineDispositionAdvice::NoCall);
    stale.current = false;
    assert_eq!(
        dispose_in_store(
            &mut stale,
            workspace,
            session,
            actor,
            &request,
            &"e".repeat(64)
        )
        .await,
        Err(Error::StaleContext)
    );
    assert_eq!(stale.writes, 0);
    let (mut unknown, request, workspace, session, actor) =
        fixture(PipelineDispositionAdvice::Ranked {
            dispatch_id: Uuid::new_v4(),
            ranked_ids: vec![
                "unknown".into(),
                PipelineKind::LightweightTddDevelopment.as_str().into(),
            ],
        });
    assert_eq!(
        dispose_in_store(
            &mut unknown,
            workspace,
            session,
            actor,
            &request,
            &"e".repeat(64)
        )
        .await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(unknown.writes, 0);
}
