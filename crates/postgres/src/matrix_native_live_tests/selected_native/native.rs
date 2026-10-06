use super::*;
use serde_json::{from_value, json};
use tect_application::{
    CandidateGuidance, CandidateOutputGuard, NativePlanningGuidance, NativePlanningOutputGuard,
};

pub(super) struct Guidance;
pub(super) struct Guard;

fn method() -> CandidateMethodSnapshot {
    let body = "Synthetic selected-native fixture method";
    CandidateMethodSnapshot {
        id: "selected-native-fixture".into(),
        revision: "1".into(),
        digest: format!("{:x}", Sha256::digest(body.as_bytes())),
        body: body.into(),
        origin_refs: vec!["synthetic-fixture:selected-native-method:v1".into()],
    }
}

#[test]
fn selected_native_method_satisfies_planning_capture_contract() {
    let method = method();
    PlanningMethodSnapshot::from_candidate(&method)
        .validate()
        .unwrap();
    assert_eq!(
        method.digest,
        "e4187fed9b013f46d9cae8ff05b39965be0a6a0a28445e4654655dd3b7c7d39a"
    );
    assert_eq!(
        method.origin_refs,
        vec!["synthetic-fixture:selected-native-method:v1"]
    );
}

impl CandidateGuidance for Guidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: "a".repeat(64),
            method: method(),
            registry_revision: "1".into(),
            registry_digest: "a".repeat(64),
            rules: vec![],
        })
    }
}

impl NativePlanningGuidance for Guidance {
    fn snapshot(
        &self,
        _: &ScopeOpenBasis,
        _: &[SlicePlanningInput],
        _: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        let entries = PipelineKind::HISTORICAL_SLICE_RUN_KINDS
            .into_iter()
            .map(|kind| PipelineCatalogueEntry {
                kind,
                description: "Fixture pipeline".into(),
                implementation_status: "executable".into(),
                description_status: "refined".into(),
                refinement_required: false,
                choose_when: "Fixture work".into(),
                do_not_choose_when: "Other work".into(),
                expected_result: "Fixture proof".into(),
                executable: true,
                default_delivery_mode: Some(PipelineDeliveryMode::Whole),
                allowed_delivery_modes: vec![PipelineDeliveryMode::Whole],
                execution_owner: PipelineExecutionOwner::SlicePipelineRun,
            })
            .collect();
        Ok(SlicePlanningSnapshotMaterial {
            method: method(),
            registry_revision: "1".into(),
            registry_digest: "a".repeat(64),
            rules: (0..4)
                .map(|index| CandidateRuleSnapshot {
                    id: format!("fixture-{index}"),
                    revision: "1".into(),
                    text: "Fixture rule".into(),
                    origin_refs: vec![],
                    applicability: vec![],
                })
                .collect(),
            catalogue: PipelineCatalogueSnapshot {
                revision: "1".into(),
                digest: "a".repeat(64),
                entries,
            },
        })
    }
}

impl CandidateOutputGuard for Guard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        Ok(input.len() as i64)
    }
    fn check_material(&self, _: &CandidateSnapshotMaterial) -> Result<()> {
        Ok(())
    }
    fn check_draft(&self, _: &ResolvedCandidateDraft) -> Result<()> {
        Ok(())
    }
    fn check_stored(&self, _: &StoredCandidateContext) -> Result<()> {
        Ok(())
    }
    fn check_begin(&self, _: &BeginCandidateSetOutcome) -> Result<()> {
        Ok(())
    }
}

impl NativePlanningOutputGuard for Guard {
    fn check_context(&self, _: &SliceCandidateContext) -> Result<()> {
        Ok(())
    }
    fn check_open_scope(&self, _: &OpenScopeOutcome) -> Result<()> {
        Ok(())
    }
}

pub(super) fn knowledge(status: Option<&PlanningKnowledgeStatus>) -> Option<PlanningManifestGuard> {
    let status = status.expect("public flow must capture planning knowledge");
    assert!(status.stale_reasons.is_empty());
    let manifest = status.manifest.as_ref().expect("captured manifest");
    Some(PlanningManifestGuard {
        manifest_id: manifest.id,
        digest: manifest.digest.clone(),
        workspace_generation: manifest.workspace_generation,
    })
}

pub(super) async fn planning(
    service: &WorkspaceService,
    context: &RequestContext,
    program: Uuid,
) -> SliceCandidateContext {
    planning_with_guidance(service, context, program, &Guidance).await
}

pub(super) async fn planning_with_guidance(
    service: &WorkspaceService,
    context: &RequestContext,
    program: Uuid,
    guidance: &dyn NativePlanningGuidance,
) -> SliceCandidateContext {
    let begin = service
        .begin_candidate_set(
            context,
            &BeginCandidateSet {
                request_id: Uuid::new_v4(),
                program_id: program,
                program_revision: 4,
                boundary: CandidateBoundary::Ongoing,
                input: "Plan one bounded native fixture Scope".into(),
                task_context: PlanningTaskContext::default(),
                advisory_preference: AdvisoryRequestPreference::Skip,
            },
            &Guidance,
            &Guard,
        )
        .await
        .unwrap();
    let candidate_context = match begin {
        BeginCandidateSetOutcome::Created(value)
        | BeginCandidateSetOutcome::Replay(value)
        | BeginCandidateSetOutcome::Existing(value) => value,
    };
    let source_ref = candidate_context.snapshot.source_refs.first().unwrap().id;
    let saved = service.save_candidate_draft(context, &SaveCandidateDraft {
        candidate_set_id: candidate_context.candidate_set.id,
        revision: candidate_context.candidate_set.revision,
        snapshot_id: candidate_context.snapshot.id, input_cursor: candidate_context.candidate_set.latest_input,
        request_id: Uuid::new_v4(), consumed_knowledge: knowledge(candidate_context.planning_knowledge.as_ref()),
        selected_advisory: None,
        draft: from_value(json!({
            "boundary":"ongoing","goals":[{"identity":{"local":"goal"},"text":"Selected fixture result",
                "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}}],
            "candidates":[{"identity":{"local":"scope"},"title":"Selected fixture","outcome":"Native fixture result",
                "trigger":"Selected choice","delivered_behavior":"Bounded result","proof":"Fixture assertions",
                "coverage_goals":[{"local":"goal"}]}]
        })).unwrap(),
    }, &Guidance, &Guard).await.unwrap();
    let candidate = &saved.draft.as_ref().unwrap().candidates[0];
    let reviewed = service.review_candidate_set(context, &ReviewCandidateSet {
        candidate_set_id: saved.context.candidate_set.id, revision: saved.context.candidate_set.revision,
        snapshot_id: saved.context.snapshot.id, input_cursor: saved.context.candidate_set.input_cursor,
        request_id: Uuid::new_v4(), consumed_knowledge: knowledge(saved.context.planning_knowledge.as_ref()),
        review: from_value(json!({"verdict":"ready","summary":"Fixture ready","findings":[],
            "candidate_decisions":[{"candidate_id":candidate.id,"decision":"accept","rationale":"Bounded fixture"}]})).unwrap(),
    }, &Guidance, &Guard).await.unwrap();
    let opened = service
        .scope_open(
            context,
            &OpenScope {
                request_id: Uuid::new_v4(),
                candidate_set_id: reviewed.context.candidate_set.id,
                candidate_set_revision: reviewed.context.candidate_set.revision,
                candidate_snapshot_id: reviewed.context.snapshot.id,
                candidate_id: candidate.id,
                candidate_revision: candidate.revision,
                task_context: PlanningTaskContext::default(),
                consumed_knowledge: knowledge(reviewed.context.planning_knowledge.as_ref()),
            },
            &Guidance,
            guidance,
            &Guard,
        )
        .await
        .unwrap();
    match opened {
        OpenScopeOutcome::Created(value) | OpenScopeOutcome::Replay(value) => value.planning,
    }
}

pub(super) fn save_request(
    context: &SliceCandidateContext,
    selection: MatrixPlanningSelection,
) -> SaveSliceCandidateDraft {
    SaveSliceCandidateDraft {
        scope_id: context.scope.id, candidate_set_id: context.candidate_set.id,
        revision: context.candidate_set.revision, snapshot_id: context.snapshot.id,
        input_cursor: context.candidate_set.input_cursor, request_id: Uuid::new_v4(),
        consumed_knowledge: knowledge(context.planning_knowledge.as_ref()), matrix_selection: Some(selection),
        draft: from_value(json!({"coverage_summary":"Selected native regression fixture","nodes":[{
            "kind":"work","identity":{"local":"selected"},"title":"Selected work","outcome":"Bounded result",
            "proof":["Regression fixture proof"],"pipeline":"slice.debug-root-cause","pipeline_reason":"Bounded diagnosis"
        }]})).unwrap(),
    }
}
