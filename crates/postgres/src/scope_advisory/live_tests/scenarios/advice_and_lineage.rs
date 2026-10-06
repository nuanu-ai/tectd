macro_rules! verify {
    ($actor:ident, $advice:ident, $answers:ident, $auth:ident, $candidate:ident, $dispatch:ident, $dispatch_id:ident, $enrollment:ident, $first:ident, $manifest:ident, $opportunity:ident, $other_opportunity:ident, $pool:ident, $prepared:ident, $reason:ident, $replay:ident, $request:ident, $result:ident, $second:ident, $selected_id:ident, $session:ident, $state:ident, $store:ident, $tenant:ident, $third_opportunity:ident, $unit:ident, $workspace:ident $(,)?) => {

    let stale_opportunity = Uuid::new_v4();
    let stale_request_key = format!("request-{stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(stale_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(&stale_request_key).bind(D).execute(&$pool).await.unwrap();
    let stale_prepared = ScopeManifestRecord {
        opportunity_id: stale_opportunity,
        candidate_set_id: $candidate,
        config_revision: $prepared.config_revision,
        opportunity_material_digest: $prepared.opportunity_material_digest.clone(),
        $manifest: $manifest.clone(),
    };
    let stale_disposition = ScopePreparedAdvisoryDisposition {
        opportunity_id: stale_opportunity,
        candidate_set_id: $candidate,
        expected_source_digest: $manifest.source.digest.clone(),
        $reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut stale_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    stale_unit
        .prepare_scope_advisory_manifest($workspace, &stale_prepared)
        .await
        .unwrap();
    stale_unit
        .finalize_prepared_scope_advisory_without_dispatch($workspace, &stale_disposition)
        .await
        .unwrap();
    stale_unit.commit().await.unwrap();
    let mut stale_replay = rw(&$store, &$enrollment.$auth, $tenant).await;
    stale_replay
        .finalize_prepared_scope_advisory_without_dispatch($workspace, &stale_disposition)
        .await
        .unwrap();
    stale_replay.commit().await.unwrap();
    let stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind(stale_opportunity)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(
        stale_state,
        ("no_call".into(), "deterministic_input_invalid".into())
    );
    let stale_attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind(stale_opportunity)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(stale_attempts, 0);

    sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown' WHERE id=$1")
        .bind($opportunity).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind($dispatch).bind($tenant).bind($workspace).bind($opportunity).bind(D).execute(&$pool).await.unwrap();
    let dispatched_invalidation = ScopePreparedAdvisoryDisposition {
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        expected_source_digest: $manifest.source.digest.clone(),
        $reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut dispatched_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        dispatched_unit
            .finalize_prepared_scope_advisory_without_dispatch($workspace, &dispatched_invalidation,)
            .await,
        Err(Error::InputConflict)
    );
    dispatched_unit.commit().await.unwrap();
    let $request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &$manifest).unwrap();
    let $answers = NormalizedScopeAdviceAnswers {
            comparative_disposition: None,
        $answers: vec![NormalizedScopeAdviceAnswer {
            alternative_id: $manifest.baseline_id.clone(),
            choice: ScopeAdviceChoice::Preferred,
            score: ScopeAdviceScoreBand::StrongFit,
            choice_confidence: ConfidenceBasisPoints(9000),
            score_confidence: ConfidenceBasisPoints(8000),
        }],
    };
    let $advice = guard_scope_advice(
        &Sha256ScopeDigest,
        $opportunity,
        &$manifest,
        &$request,
        &$answers,
    )
    .unwrap();
    let advice_record = GuardedScopeAdviceRecord {
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        $dispatch_id: $dispatch,
        dispatch_material_digest: D.into(),
        config_revision: 1,
        $advice: $advice.clone(),
    };
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.persist_guarded_scope_advice($workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop($unit);
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind($dispatch).execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind($opportunity).execute(&$pool).await.unwrap();
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    $unit.persist_guarded_scope_advice($workspace, &advice_record)
        .await
        .unwrap();
    $unit.commit().await.unwrap();
    set_config(&$pool, $tenant, $workspace, false).await;
    let mut $unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $unit.persist_guarded_scope_advice($workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop($unit);
    set_config(&$pool, $tenant, $workspace, true).await;

    // Two valid advice records in one workspace compete for a single request ID.
    // Both transactions start together; the winner commits before the loser checks replay.
    let $other_opportunity = Uuid::new_v4();
    let other_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind($other_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(format!("request-{other_opportunity}", $other_opportunity = $other_opportunity)).bind(D).execute(&$pool).await.unwrap();
    let mut setup = rw(&$store, &$enrollment.$auth, $tenant).await;
    setup
        .prepare_scope_advisory_manifest(
            $workspace,
            &ScopeManifestRecord {
                opportunity_id: $other_opportunity,
                ..$prepared.clone()
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(other_dispatch).bind($tenant).bind($workspace).bind($other_opportunity).bind(D)
        .execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(other_dispatch).execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind($other_opportunity).execute(&$pool).await.unwrap();
    let other_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        $other_opportunity,
        &$manifest,
        &$request,
        &$answers,
    )
    .unwrap();
    assert_eq!(
        $advice.content_digest(&Sha256ScopeDigest).unwrap(),
        other_advice.content_digest(&Sha256ScopeDigest).unwrap()
    );
    assert_ne!($advice.id, other_advice.id);
    let mut setup = rw(&$store, &$enrollment.$auth, $tenant).await;
    setup
        .persist_guarded_scope_advice(
            $workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: $other_opportunity,
                candidate_set_id: $candidate,
                $dispatch_id: other_dispatch,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                $advice: other_advice.clone(),
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let mut $replay = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $replay
            .persist_guarded_scope_advice(
                $workspace,
                &GuardedScopeAdviceRecord {
                    opportunity_id: $other_opportunity,
                    candidate_set_id: $candidate,
                    $dispatch_id: other_dispatch,
                    dispatch_material_digest: D.into(),
                    config_revision: 1,
                    $advice: other_advice.clone(),
                }
            )
            .await
            .unwrap(),
        other_advice
    );
    $replay.commit().await.unwrap();
    // A persisted v1 aggregate has no opportunity field and uses the content hash as its ID.
    let mut legacy_advice = other_advice.clone();
    legacy_advice.id = ScopeAdviceId(legacy_advice.content_digest(&Sha256ScopeDigest).unwrap());
    legacy_advice.opportunity_id = None;
    sqlx::query("UPDATE advisory_scope_advice SET advice_id=$1,aggregate_payload=$2 WHERE tenant_id=$3 AND workspace_id=$4 AND opportunity_id=$5")
        .bind(&legacy_advice.id.0)
        .bind(serde_json::to_value(&legacy_advice).unwrap())
        .bind($tenant).bind($workspace).bind($other_opportunity)
        .execute(&$pool).await.unwrap();
    let mut legacy_read = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        legacy_read
            .guarded_scope_advice($workspace, $other_opportunity)
            .await
            .unwrap(),
        Some(legacy_advice)
    );
    legacy_read.commit().await.unwrap();
    sqlx::query("UPDATE advisory_scope_advice SET advice_id=$1,aggregate_payload=$2 WHERE tenant_id=$3 AND workspace_id=$4 AND opportunity_id=$5")
        .bind(&other_advice.id.0)
        .bind(serde_json::to_value(&other_advice).unwrap())
        .bind($tenant).bind($workspace).bind($other_opportunity)
        .execute(&$pool).await.unwrap();
    let $third_opportunity = Uuid::new_v4();
    let third_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind($third_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(format!("request-{third_opportunity}", $third_opportunity = $third_opportunity)).bind(D).execute(&$pool).await.unwrap();
    let mut setup = rw(&$store, &$enrollment.$auth, $tenant).await;
    setup
        .prepare_scope_advisory_manifest(
            $workspace,
            &ScopeManifestRecord {
                opportunity_id: $third_opportunity,
                ..$prepared.clone()
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(third_dispatch).bind($tenant).bind($workspace).bind($third_opportunity).bind(D)
        .execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(third_dispatch).execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind($third_opportunity).execute(&$pool).await.unwrap();
    let mut third_answers = $answers.clone();
    third_answers.$answers[0].choice_confidence = ConfidenceBasisPoints(8800);
    let third_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        $third_opportunity,
        &$manifest,
        &$request,
        &third_answers,
    )
    .unwrap();
    assert_ne!(other_advice.id, third_advice.id);
    let mut setup = rw(&$store, &$enrollment.$auth, $tenant).await;
    setup
        .persist_guarded_scope_advice(
            $workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: $third_opportunity,
                candidate_set_id: $candidate,
                $dispatch_id: third_dispatch,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                $advice: third_advice.clone(),
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let race_request_id = Uuid::new_v4();
    let race_barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let race = |opportunity_id,
                advice_id: ScopeAdviceId,
                barrier: std::sync::Arc<tokio::sync::Barrier>| {
        let $store = $store.clone();
        let $auth = $enrollment.$auth.clone();
        let $selected_id = $manifest.baseline_id.clone();
        async move {
            let mut $unit = rw(&$store, &$auth, $tenant).await;
            barrier.wait().await;
            let $result = $unit
                .cas_scope_advisory_disposition(
                    $workspace,
                    ScopeDispositionRecord {
                        opportunity_id,
                        candidate_set_id: $candidate,
                        actor_id: $actor,
                        session_id: $session,
                        $request: ScopeDispositionRequest {
                            request_id: race_request_id,
                            advice_id,
                            expected_revision: 0,
                            action: ScopeDispositionAction::Accept,
                            $selected_id: Some($selected_id.clone()),
                            items: vec![ScopeDispositionItem {
                                alternative_id: $selected_id,
                                $state: ScopeDispositionItemState::Selected,
                            }],
                            rationale: "race".into(),
                        },
                    },
                )
                .await;
            if $result.is_ok() {
                $unit.commit().await.unwrap();
            }
            $result
        }
    };
    let ($first, $second) = tokio::join!(
        race(
            $other_opportunity,
            other_advice.id.clone(),
            race_barrier.clone()
        ),
        race($third_opportunity, third_advice.id.clone(), race_barrier),
    );
    assert!(
        matches!(
            (&$first, &$second),
            (Ok(_), Err(Error::InputConflict)) | (Err(Error::InputConflict), Ok(_))
        ),
        "same request across distinct advice IDs must have one success and one InputConflict: {first:?}, {second:?}", $first = $first, $second = $second
    );
    };
}

pub(in super::super) use verify;
