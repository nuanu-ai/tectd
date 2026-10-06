use super::*;
use tect_application::{AntiBloatAttemptState, StoredAntiBloatReview};
use tect_host::{JevAntiBloatConfig, JevAntiBloatProvider};

// Only this disposable fixture owns the deterministic private seed.
pub(super) fn signed_scope_budget_fixture(
    workspace: Uuid,
    owner: Uuid,
    calls: i64,
) -> (tect_domain::AdvisoryBudgetPolicy, crate::BudgetOwnerKeys) {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};

    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let from = now.checked_sub(60_000).unwrap();
    let until = now.checked_add(3_600_000).unwrap();
    let id = Uuid::new_v4();
    let version = 3;
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: calls,
        input_tokens: 1_024,
        output_tokens: 1_024,
        request_utf8_bytes: 65_536,
        elapsed_monotonic_ms: 10_000,
        retry_dispatches: 1,
    };
    let digest = AdvisoryBudgetPolicy::digest_for(id, version, from, until, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        version,
        digest.clone(),
        from,
        until,
        ceilings,
        owner,
        "0".repeat(128),
    )
    .unwrap();
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
    let signature = keypair.sign(&unsigned.approval_signing_message(workspace).unwrap());
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let signed = AdvisoryBudgetPolicy::new(
        id,
        version,
        digest,
        from,
        until,
        ceilings,
        owner,
        hex(signature.as_ref()),
    )
    .unwrap();
    let keys = crate::BudgetOwnerKeys::from_json(
        &serde_json::json!([{
            "workspace_id": workspace,
            "owner_id": owner,
            "public_key_hex": hex(keypair.public_key().as_ref()),
        }])
        .to_string(),
    )
    .unwrap();
    (signed, keys)
}

pub(super) async fn run(
    admin_pool: &sqlx::PgPool,
    runtime_pool: &sqlx::PgPool,
    store: &PgStore,
    enrollment: &crate::admin::Enrollment,
    tenant: Uuid,
    actor: Uuid,
    workspace: Uuid,
    session: Uuid,
    candidate: Uuid,
    old: &StoredAntiBloatReview,
    authored_delta: &AntiBloatAuthoredDelta,
    resolved: &ResolvedCandidateDraft,
) {
    let (policy, keys) = signed_scope_budget_fixture(workspace, actor, 2);
    let trusted = store.clone().with_budget_owner_keys(keys);
    let mut tx = rw(&trusted, &enrollment.auth, tenant).await;
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (endpoint, done, server) = super::native_transport::once().await;
    let provider = JevAntiBloatProvider::new(
        JevAntiBloatConfig {
            profile: "fixture".into(),
            endpoint,
            model: "jev".into(),
            timeout: std::time::Duration::from_secs(2),
            maximum_request_bytes: 65536,
            maximum_response_bytes: 16384,
        },
        "fake-local-key".into(),
    )
    .unwrap();
    let adapters = Arc::new(UnusedVerifierAdapters);
    let service = WorkspaceService::new(Arc::new(trusted), adapters.clone(), adapters)
        .with_anti_bloat_provider(Arc::new(provider));
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("anti-bloat-live-{workspace}"),
    };
    let native = service
        .prepare_anti_bloat(
            &context,
            candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    assert_eq!(native.state, AntiBloatAttemptState::Prepared);
    assert_eq!(
        service
            .run_anti_bloat_once(&context, native.review_id)
            .await
            .unwrap(),
        AntiBloatAttemptState::Ranked(vec![authored_delta.finding_id.clone()])
    );
    assert_eq!(
        service
            .run_anti_bloat_once(&context, native.review_id)
            .await
            .unwrap(),
        AntiBloatAttemptState::Ranked(vec![authored_delta.finding_id.clone()])
    );
    done.send(()).unwrap();
    let (request, response, second) = server.await.unwrap();
    assert!(!second, "recovery must not dispatch twice");
    let persisted:(Vec<u8>,Vec<u8>,String,bool)=sqlx::query_as("SELECT request_bytes,raw_response,state,response_complete FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2").bind(tenant).bind(native.review_id).fetch_one(admin_pool).await.unwrap();
    assert_eq!(persisted.0, request);
    assert_eq!(persisted.1, response);
    assert_eq!(persisted.2, "ranked");
    assert!(persisted.3);
    let usage:(i64,i64,bool,bool)=sqlx::query_as("SELECT input_tokens,output_tokens,unknown_usage,exhausted_after_response FROM scope_anti_bloat_budget_consumptions WHERE tenant_id=$1 AND review_id=$2").bind(tenant).bind(native.review_id).fetch_one(admin_pool).await.unwrap();
    assert_eq!(usage, (20, 30, false, false));
    let totals: (i64, i64, i64, i64, i64, i64, i64, i64) =
        sqlx::query_as("SELECT * FROM advisory_budget_policy_usage_totals($1,$2,$3,$4,$5)")
            .bind(tenant)
            .bind(workspace)
            .bind(policy.id())
            .bind(policy.version())
            .bind(policy.digest())
            .fetch_one(admin_pool)
            .await
            .unwrap();
    assert_eq!(
        (totals.0, totals.2, totals.3, totals.4, totals.6, totals.7),
        (1, 0, 20, 30, 0, 0)
    );
    assert_eq!(totals.1, request.len() as i64);
    let mut forbidden = authored_delta.clone();
    forbidden.review_id = native.review_id;
    let required = resolved
        .candidates
        .iter()
        .find(|v| v.grounding.is_source_grounded())
        .unwrap();
    let review = service
        .get_anti_bloat(&context, native.review_id)
        .await
        .unwrap();
    let protected_finding = review
        .review
        .findings
        .iter()
        .find(|v| v.candidate_id == required.id)
        .unwrap();
    assert_eq!(protected_finding.class, AntiBloatClass::NecessaryResult);
    assert!(!protected_finding.rankable);
    forbidden.finding_id = protected_finding.id.clone();
    forbidden.delta.operations = vec![CandidateDeltaOperation::CandidateRemove {
        candidate_id: required.id,
        expected_revision: required.revision,
    }];
    assert!(
        service
            .apply_anti_bloat(&context, &forbidden)
            .await
            .is_err()
    );
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM scope_candidate_sets WHERE id=$1")
        .bind(candidate)
        .fetch_one(admin_pool)
        .await
        .unwrap();
    assert_eq!(revision, 4);
    let denied_effects: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_caller_links WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(admin_pool)
    .await
    .unwrap();
    assert_eq!(denied_effects, 0);
    let mut delta = authored_delta.clone();
    delta.review_id = native.review_id;
    let receipt = service.apply_anti_bloat(&context, &delta).await.unwrap();
    assert_eq!((receipt.from_revision, receipt.to_revision), (4, 5));
    assert_eq!(
        service.apply_anti_bloat(&context, &delta).await.unwrap(),
        receipt
    );
    let mut stale = delta.clone();
    stale.review_id = old.review_id;
    stale.delta.idempotency_key.push_str("-stale");
    assert_eq!(
        service.apply_anti_bloat(&context, &stale).await,
        Err(Error::InputConflict)
    );
    let verifier = admin::prepare_verifier_enrollment(admin_pool, tenant, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)").bind(verifier_session).bind(tenant).bind(verifier.auth.host_id).bind(workspace).bind(verifier_session.to_string()).execute(admin_pool).await.unwrap();
    let vc = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: verifier_session.to_string(),
        workspace_key: context.workspace_key.clone(),
    };
    let (material, digest) = service
        .get_anti_bloat_verification_material(&vc, native.review_id)
        .await
        .unwrap();
    assert_eq!(material.verdict().0, AntiBloatVerificationVerdict::Pass);
    let attestation = service
        .verify_anti_bloat_apply(
            &vc,
            &VerifyAntiBloatApply {
                request_id: Uuid::new_v4(),
                review_id: native.review_id,
                expected_evidence_digest: digest,
            },
        )
        .await
        .unwrap();
    assert_eq!(attestation.verdict, AntiBloatVerificationVerdict::Pass);
    assert_ne!(verifier.principal_id, actor);
    println!(
        "synthetic native bridge review={} workspace={} bytes={} raw={} calls=1 usage=20/30 revision=4->5 distinct_verifier={}",
        native.review_id,
        workspace,
        request.len(),
        response.len(),
        verifier.principal_id
    );
    runtime_pool.close().await;
    admin_pool.close().await;
}
