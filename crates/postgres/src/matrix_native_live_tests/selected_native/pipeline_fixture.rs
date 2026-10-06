use serde_json::{Value, json};
use tect_application::{
    FixedPipelineCompatibilityPolicy, NativePlanningGuidance, PipelineDefinitionProvider,
    PipelineExecutionOutputGuard, PipelineProviderObservation,
    PipelineRecommendationDefinitionProvider, PipelineRecommendationProvider,
    PipelineStartedDispatchPermit, PreparedPipelineRecommendation,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse,
};

#[derive(Clone)]
struct Definitions(PipelineDefinitionSnapshot);

fn definitions() -> Definitions {
    // Exact existing host Debug JSON, NOT host/provider runtime acceptance.
    let value: PipelineDefinitionSnapshot = serde_json::from_str(include_str!(
        "../../../../host/pipeline-definitions/debug-root-cause.json"
    ))
    .unwrap();
    assert_eq!(value.kind, PipelineKind::DebugRootCause);
    assert_eq!(
        value.digest,
        "afb0f21932a11eceb8e3aba01d3d07ec9f74160203085391f7de1758118a6574"
    );
    let mut material = value.clone();
    material.digest.clear();
    assert_eq!(
        value.digest,
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&material).unwrap())
        )
    );
    for body in std::iter::once(&value.overview).chain(value.phases.iter().flat_map(|phase| {
        phase
            .instructions
            .iter()
            .chain(&phase.skills)
            .chain(&phase.resources)
    })) {
        assert_eq!(
            body.digest,
            format!("{:x}", Sha256::digest(body.body.as_bytes()))
        );
    }
    value.validate().unwrap();
    Definitions(value)
}

impl PipelineRecommendationDefinitionProvider for Definitions {
    fn definition(
        &self,
        revision: &str,
        kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>> {
        Ok((revision == "4" && kind == PipelineKind::DebugRootCause).then(|| self.0.clone()))
    }
}
impl PipelineDefinitionProvider for Definitions {
    fn definition(&self, kind: PipelineKind) -> Result<PipelineDefinitionSnapshot> {
        if kind != self.0.kind {
            return Err(Error::InvalidArguments);
        }
        Ok(self.0.clone())
    }
}

struct Guidance(Definitions);
impl NativePlanningGuidance for Guidance {
    fn snapshot(
        &self,
        basis: &ScopeOpenBasis,
        inputs: &[SlicePlanningInput],
        results: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        let mut material = native::Guidance.snapshot(basis, inputs, results)?;
        let template = material.catalogue.entries[0].clone();
        material.catalogue.entries = PipelineKind::CURRENT_SLICE_RUN_KINDS
            .into_iter()
            .chain([PipelineKind::PromoteToDurableKnowledge])
            .map(|kind| {
                let mut entry = template.clone();
                entry.kind = kind;
                if kind == PipelineKind::DebugRootCause {
                    entry.default_delivery_mode = Some(self.0.0.default_mode);
                    entry.allowed_delivery_modes = self.0.0.allowed_modes.clone();
                }
                if kind == PipelineKind::PromoteToDurableKnowledge {
                    entry.execution_owner = PipelineExecutionOwner::KnowledgeChange;
                }
                entry
            })
            .collect();
        material.catalogue.revision = "4".into();
        material.catalogue.digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&material.catalogue.entries).unwrap())
        );
        material.catalogue.validate()?;
        Ok(material)
    }
}

struct NoTransport(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl PipelineRecommendationProvider for NoTransport {
    fn prepare(
        &self,
        _: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::TransportUnavailable)
    }
    fn parse_sealed_response(
        &self,
        _: &PipelineRecommendationManifest,
        _: &PreparedPipelineRecommendationAttempt,
        _: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::TransportUnavailable)
    }
    async fn attempt_prepared(
        &self,
        _: PreparedPipelineRecommendationAttempt,
        _: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::TransportUnavailable)
    }
}

fn policy(
    source: &MatrixTaskSource,
    cards: &EngineeringMatrixComposition,
    definitions: &Definitions,
) -> PipelineCompatibilityPolicy {
    // Synthetic test policy, NOT real Owner compatibility approval.
    let plan = PipelineVerificationPlan::from_definition(&definitions.0).unwrap();
    let obligation = plan.obligations.first().unwrap();
    let MatrixFact::Known { value: mode, .. } = &source.revision.input.mode else {
        panic!("fixture must retain known declared mode");
    };
    let value = PipelineCompatibilityPolicy {
        version: PIPELINE_COMPATIBILITY_POLICY_VERSION.into(),
        task_id: source.revision.task_id.to_string(),
        task_revision: source.revision.revision.to_string(),
        catalogue_revision: "4".into(),
        rules: vec![PipelineCompatibilityRule {
            kind: PipelineKind::DebugRootCause,
            matrix_input_digest: matrix_input_digest(&source.revision.input).unwrap(),
            allowed_modes: vec![*mode],
            selected_candidate_ids: vec!["a".into()],
            card_coverage: cards
                .mandatory_cards
                .iter()
                .map(|card| PipelineCardCoverage {
                    card_id: card.id.to_string(),
                    phase_id: obligation.phase_id.clone(),
                    obligation_digest: pipeline_obligation_digest(obligation).unwrap(),
                })
                .collect(),
        }],
    };
    value.validate_host_snapshot("4").unwrap();
    value
}

struct RunGuard;
impl PipelineExecutionOutputGuard for RunGuard {
    fn check_context(&self, _: &PipelineRunContext) -> Result<()> {
        Ok(())
    }
    fn check_begin(&self, _: &BeginPipelineRunOutcome) -> Result<()> {
        Ok(())
    }
    fn check_mutation(&self, _: &PipelineMutationOutcome) -> Result<()> {
        Ok(())
    }
    fn check_checkpoint_resolution(&self, _: &ResolvePipelineCheckpointOutcome) -> Result<()> {
        Ok(())
    }
}

async fn readback(pool: &PgPool, tenant: Uuid, workspace: Uuid) -> Value {
    let mut tx = pool.begin().await.unwrap();
    // Same tenant latch as PgUnitOfWork; SELECT only, no fixture row manufacture.
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut values = serde_json::Map::new();
    for table in [
        "pipeline_advice_contexts",
        "pipeline_advice_dispositions",
        "native_slices",
        "pipeline_open_effect_attestations",
        "slice_pipeline_runs",
        "advisory_dispatch",
        "pipeline_advice_interpretations",
    ] {
        let sql = format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]'::jsonb) FROM {table} r WHERE tenant_id=$1 AND workspace_id=$2"
        );
        let rows: Value = sqlx::query_scalar(&sql)
            .bind(tenant)
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        values.insert(table.into(), rows);
    }
    tx.commit().await.unwrap();
    Value::Object(values)
}
