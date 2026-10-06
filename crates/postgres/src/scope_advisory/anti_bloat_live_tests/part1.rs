macro_rules! anti_core_part_1 { ($active:ident, $actor:ident, $adapters:ident, $admin_pool:ident, $advice:ident, $advice_request:ident, $advice_writer:ident, $after:ident, $app:ident, $app_reader:ident, $apply:ident, $apply_store:ident, $attestation:ident, $attestation_count:ident, $authored:ident, $authored_delta:ident, $before:ident, $before_app:ident, $binding:ident, $bytes:ident, $caller_link:ident, $candidate:ident, $ceilings:ident, $decider:ident, $dispatch_id:ident, $disposition:ident, $draft:ident, $eligible:ident, $enrollment:ident, $evidence_digest:ident, $exploratory:ident, $finding:ident, $foreign:ident, $foreign_context:ident, $foreign_session:ident, $foreign_verifier:ident, $foreign_workspace:ident, $from:ident, $id:ident, $install:ident, $later:ident, $lineage:ident, $link:ident, $now:ident, $observed:ident, $opportunity:ident, $ordinary:ident, $ordinary_writer:ident, $other_workspace:ident, $owner_context:ident, $persisted:ident, $policies:ident, $policy:ident, $prepared:ident, $program:ident, $program_body:ident, $reader:ident, $receipt:ident, $record:ident, $replay:ident, $replay_store:ident, $request:ident, $reservation_count:ident, $resolved:ident, $resolver:ident, $review_count:ident, $runtime_pool:ident, $runtime_url:ident, $save:ident, $saver:ident, $seed:ident, $selected:ident, $selected_app:ident, $selected_reader:ident, $service:ident, $session:ident, $shadow_count:ident, $snapshot:ident, $source_digest:ident, $source_ref:ident, $stale:ident, $stale_review:ident, $stale_verify:ident, $store:ident, $stored:ident, $tenant:ident, $unchanged_revision:ident, $until:ident, $verifier:ident, $verifier_context:ident, $verifier_session:ident, $verify:ident, $workspace:ident, $workspace_key:ident, $writer:ident, $wrong:ident, $wrong_digest:ident) => {
    let $dispatch_id = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','y','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind($dispatch_id).bind($tenant).bind($workspace).bind($opportunity).bind(D)
        .execute(&$admin_pool).await.expect("part1.rs:5");
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind($opportunity).execute(&$admin_pool).await.expect("part1.rs:7");
    let $advice_request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &$authored).expect("part1.rs:8");
    let $advice = guard_scope_advice(
        &Sha256ScopeDigest,
        $opportunity,
        &$authored,
        &$advice_request,
        &NormalizedScopeAdviceAnswers {
            answers: vec![NormalizedScopeAdviceAnswer {
                alternative_id: $authored.baseline_id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score: ScopeAdviceScoreBand::StrongFit,
                choice_confidence: ConfidenceBasisPoints(9000),
                score_confidence: ConfidenceBasisPoints(8000),
            }],
            comparative_disposition: None,
        },
    )
    .expect("part1.rs:25");
    let mut $advice_writer = rw(&$store, &$enrollment.auth, $tenant).await;
    $advice_writer
        .persist_guarded_scope_advice(
            $workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                $dispatch_id,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                $advice: $advice.clone(),
            },
        )
        .await
        .expect("part1.rs:40");
    $advice_writer.commit().await.expect("part1.rs:41");
    let mut $decider = rw(&$store, &$enrollment.auth, $tenant).await;
    let $disposition = $decider
        .cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: ScopeDispositionRequest {
                    request_id: Uuid::new_v4(),
                    advice_id: $advice.$id.clone(),
                    expected_revision: 0,
                    action: ScopeDispositionAction::Accept,
                    selected_id: Some($authored.baseline_id.clone()),
                    items: vec![ScopeDispositionItem {
                        alternative_id: $authored.baseline_id.clone(),
                        state: ScopeDispositionItemState::Selected,
                    }],
                    rationale: "Select the source-grounded result".into(),
                },
            },
        )
        .await
        .expect("part1.rs:66");
    $decider.commit().await.expect("part1.rs:67");
    let $save = SaveCandidateDraft {
        candidate_set_id: $candidate,
        revision: 3,
        snapshot_id: $snapshot,
        input_cursor: 2,
        request_id: Uuid::new_v4(),
        $draft,
        consumed_knowledge: None,
        selected_advisory: Some(SelectedScopeAdvisory {
            opportunity_id: $opportunity,
            disposition_id: $disposition.$id,
            selected_id: $authored.baseline_id.clone(),
            alternative_key: "baseline".into(),
        }),
    };
    let mut $saver = rw(&$store, &$enrollment.auth, $tenant).await;
    let $stored = $saver
        .save_selected_candidate_draft($workspace, $actor, $session, &$save)
        .await
        .expect("part1.rs:87");
    assert_eq!($stored.context.candidate_set.revision, 4);
    assert_eq!($stored.$draft, Some($resolved.clone()));
    $saver.commit().await.expect("part1.rs:90");

    let $lineage: (i64, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT b.selected_draft_revision,b.selected_material_digest,b.selected_caller_link_id, \
                b.selected_caller_request_id FROM scope_anti_bloat_bindings b \
         WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
           AND b.candidate_set_revision=4",
    )
    .bind($tenant)
    .bind($workspace)
    .bind($candidate)
    .fetch_one(&$admin_pool)
    .await
    .expect("part1.rs:103");
    assert_eq!($lineage.0, 4);
    assert_eq!($lineage.1, $authored.emitted[0].material_digest);
    assert_eq!($lineage.3, $save.request_id);
    let $caller_link: Uuid = sqlx::query_scalar(
        "SELECT link_id FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND request_id=$4",
    )
    .bind($tenant)
    .bind($workspace)
    .bind($candidate)
    .bind($save.request_id)
    .fetch_one(&$admin_pool)
    .await
    .expect("part1.rs:117");
    assert_eq!($lineage.2, $caller_link);

    let mut $selected_reader = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    $selected_reader
        .authenticate(&$enrollment.auth)
        .await
        .expect("part1.rs:124");
    let $selected =
        AntiBloatStore::authoritative_input(&mut $selected_reader, $workspace, $candidate, 4)
            .await
            .expect("part1.rs:128")
            .expect("part1.rs:129");
    assert_eq!($selected.selected_revision, 4);
    assert_eq!($selected.selected_id, $authored.baseline_id);
    assert_eq!($selected.manifest, $authored);
    for ($other_workspace, other_revision) in [($workspace, 3), ($workspace, 5), ($other_workspace, 4)]
    {
        assert!(
            AntiBloatStore::authoritative_input(
                &mut $selected_reader,
                $other_workspace,
                $candidate,
                other_revision
            )
            .await
            .expect("part1.rs:143")
            .is_none()
        );
    }
    let mut $selected_app = AntiBloatApplication {
        $store: $selected_reader,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        $selected_app
            .prepare(
                $workspace,
                $actor,
                $candidate,
                3,
                AdvisoryRequestPreference::UseWorkspace
            )
            .await,
        Err(Error::NotFound)
    );
    assert_eq!(
        $selected_app
            .prepare(
                $other_workspace,
                $actor,
                $candidate,
                4,
                AdvisoryRequestPreference::UseWorkspace
            )
            .await,
        Err(Error::NotFound)
    );
    let $prepared = $selected_app
        .prepare(
            $workspace,
            $actor,
            $candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .expect("part1.rs:184");
    assert_eq!(
        $prepared.state,
        tect_application::AntiBloatAttemptState::Prepared
    );
    let $stale_review = $selected_app
        .prepare(
            $workspace,
            $actor,
            $candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .expect("part1.rs:198");
    let $exploratory = $resolved
        .candidates
        .iter()
        .find(|value| !value.grounding.is_source_grounded())
        .expect("part1.rs:203");
    let $finding = $prepared
        .review
        .findings
        .iter()
        .find(|value| value.candidate_id == $exploratory.$id)
        .expect("part1.rs:209");
    assert!($finding.rankable);
    let $authored_delta = AntiBloatAuthoredDelta {
        review_id: $prepared.review_id,
        finding_id: $finding.$id.clone(),
        $disposition: AntiBloatDisposition::Narrow,
        delta: CandidateDeltaBatch {
            candidate_set_id: $candidate,
            expected_revision: 4,
            idempotency_key: format!("live-narrow-{}", $prepared.review_id),
            operations: vec![CandidateDeltaOperation::CandidateRemove {
                candidate_id: $exploratory.$id,
                expected_revision: $exploratory.revision,
            }],
        },
    };
    Box::new($selected_app.$store).commit().await.expect("part1.rs:225");
    let $now = i64::try_from(
        std::time::SystemTime::$now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("part1.rs:229")
            .as_millis(),
    )
    .expect("part1.rs:232");
    let $ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1,
    };
    let mut $policies = Vec::new();
    for version in [1, 2] {
        let $id = Uuid::new_v4();
        let $from = $now - 60_000;
        let $until = $now + 600_000;
        let $policy = AdvisoryBudgetPolicy::new(
            $id,
            version,
            AdvisoryBudgetPolicy::digest_for($id, version, $from, $until, $ceilings),
            $from,
            $until,
            $ceilings,
            $actor,
            "a".repeat(128),
        )
        .expect("part1.rs:256");
        let mut $install = rw(&$store, &$enrollment.auth, $tenant).await;
        $install
            .advisory_budget_policy_store()
            .expect("part1.rs:260")
            .install_budget_policy($workspace, &$policy)
            .await
            .expect("part1.rs:263");
        $install.commit().await.expect("part1.rs:264");
        $policies.push($policy);
    }
    let $eligible = $prepared
        .review
        .findings
        .iter()
        .filter(|$finding| $finding.rankable)
        .map(|$finding| $finding.$id.clone())
        .collect::<Vec<_>>();
    let $bytes = serde_json::to_vec(&serde_json::json!({
        "review": &$prepared.review, "eligible_ids": &$eligible,
    }))
    .expect("part1.rs:277");
    let $request = AntiBloatPreparedRequest {
        sha256: format!("{:x}", Sha256::digest(&$bytes)),
        $bytes,
        material_sha256: tect_application::anti_bloat_material_sha256(&$prepared).expect("part1.rs:281"),
        adapter_identity: "generic-json-v1".into(),
    };
    let mut $stale = rw(&$store, &$enrollment.auth, $tenant).await;
    assert_eq!(
        $stale
            .anti_bloat_store()
            .expect("part1.rs:288")
            .begin_send(&$prepared, &$request, &$policies[0], None)
            .await,
        Err(Error::BudgetPolicyInvalid)
    );
    $stale.commit().await.expect("part1.rs:293");
    let $reservation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3"
    ).bind($tenant).bind($workspace).bind($prepared.review_id)
        .fetch_one(&$admin_pool).await.expect("part1.rs:297");
    assert_eq!($reservation_count, 0);
    let mut $active = rw(&$store, &$enrollment.auth, $tenant).await;
    assert!(
        $active
            .anti_bloat_store()
            .expect("part1.rs:303")
            .begin_send(&$prepared, &$request, &$policies[1], None)
            .await
            .expect("part1.rs:306")
            .is_some()
    );
    drop($active); // Roll back the accepted reservation so the existing apply proof can continue.

}; }
