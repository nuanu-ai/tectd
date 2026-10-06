macro_rules! anti_core_part_2 { ($active:ident, $actor:ident, $adapters:ident, $admin_pool:ident, $advice:ident, $advice_request:ident, $advice_writer:ident, $after:ident, $app:ident, $app_reader:ident, $apply:ident, $apply_store:ident, $attestation:ident, $attestation_count:ident, $authored:ident, $authored_delta:ident, $before:ident, $before_app:ident, $binding:ident, $bytes:ident, $caller_link:ident, $candidate:ident, $ceilings:ident, $decider:ident, $dispatch_id:ident, $disposition:ident, $draft:ident, $eligible:ident, $enrollment:ident, $evidence_digest:ident, $exploratory:ident, $finding:ident, $foreign:ident, $foreign_context:ident, $foreign_session:ident, $foreign_verifier:ident, $foreign_workspace:ident, $from:ident, $id:ident, $install:ident, $later:ident, $lineage:ident, $link:ident, $now:ident, $observed:ident, $opportunity:ident, $ordinary:ident, $ordinary_writer:ident, $other_workspace:ident, $owner_context:ident, $persisted:ident, $policies:ident, $policy:ident, $prepared:ident, $program:ident, $program_body:ident, $reader:ident, $receipt:ident, $record:ident, $replay:ident, $replay_store:ident, $request:ident, $reservation_count:ident, $resolved:ident, $resolver:ident, $review_count:ident, $runtime_pool:ident, $runtime_url:ident, $save:ident, $saver:ident, $seed:ident, $selected:ident, $selected_app:ident, $selected_reader:ident, $service:ident, $session:ident, $shadow_count:ident, $snapshot:ident, $source_digest:ident, $source_ref:ident, $stale:ident, $stale_review:ident, $stale_verify:ident, $store:ident, $stored:ident, $tenant:ident, $unchanged_revision:ident, $until:ident, $verifier:ident, $verifier_context:ident, $verifier_session:ident, $verify:ident, $workspace:ident, $workspace_key:ident, $writer:ident, $wrong:ident, $wrong_digest:ident) => {
    let (first, replay) = tokio::join!(
        apply_once(&$runtime_pool, $tenant, &$enrollment.auth, &$authored_delta),
        apply_once(&$runtime_pool, $tenant, &$enrollment.auth, &$authored_delta),
    );
    assert_eq!(first, replay);
    let $receipt = first;
    assert_eq!(($receipt.from_revision, $receipt.to_revision), (4, 5));
    assert_eq!($receipt.$source_digest, $authored.source.digest);
    assert_eq!($receipt.before_material_digest, $authored.emitted[0].material_digest);
    let $persisted: (i64, serde_json::Value) = sqlx::query_as(
        "SELECT s.revision,d.payload FROM scope_candidate_sets s JOIN scope_candidate_drafts d \
         ON (d.tenant_id,d.workspace_id,d.candidate_set_id,d.set_revision)= \
            (s.tenant_id,s.workspace_id,s.id,s.revision) \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind($candidate)
    .fetch_one(&$admin_pool)
    .await
    .expect("part2.rs:27");
    assert_eq!($persisted.0, 5);
    let $after: ResolvedCandidateDraft = serde_json::from_value($persisted.1).expect("part2.rs:29");
    assert_eq!($after.candidates.len(), 1);
    assert_eq!(
        $after.candidates[0].grounding,
        CandidateGrounding::SourceGrounded
    );
    assert_eq!($after.goals, $resolved.goals);
    assert_eq!(
        scope_candidate_material_digest(&Sha256ScopeDigest, &$after).expect("part2.rs:37"),
        $receipt.after_material_digest
    );
    let $link: (Uuid, i64, i64, String, serde_json::Value) = sqlx::query_as(
        "SELECT caller_request_id,from_revision,to_revision,source_digest,caller_receipt \
         FROM scope_anti_bloat_caller_links WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind($tenant).bind($workspace).bind($prepared.review_id).fetch_one(&$admin_pool).await.expect("part2.rs:44");
    assert_eq!(
        ($link.0, $link.1, $link.2, $link.3),
        (
            $receipt.caller_request_id,
            4,
            5,
            $receipt.$source_digest.clone()
        )
    );
    assert_eq!(
        serde_json::from_value::<AntiBloatApplyReceipt>($link.4).expect("part2.rs:55"),
        $receipt
    );
    let $shadow_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3",
    )
    .bind($tenant).bind($workspace).bind($candidate).fetch_one(&$admin_pool).await.expect("part2.rs:61");
    assert_eq!($shadow_count, 0);
    let mut $replay_store = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    $replay_store.authenticate(&$enrollment.auth).await.expect("part2.rs:64");
    let mut $replay = AntiBloatApplication {
        $store: $replay_store,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        $replay.disposition_and_apply(&$authored_delta).await.expect("part2.rs:70"),
        $receipt
    );
    let mut $wrong = $authored_delta.clone();
    $wrong.delta.idempotency_key.push_str("-different");
    assert_eq!(
        $replay.disposition_and_apply(&$wrong).await,
        Err(Error::InputConflict)
    );
    let mut $stale = $authored_delta.clone();
    $stale.review_id = $stale_review.review_id;
    $stale.delta.idempotency_key.push_str("-stale");
    assert_eq!(
        $replay.disposition_and_apply(&$stale).await,
        Err(Error::InputConflict)
    );
    Box::new($replay.$store).commit().await.expect("part2.rs:86");

    let $verifier = admin::prepare_verifier_enrollment(&$admin_pool, $tenant, $workspace)
        .await
        .expect("part2.rs:90")
        .try_commit()
        .await
        .expect("part2.rs:93");
    let $verifier_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind($tenant).bind($verifier.auth.host_id).bind($workspace)
        .bind($verifier_session.to_string()).execute(&$admin_pool).await.expect("part2.rs:97");
    let $adapters = Arc::new(UnusedVerifierAdapters);
    let $service = WorkspaceService::new(
        Arc::new(PgStore::from_pool($runtime_pool.clone())),
        $adapters.clone(),
        $adapters,
    );
    let $workspace_key = format!("anti-bloat-live-{workspace}", $workspace=$workspace);
    let $verifier_context = RequestContext {
        auth: $verifier.auth.clone(),
        native_session_id: $verifier_session.to_string(),
        $workspace_key: $workspace_key.clone(),
    };
    let $owner_context = RequestContext {
        auth: $enrollment.auth.clone(),
        native_session_id: $session.to_string(),
        $workspace_key,
    };
    assert_eq!(
        $service
            .get_anti_bloat_verification_material(&$owner_context, $prepared.review_id)
            .await,
        Err(Error::Forbidden),
    );
    let ($observed, $evidence_digest) = $service
        .get_anti_bloat_verification_material(&$verifier_context, $prepared.review_id)
        .await
        .expect("part2.rs:124");
    assert!($observed.source_fragments_match);
    assert_eq!(
        $observed.input.protected_obligations_digest,
        $observed.review.protected_obligations_digest
    );
    assert_eq!(
        anti_bloat_protected_obligations_digest(
            &Sha256ScopeDigest,
            &$observed.input.protected_obligations
        )
        .expect("part2.rs:135"),
        $observed.input.protected_obligations_digest
    );
    assert_eq!($observed.after_saved, $after);
    assert_eq!($observed.$receipt, $receipt);
    assert_eq!($observed.verdict().0, AntiBloatVerificationVerdict::Pass);
    let $verify = VerifyAntiBloatApply {
        request_id: Uuid::new_v4(),
        review_id: $prepared.review_id,
        expected_evidence_digest: $evidence_digest.clone(),
    };
    assert_eq!(
        $service
            .verify_anti_bloat_apply(&$owner_context, &$verify)
            .await,
        Err(Error::Forbidden),
    );
    let mut $wrong_digest = $verify.clone();
    $wrong_digest.request_id = Uuid::new_v4();
    $wrong_digest.expected_evidence_digest = "f".repeat(64);
    assert_eq!(
        $service
            .verify_anti_bloat_apply(&$verifier_context, &$wrong_digest)
            .await,
        Err(Error::InputConflict),
    );
    let $attestation = $service
        .verify_anti_bloat_apply(&$verifier_context, &$verify)
        .await
        .expect("part2.rs:164");
    assert_eq!($attestation.verdict, AntiBloatVerificationVerdict::Pass);
    assert_eq!(
        $attestation.reason,
        AntiBloatVerificationReason::FullGraphPreserved
    );
    assert_eq!($attestation.verifier_principal_id, $verifier.principal_id);
    assert_ne!($attestation.verifier_principal_id, $actor);
    assert_eq!($attestation.$evidence_digest, $evidence_digest);
    assert_eq!(
        $service
            .verify_anti_bloat_apply(&$verifier_context, &$verify)
            .await
            .expect("part2.rs:177"),
        $attestation
    );
    let $attestation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_preservation_attestations \
         WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind($prepared.review_id)
    .fetch_one(&$admin_pool)
    .await
    .expect("part2.rs:189");
    assert_eq!($attestation_count, 1);
    let $unchanged_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    ).bind($tenant).bind($workspace).bind($candidate).fetch_one(&$admin_pool).await.expect("part2.rs:193");
    assert_eq!($unchanged_revision, 5);

    // Current Program authority must be re-observed even when the persisted
    // draft, source snapshot and historic passing attestation remain unchanged.
    sqlx::query("UPDATE programs SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind($tenant).bind($workspace).bind($program).execute(&$admin_pool).await.expect("source drift");
    let (drift, drift_digest) = $service.get_anti_bloat_verification_material(&$verifier_context, $prepared.review_id)
        .await.expect("current source readback");
    assert!(!drift.source_fragments_match);
    assert_eq!(drift.verdict().0, AntiBloatVerificationVerdict::Unknown);
    assert_ne!(drift_digest, $evidence_digest);
    let drift_request = VerifyAntiBloatApply { request_id: Uuid::new_v4(), review_id: $prepared.review_id,
        expected_evidence_digest: drift_digest };
    assert_eq!($service.verify_anti_bloat_apply(&$verifier_context, &drift_request).await
        .expect("current source Unknown attestation").verdict, AntiBloatVerificationVerdict::Unknown);
    assert_eq!($service.verify_anti_bloat_apply(&$verifier_context, &$verify).await.expect("historical immutable replay"), $attestation);
    sqlx::query("UPDATE programs SET revision=revision-1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind($tenant).bind($workspace).bind($program).execute(&$admin_pool).await.expect("restore synthetic source fixture");

    let unchanged: (i64, serde_json::Value) = sqlx::query_as(
        "SELECT s.revision,d.payload FROM scope_candidate_sets s JOIN scope_candidate_drafts d ON \
         (d.tenant_id,d.workspace_id,d.candidate_set_id,d.set_revision)=(s.tenant_id,s.workspace_id,s.id,s.revision) \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3")
        .bind($tenant).bind($workspace).bind($candidate).fetch_one(&$admin_pool).await.expect("no source-drift caller effect");
    assert_eq!(unchanged.0, 5);
    assert_eq!(serde_json::from_value::<ResolvedCandidateDraft>(unchanged.1).expect("wholegraph"), $after);
    sqlx::query("UPDATE hosts SET revoked=true WHERE tenant_id=$1 AND id=$2")
        .bind($tenant).bind($verifier.auth.host_id).execute(&$admin_pool).await.expect("synthetic revoked verifier");
    assert_eq!($service.verify_anti_bloat_apply(&$verifier_context, &$verify).await, Err(Error::Unauthorized));
    sqlx::query("UPDATE hosts SET revoked=false WHERE tenant_id=$1 AND id=$2")
        .bind($tenant).bind($verifier.auth.host_id).execute(&$admin_pool).await.expect("restore synthetic verifier");
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM scope_anti_bloat_caller_links WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3")
        .bind($tenant).bind($workspace).bind($prepared.review_id).fetch_one(&$admin_pool).await.expect("one effect fence");
    assert_eq!(links, 1);

    // A later native input advances the authoritative set. A new verifier
    // request for the old r4 -> r5 effect must then fail stale.
    let $ordinary = RecordCandidateInput {
        candidate_set_id: $candidate,
        revision: 5,
        request_id: Uuid::new_v4(),
        input: "A new planning input arrived after anti-bloat attestation".into(),
    };
    let mut $ordinary_writer = rw(&$store, &$enrollment.auth, $tenant).await;
    let $later = $ordinary_writer
        .record_candidate_input($workspace, $session, &$ordinary, $ordinary.input.len() as i64)
        .await
        .expect("part2.rs:208");
    assert_eq!($later.context.candidate_set.revision, 6);
    $ordinary_writer.commit().await.expect("part2.rs:210");
    let (stale_material, _) = $service
        .get_anti_bloat_verification_material(&$verifier_context, $prepared.review_id)
        .await
        .expect("part2.rs:214");
    assert!(!stale_material.source_fragments_match);
    assert_eq!(
        stale_material.verdict(),
        (
            AntiBloatVerificationVerdict::Unknown,
            AntiBloatVerificationReason::SourceEvidenceUnavailable
        )
    );
    let mut $stale_verify = $verify.clone();
    $stale_verify.request_id = Uuid::new_v4();
    assert_eq!(
        $service
            .verify_anti_bloat_apply(&$verifier_context, &$stale_verify)
            .await,
        Err(Error::StaleRevision),
    );
    assert_eq!(
        $service
            .verify_anti_bloat_apply(&$verifier_context, &$verify)
            .await
            .expect("part2.rs:235"),
        $attestation
    );

    let $foreign = admin::enroll_host(&$admin_pool, None, vec![]).await.expect("part2.rs:239");
    let $foreign_workspace = Uuid::new_v4();
    let $foreign_session = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind($foreign_workspace)
        .bind($foreign.tenant_id)
        .bind(format!("anti-bloat-foreign-{foreign_workspace}", $foreign_workspace=$foreign_workspace))
        .execute(&$admin_pool)
        .await
        .expect("part2.rs:248");
    let $foreign_verifier =
        admin::prepare_verifier_enrollment(&$admin_pool, $foreign.tenant_id, $foreign_workspace)
            .await
            .expect("part2.rs:252")
            .try_commit()
            .await
            .expect("part2.rs:255");
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind($foreign.tenant_id).bind($foreign_verifier.auth.host_id)
        .bind($foreign_workspace).bind($foreign_session.to_string())
        .execute(&$admin_pool).await.expect("part2.rs:259");
    let $foreign_context = RequestContext {
        auth: $foreign_verifier.auth,
        native_session_id: $foreign_session.to_string(),
        $workspace_key: format!("anti-bloat-foreign-{foreign_workspace}", $foreign_workspace=$foreign_workspace),
    };
    assert_eq!(
        $service
            .get_anti_bloat_verification_material(&$foreign_context, $prepared.review_id)
            .await,
        Err(Error::NotFound),
    );

}; }
