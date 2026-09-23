use super::*;
use crate::PgStore;
use tect_application::{Store, TransactionMode};

pub(super) const D: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) fn manifest(candidate: Uuid, snapshot: Uuid, program: Uuid) -> ScopeConstructorManifest {
    let mut source = FrozenScopeSource {
        candidate_set_id: candidate,
        candidate_set_revision: 3,
        snapshot_id: snapshot,
        input_cursor: 2,
        program_id: program,
        program_revision: 4,
        program_latest_input: 2,
        planning_latest_input: 2,
        selected_sources_digest: D.into(),
        method_revision: "4".into(),
        method_digest: D.into(),
        registry_revision: "3".into(),
        registry_digest: D.into(),
        inputs: vec![FrozenSourceInput {
            id: "program.intent".into(),
            version: "4".into(),
            digest: D.into(),
            provenance: "program.intent@4".into(),
            applicability: SourceApplicability::Applicable,
        }],
        digest: String::new(),
    };
    source.digest = source.canonical_digest(&Sha256ScopeDigest).unwrap();
    let obligations = vec![SourceObligation {
        id: "obligation.intent".into(),
        source_input_id: "program.intent".into(),
        statement_digest: D.into(),
        conditions: vec![],
        exceptions: vec![],
    }];
    let constructor = ScopeConstructorIdentity {
        id: "fixture-constructor".into(),
        version: "1".into(),
        digest: D.into(),
    };
    let coverage = vec![ObligationCoverage {
        obligation_id: "obligation.intent".into(),
        condition_ids: vec![],
        exception_ids: vec![],
    }];
    let goal_id = Uuid::new_v4();
    let candidate_id = Uuid::new_v4();
    let material = ResolvedCandidateDraft {
        boundary: CandidateBoundary::Finite,
        goals: vec![CoverageGoalEntity {
            id: goal_id,
            revision: 1,
            text: "Preserve source".into(),
            source_ref_id: Uuid::new_v4(),
            exact_quote: None,
            resolution: CoverageResolutionEntity {
                kind: CoverageResolutionKind::Candidate,
                id: candidate_id,
            },
        }],
        evidence: vec![],
        candidates: vec![CandidateEntity {
            id: candidate_id,
            revision: 1,
            title: "Cohesive".into(),
            outcome: "Exact outcome".into(),
            trigger: "Exact trigger".into(),
            delivered_behavior: "Exact behavior".into(),
            proof: "Exact proof".into(),
            includes: vec!["source".into()],
            excludes: vec![],
            dependencies: vec![],
            coverage_goal_ids: vec![goal_id],
            evidence_ids: vec![],
        }],
        blockers: vec![],
        pending_question: None,
        empty_disposition: None,
        protected_changes: vec![],
        delta: CandidateDelta {
            added: vec![CandidateAdded {
                candidate_id,
                revision: 1,
            }],
            ..CandidateDelta::default()
        },
    };
    let material_digest = scope_candidate_material_digest(&Sha256ScopeDigest, &material).unwrap();
    let id = stable_scope_alternative_id(
        &Sha256ScopeDigest,
        &constructor,
        &source.digest,
        ScopeDecompositionKind::Cohesive,
        &material_digest,
        &coverage,
    )
    .unwrap();
    let alternative = ScopeDecompositionAlternative {
        id: id.clone(),
        kind: ScopeDecompositionKind::Cohesive,
        material,
        material_digest,
        coverage,
    };
    let mut value = ScopeConstructorManifest {
        constructor,
        source,
        obligations,
        emitted: vec![alternative],
        rejected: vec![],
        baseline_id: id.clone(),
        ordered_ids: vec![id],
        eligible_set_digest: String::new(),
        whole_set_digest: String::new(),
    };
    value.eligible_set_digest = value
        .canonical_eligible_set_digest(&Sha256ScopeDigest)
        .unwrap();
    value.whole_set_digest = value
        .canonical_whole_set_digest(&Sha256ScopeDigest)
        .unwrap();
    value
}

pub(super) async fn rw(
    store: &PgStore,
    auth: &HostAuth,
    tenant: Uuid,
) -> Box<dyn tect_application::UnitOfWork> {
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(auth).await.unwrap();
    unit.set_tenant(tenant).await.unwrap();
    unit
}

pub(super) async fn set_config(pool: &sqlx::PgPool, tenant: Uuid, workspace: Uuid, enabled: bool) {
    let sql = if enabled {
        "UPDATE advisory_workspace_config SET revision=1,mode='optional',provider_profile_ref='fixture',model_configuration='{\"model\":\"jev\"}' WHERE tenant_id=$1 AND workspace_id=$2"
    } else {
        "UPDATE advisory_workspace_config SET revision=0,mode='disabled',provider_profile_ref=NULL,model_configuration=NULL WHERE tenant_id=$1 AND workspace_id=$2"
    };
    sqlx::query(sql)
        .bind(tenant)
        .bind(workspace)
        .execute(pool)
        .await
        .unwrap();
}
