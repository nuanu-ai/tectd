use super::live_support::{D, manifest, reseal_manifest, rw};
use super::*;
use crate::{PgStore, admin, store::PgUnitOfWork};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tect_application::{
    AntiBloatApplication, AntiBloatAuthoredDelta, AntiBloatPreparedRequest, AntiBloatStore,
    AntiBloatVerificationEvidence, DisabledAntiBloatRankingProvider, SetupFiles, SourceInspector,
    UnitOfWork, VerifyAntiBloatApply, WorkspaceService,
};

struct UnusedVerifierAdapters;

#[async_trait]
impl SourceInspector for UnusedVerifierAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl SetupFiles for UnusedVerifierAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<SetupDirectory> {
        Err(Error::InternalInvariant)
    }
    fn inspect(&self, _: &SetupDirectory, _: usize) -> Result<FileObservation> {
        Err(Error::InternalInvariant)
    }
    fn publish(&self, _: &SetupDirectory, _: &str) -> Result<FilePublication> {
        Err(Error::InternalInvariant)
    }
}

#[test]
fn trusted_graph_links_every_goal_so_even_duplicate_candidates_are_not_rankable() {
    let candidate_set = Uuid::new_v4();
    let source_ref = Uuid::new_v4();
    let mut authored = manifest(
        candidate_set,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &[(source_ref, D)],
    );
    authored.constructor = source_authored_identity();
    let baseline = &mut authored.emitted[0];
    let duplicate_id = Uuid::new_v4();
    let duplicate_goal_id = Uuid::new_v4();
    let mut duplicate = baseline.material.candidates[0].clone();
    duplicate.id = duplicate_id;
    duplicate.coverage_goal_ids = vec![duplicate_goal_id];
    let mut duplicate_goal = baseline.material.goals[0].clone();
    duplicate_goal.id = duplicate_goal_id;
    duplicate_goal.resolution.id = duplicate_id;
    baseline.material.candidates.push(duplicate);
    baseline.material.goals.push(duplicate_goal);
    baseline.material.delta.added.push(CandidateAdded {
        candidate_id: duplicate_id,
        revision: 1,
    });
    baseline.material_digest =
        scope_candidate_material_digest(&Sha256ScopeDigest, &baseline.material).unwrap();
    reseal_manifest(&mut authored);
    authored.validate(&Sha256ScopeDigest).unwrap();
    let (links, dependency_digest, graph_provenance) =
        authored_graph_binding(&authored, &[]).unwrap();
    assert_eq!(links.len(), 2);
    let protected_obligations = authored
        .obligations
        .iter()
        .map(|source| AntiBloatProtectedObligation {
            id: format!("scope-ref:{}", source.id),
            content_digest: source.statement_digest.clone(),
            origin: AntiBloatObligationOrigin::ScopeSource,
            scope_candidate_id: None,
        })
        .collect::<Vec<_>>();
    let protected_obligations_digest =
        anti_bloat_protected_obligations_digest(&Sha256ScopeDigest, &protected_obligations)
            .unwrap();
    let input = AntiBloatInput {
        selected_revision: authored.source.candidate_set_revision + 1,
        selected_id: authored.baseline_id.clone(),
        manifest: authored,
        graph_provenance,
        dependency_digest,
        obligation_links: links,
        non_goal_source_obligation_ids: vec![],
        mandatory_policy_obligation_ids: vec![],
        protected_obligations,
        protected_obligations_digest,
    };
    let review = review_anti_bloat(&Sha256ScopeDigest, &input).unwrap();
    assert_eq!(review.findings.len(), 2);
    assert!(
        review.findings.iter().all(|finding| {
            finding.class == AntiBloatClass::NecessaryResult && !finding.rankable
        })
    );
}

async fn apply_once(
    pool: &sqlx::PgPool,
    tenant: Uuid,
    auth: &HostAuth,
    delta: &AntiBloatAuthoredDelta,
) -> AntiBloatApplyReceipt {
    let mut unit = PgUnitOfWork::test_begin(pool, tenant).await;
    unit.authenticate(auth)
        .await
        .expect("authenticated synthetic caller");
    let mut app = AntiBloatApplication {
        store: unit,
        provider: DisabledAntiBloatRankingProvider,
    };
    let receipt = app
        .disposition_and_apply(delta)
        .await
        .expect("concurrent same-intent apply");
    Box::new(app.store)
        .commit()
        .await
        .expect("atomic caller effect commit");
    receipt
}

include!("anti_bloat_live_tests/part0.rs");
include!("anti_bloat_live_tests/part1.rs");
include!("anti_bloat_live_tests/part2.rs");
#[tokio::test]
#[ignore = "requires owned disposable PG fixture"]
async fn selected_save_activates_exact_source_bound_anti_bloat_review() {
    anti_core_part_0!(
        active,
        actor,
        adapters,
        admin_pool,
        advice,
        advice_request,
        advice_writer,
        after,
        app,
        app_reader,
        apply,
        apply_store,
        attestation,
        attestation_count,
        authored,
        authored_delta,
        before,
        before_app,
        binding,
        bytes,
        caller_link,
        candidate,
        ceilings,
        decider,
        dispatch_id,
        disposition,
        draft,
        eligible,
        enrollment,
        evidence_digest,
        exploratory,
        finding,
        foreign,
        foreign_context,
        foreign_session,
        foreign_verifier,
        foreign_workspace,
        from,
        id,
        install,
        later,
        lineage,
        link,
        now,
        observed,
        opportunity,
        ordinary,
        ordinary_writer,
        other_workspace,
        owner_context,
        persisted,
        policies,
        policy,
        prepared,
        program,
        program_body,
        reader,
        receipt,
        record,
        replay,
        replay_store,
        request,
        reservation_count,
        resolved,
        resolver,
        review_count,
        runtime_pool,
        runtime_url,
        save,
        saver,
        seed,
        selected,
        selected_app,
        selected_reader,
        service,
        session,
        shadow_count,
        snapshot,
        source_digest,
        source_ref,
        stale,
        stale_review,
        stale_verify,
        store,
        stored,
        tenant,
        unchanged_revision,
        until,
        verifier,
        verifier_context,
        verifier_session,
        verify,
        workspace,
        workspace_key,
        writer,
        wrong,
        wrong_digest
    );
    anti_core_part_1!(
        active,
        actor,
        adapters,
        admin_pool,
        advice,
        advice_request,
        advice_writer,
        after,
        app,
        app_reader,
        apply,
        apply_store,
        attestation,
        attestation_count,
        authored,
        authored_delta,
        before,
        before_app,
        binding,
        bytes,
        caller_link,
        candidate,
        ceilings,
        decider,
        dispatch_id,
        disposition,
        draft,
        eligible,
        enrollment,
        evidence_digest,
        exploratory,
        finding,
        foreign,
        foreign_context,
        foreign_session,
        foreign_verifier,
        foreign_workspace,
        from,
        id,
        install,
        later,
        lineage,
        link,
        now,
        observed,
        opportunity,
        ordinary,
        ordinary_writer,
        other_workspace,
        owner_context,
        persisted,
        policies,
        policy,
        prepared,
        program,
        program_body,
        reader,
        receipt,
        record,
        replay,
        replay_store,
        request,
        reservation_count,
        resolved,
        resolver,
        review_count,
        runtime_pool,
        runtime_url,
        save,
        saver,
        seed,
        selected,
        selected_app,
        selected_reader,
        service,
        session,
        shadow_count,
        snapshot,
        source_digest,
        source_ref,
        stale,
        stale_review,
        stale_verify,
        store,
        stored,
        tenant,
        unchanged_revision,
        until,
        verifier,
        verifier_context,
        verifier_session,
        verify,
        workspace,
        workspace_key,
        writer,
        wrong,
        wrong_digest
    );
    anti_core_part_2!(
        active,
        actor,
        adapters,
        admin_pool,
        advice,
        advice_request,
        advice_writer,
        after,
        app,
        app_reader,
        apply,
        apply_store,
        attestation,
        attestation_count,
        authored,
        authored_delta,
        before,
        before_app,
        binding,
        bytes,
        caller_link,
        candidate,
        ceilings,
        decider,
        dispatch_id,
        disposition,
        draft,
        eligible,
        enrollment,
        evidence_digest,
        exploratory,
        finding,
        foreign,
        foreign_context,
        foreign_session,
        foreign_verifier,
        foreign_workspace,
        from,
        id,
        install,
        later,
        lineage,
        link,
        now,
        observed,
        opportunity,
        ordinary,
        ordinary_writer,
        other_workspace,
        owner_context,
        persisted,
        policies,
        policy,
        prepared,
        program,
        program_body,
        reader,
        receipt,
        record,
        replay,
        replay_store,
        request,
        reservation_count,
        resolved,
        resolver,
        review_count,
        runtime_pool,
        runtime_url,
        save,
        saver,
        seed,
        selected,
        selected_app,
        selected_reader,
        service,
        session,
        shadow_count,
        snapshot,
        source_digest,
        source_ref,
        stale,
        stale_review,
        stale_verify,
        store,
        stored,
        tenant,
        unchanged_revision,
        until,
        verifier,
        verifier_context,
        verifier_session,
        verify,
        workspace,
        workspace_key,
        writer,
        wrong,
        wrong_digest
    );
}

mod native_bridge;
