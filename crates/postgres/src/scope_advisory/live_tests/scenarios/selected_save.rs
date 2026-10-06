macro_rules! verify {
    ($actor:ident, $advice:ident, $altered:ident, $answers:ident, $audit_unit:ident, $auth:ident, $authored_scope_set:ident, $authority:ident, $authority_request:ident, $candidate:ident, $context:ident, $dispatch_id:ident, $enrollment:ident, $failed:ident, $manifest:ident, $observation:ident, $opportunity:ident, $pool:ident, $rejected:ident, $replay:ident, $request:ident, $save:ident, $second:ident, $second_auth:ident, $second_store:ident, $selected_audit:ident, $selected_id:ident, $selected_opportunity:ident, $service:ident, $session:ident, $snapshot:ident, $state:ident, $store:ident, $stored:ident, $tenant:ident, $unit:ident, $workspace:ident $(,)?) => {

    // The selected source-authored material, preservation and caller receipt
    // commit together. A changed draft rolls all of them back.
    set_config(&$pool, $tenant, $workspace, true).await;
    sqlx::query("UPDATE scope_candidate_sets SET status='draft' WHERE id=$1")
        .bind($candidate)
        .execute(&$pool)
        .await
        .unwrap();
    let $selected_opportunity = Uuid::new_v4();
    let selected_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'5',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind($selected_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(format!("request-{selected_opportunity}", $selected_opportunity = $selected_opportunity)).bind(D).execute(&$pool).await.unwrap();
    let ScopeAuthorityOutcome::Authorized(current) =
        $authority.observe(&$authority_request).await.unwrap()
    else {
        panic!("current source must be authorized");
    };
    let mut authored_for_save = $authored_scope_set.clone();
    authored_for_save.expected_candidate_set_revision = 5;
    let selected_manifest = PgScopeAuthoredManifestSupplier::new(
        $store.clone(),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            $store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
    )
    .supply_authored(&tect_application::ScopeAuthoredManifestRequest {
        tenant_id: $tenant,
        $observation: *current,
        $authored_scope_set: authored_for_save.clone(),
    })
    .await
    .unwrap();
    let selected_record = ScopeManifestRecord {
        opportunity_id: $selected_opportunity,
        candidate_set_id: $candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        $manifest: selected_manifest.clone(),
    };
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    $unit.prepare_authored_scope_advisory_manifest($workspace, &selected_record, &"f".repeat(64))
        .await
        .unwrap();
    $unit.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','y','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(selected_dispatch).bind($tenant).bind($workspace).bind($selected_opportunity).bind(D)
        .execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind($selected_opportunity).execute(&$pool).await.unwrap();
    let selected_request =
        ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &selected_manifest).unwrap();
    let selected_answers = NormalizedScopeAdviceAnswers {
            comparative_disposition: None,
        $answers: vec![NormalizedScopeAdviceAnswer {
            alternative_id: selected_manifest.baseline_id.clone(),
            choice: ScopeAdviceChoice::Preferred,
            score: ScopeAdviceScoreBand::StrongFit,
            choice_confidence: ConfidenceBasisPoints(9000),
            score_confidence: ConfidenceBasisPoints(8000),
        }],
    };
    let selected_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        $selected_opportunity,
        &selected_manifest,
        &selected_request,
        &selected_answers,
    )
    .unwrap();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    $unit.persist_guarded_scope_advice(
        $workspace,
        &GuardedScopeAdviceRecord {
            opportunity_id: $selected_opportunity,
            candidate_set_id: $candidate,
            $dispatch_id: selected_dispatch,
            dispatch_material_digest: D.into(),
            config_revision: 1,
            $advice: selected_advice.clone(),
        },
    )
    .await
    .unwrap();
    $unit.commit().await.unwrap();
    let selected_disposition = $service
        .decide_scope_advisory(
            &$context,
            $selected_opportunity,
            $candidate,
            ScopeDispositionRequest {
                request_id: Uuid::new_v4(),
                advice_id: selected_advice.id.clone(),
                expected_revision: 0,
                action: ScopeDispositionAction::Accept,
                $selected_id: Some(selected_manifest.baseline_id.clone()),
                items: vec![ScopeDispositionItem {
                    alternative_id: selected_manifest.baseline_id.clone(),
                    $state: ScopeDispositionItemState::Selected,
                }],
                rationale: "Use selected cohesive alternative".into(),
            },
        )
        .await
        .unwrap();
    let mut $save = SaveCandidateDraft {
        candidate_set_id: $candidate,
        revision: 5,
        snapshot_id: $snapshot,
        input_cursor: 2,
        request_id: Uuid::new_v4(),
        draft: authored_for_save.alternatives[0].draft.clone(),
        consumed_knowledge: None,
        selected_advisory: Some(SelectedScopeAdvisory {
            opportunity_id: $selected_opportunity,
            disposition_id: selected_disposition.id,
            $selected_id: selected_manifest.baseline_id.clone(),
            alternative_key: "baseline".into(),
        }),
    };
    let mut superseding = rw(&$store, &$enrollment.$auth, $tenant).await;
    let $rejected = superseding
        .cas_scope_advisory_disposition(
            $workspace,
            ScopeDispositionRecord {
                opportunity_id: $selected_opportunity,
                candidate_set_id: $candidate,
                actor_id: $actor,
                session_id: $session,
                $request: ScopeDispositionRequest {
                    request_id: Uuid::new_v4(),
                    advice_id: selected_advice.id.clone(),
                    expected_revision: 1,
                    action: ScopeDispositionAction::RejectAll,
                    $selected_id: None,
                    items: vec![ScopeDispositionItem {
                        alternative_id: selected_manifest.baseline_id.clone(),
                        $state: ScopeDispositionItemState::NotSelected,
                    }],
                    rationale: "Supersede the selected decision".into(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!($rejected.revision, 2);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let pending_store = $store.clone();
    let pending_auth = $enrollment.$auth.clone();
    let pending_save = $save.clone();
    let mut pending = tokio::spawn(async move {
        let mut $unit = rw(&pending_store, &pending_auth, $tenant).await;
        started_tx.send(()).unwrap();
        $unit.save_selected_candidate_draft($workspace, $actor, $session, &pending_save)
            .await
    });
    started_rx.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
            .await
            .is_err()
    );
    superseding.commit().await.unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::StaleRevision),
    );
    let rejected_effects: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind($candidate).bind($save.request_id).fetch_one(&$pool).await.unwrap();
    assert_eq!(rejected_effects, (0, 0));
    let accepted_again = $service
        .decide_scope_advisory(
            &$context,
            $selected_opportunity,
            $candidate,
            ScopeDispositionRequest {
                request_id: Uuid::new_v4(),
                advice_id: selected_advice.id.clone(),
                expected_revision: 2,
                action: ScopeDispositionAction::Accept,
                $selected_id: Some(selected_manifest.baseline_id.clone()),
                items: vec![ScopeDispositionItem {
                    alternative_id: selected_manifest.baseline_id.clone(),
                    $state: ScopeDispositionItemState::Selected,
                }],
                rationale: "Select the frozen alternative after review".into(),
            },
        )
        .await
        .unwrap();
    $save.selected_advisory.as_mut().unwrap().disposition_id = accepted_again.id;
    let mut bypass = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        bypass.save_candidate_draft($workspace, &$save).await,
        Err(Error::InvalidArguments),
    );
    drop(bypass);
    let bypass_effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind($candidate).bind($save.request_id).fetch_one(&$pool).await.unwrap();
    assert_eq!(bypass_effects, (0, 0, 0));
    // Public boxed-UOW rejection: committed-effects proof only, not staged SQL counts.
    for selected in [false, true] {
        let mut forged = $save.clone();
        forged.request_id = Uuid::new_v4();
        if !selected {
            forged.selected_advisory = None;
        }
        let mut optional = forged.draft.candidates[0].clone();
        optional.identity = DraftIdentity {
            local: Some("forged_optional".into()), id: None, revision: None,
        };
        optional.grounding = CandidateGrounding::ExploratoryUnrequested {
            provenance: ExploratoryProvenance::SourceAuthoredV2,
        };
        optional.coverage_goals.clear();
        forged.draft.candidates.push(optional);
        forged.draft.validate().unwrap();
        let before: i64 = sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3"
        ).bind($tenant).bind($workspace).bind($candidate).fetch_one(&$pool).await.unwrap();
        let mut tx = rw(&$store, &$enrollment.$auth, $tenant).await;
        let result = if selected {
            tx.save_selected_candidate_draft($workspace, $actor, $session, &forged).await
        } else {
            tx.save_candidate_draft($workspace, &forged).await
        };
        drop(tx);
        assert_eq!(result, Err(Error::InvalidArguments));
        let effects: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                    (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                    (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2),\
                    (SELECT revision FROM scope_candidate_sets WHERE tenant_id=$3 AND workspace_id=$4 AND id=$1)"
        ).bind($candidate).bind(forged.request_id).bind($tenant).bind($workspace)
            .fetch_one(&$pool).await.unwrap();
        assert_eq!(effects, (0, 0, 0, before));
    }
    // Private-writer staged-effects proof; public boxed-UOW cases above prove
    // committed effects only because UnitOfWork exposes no transaction inspector.
    for mode in ["ordinary", "selected", "selected_material"] {
        let mut request = $save.clone();
        request.request_id = Uuid::new_v4();
        let mut material = selected_manifest
            .eligible(&request.selected_advisory.as_ref().unwrap().$selected_id)
            .unwrap().material.clone();
        request.draft.validate().unwrap();
        request.draft.require_source_grounded().unwrap();
        material.validate().unwrap();
        material.require_source_grounded().unwrap();
        if mode == "selected_material" {
            // Keep the request grounded and both selected fields present.
            // Only the supplied resolved material carries exploratory provenance.
            let mut optional = material.candidates[0].clone();
            optional.id = Uuid::new_v4();
            optional.coverage_goal_ids.clear();
            optional.grounding = CandidateGrounding::ExploratoryUnrequested {
                provenance: ExploratoryProvenance::SourceAuthoredV2,
            };
            material.delta.added.push(CandidateAdded {
                candidate_id: optional.id, revision: optional.revision,
            });
            material.candidates.push(optional);
            material.validate().unwrap();
            assert_eq!(material.require_source_grounded(), Err(Error::InvalidArguments));
            request.draft.require_source_grounded().unwrap();
            assert!(request.selected_advisory.is_some());
        } else {
            let mut optional = request.draft.candidates[0].clone();
            optional.identity = DraftIdentity {
                local: Some("private_forged_optional".into()), id: None, revision: None,
            };
            optional.coverage_goals.clear();
            optional.grounding = CandidateGrounding::ExploratoryUnrequested {
                provenance: ExploratoryProvenance::SourceAuthoredV2,
            };
            request.draft.candidates.push(optional);
            if mode == "ordinary" { request.selected_advisory = None; }
            request.draft.validate().unwrap();
            assert_eq!(request.draft.require_source_grounded(), Err(Error::InvalidArguments));
        }
        let effects_sql =
            "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3),\
                    (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3),\
                    (SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3),\
                    (SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)";
        let mut tx = $pool.begin().await.unwrap();
        let before: (i64, i64, i64, i64) = sqlx::query_as(effects_sql)
            .bind($tenant).bind($workspace).bind($candidate)
            .fetch_one(&mut *tx).await.unwrap();
        let result = match mode {
            "ordinary" => crate::scope_candidates::save_draft(
                &mut tx, $tenant, $workspace, &request).await,
            "selected" => crate::scope_advisory::save_selected_candidate_draft(
                &mut tx, $tenant, $workspace, $actor, $session, &request).await,
            _ => crate::scope_candidates::save_draft_with_material(
                &mut tx, $tenant, $workspace, &request, Some(&material)).await,
        };
        // Read before rollback: rollback cannot hide transient staged writes.
        let staged: (i64, i64, i64, i64) = sqlx::query_as(effects_sql)
            .bind($tenant).bind($workspace).bind($candidate)
            .fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        let committed: (i64, i64, i64, i64) = sqlx::query_as(effects_sql)
            .bind($tenant).bind($workspace).bind($candidate)
            .fetch_one(&$pool).await.unwrap();
        assert_eq!(result, Err(Error::InvalidArguments));
        assert_eq!(staged, before, "{mode}: staged effects before rollback");
        assert_eq!(committed, before, "{mode}: committed effects after rollback");
    }
    let mut $altered = $save.clone();
    $altered.request_id = Uuid::new_v4();
    $altered.draft.candidates[0].title = "Altered".into();
    let mut $failed = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $failed
            .save_selected_candidate_draft($workspace, $actor, $session, &$altered)
            .await,
        Err(Error::InputConflict)
    );
    drop($failed);
    let rolled_back: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind($candidate).bind($altered.request_id).fetch_one(&$pool).await.unwrap();
    assert_eq!(rolled_back, (0, 0, 0));
    let mut wrong_id = $save.clone();
    wrong_id.request_id = Uuid::new_v4();
    wrong_id.selected_advisory.as_mut().unwrap().$selected_id = ScopeAlternativeId(D.into());
    let mut $failed = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $failed
            .save_selected_candidate_draft($workspace, $actor, $session, &wrong_id)
            .await,
        Err(Error::InputConflict)
    );
    drop($failed);
    let mut successful = rw(&$store, &$enrollment.$auth, $tenant).await;
    let $stored = successful
        .save_selected_candidate_draft($workspace, $actor, $session, &$save)
        .await
        .unwrap();
    assert_eq!(
        $stored.draft.as_ref(),
        Some(&selected_manifest.emitted[0].material)
    );
    let (second_started_tx, second_started_rx) = tokio::sync::oneshot::channel();
    let $second_store = $store.clone();
    let $second_auth = $enrollment.$auth.clone();
    let second_save = $save.clone();
    let mut $second = tokio::spawn(async move {
        let mut $unit = rw(&$second_store, &$second_auth, $tenant).await;
        second_started_tx.send(()).unwrap();
        let $replay = $unit
            .save_selected_candidate_draft($workspace, $actor, $session, &second_save)
            .await?;
        $unit.commit().await?;
        Ok::<_, Error>($replay)
    });
    second_started_rx.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut $second)
            .await
            .is_err()
    );
    successful.commit().await.unwrap();
    let concurrent_replay = tokio::time::timeout(std::time::Duration::from_secs(5), $second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(concurrent_replay.draft, $stored.draft);
    assert_eq!(concurrent_replay.$context.candidate_set.revision, 6);
    let committed: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_preservation_receipt WHERE candidate_set_id=$1 AND request_id=$2 AND status='passed'),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2 AND caller_result_revision=6)"
    ).bind($candidate).bind($save.request_id).fetch_one(&$pool).await.unwrap();
    assert_eq!(committed, (1, 1, 1, 1));
    let mut $audit_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let $selected_audit = $audit_unit
        .candidate_advisory_opportunity_detail($workspace, $candidate, $selected_opportunity)
        .await
        .unwrap()
        .$opportunity;
    assert_eq!($selected_audit.disposition_id, Some(accepted_again.id));
    assert_eq!(
        $selected_audit.preservation_status.as_deref(),
        Some("passed")
    );
    assert!($selected_audit.preservation_receipt_id.is_some());
    assert_eq!($selected_audit.caller_receipt_id, Some($save.request_id));
    assert!($selected_audit.caller_link_id.is_some());
    assert_eq!($selected_audit.verifier_receipt_id, None);
    assert_eq!($selected_audit.selected_save_observation, None);
    drop($audit_unit);
    };
}

pub(in super::super) use verify;
