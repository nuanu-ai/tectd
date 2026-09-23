use super::live_support::{D, manifest, rw, set_config};
use super::*;
use crate::{PgStore, admin};

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn seven_aggregate_vertical_rejects_wrong_candidate_unresolved_partial_lineage_and_identity()
{
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let actor = enrollment.principal_id;
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let opportunity = Uuid::new_v4();
    let dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("scope-live-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    for id in [session, verifier_session] {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind(tenant).bind(enrollment.auth.host_id).bind(workspace).bind(id.to_string())
            .execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'body')")
        .bind(tenant).bind(workspace).bind(D).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(D).execute(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(snapshot)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("request-{opportunity}")).bind(D).execute(&pool).await.unwrap();
    let preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(preselection, (true, 0));

    let manifest = manifest(candidate, snapshot, program);
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    let wrong = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: Uuid::new_v4(),
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &wrong)
            .await,
        Err(Error::InputConflict)
    );
    let mut rejected_baseline = manifest.clone();
    let mut rejected = rejected_baseline.emitted[0].clone();
    rejected.material.candidates[0].title = "Rejected".into();
    rejected.material_digest =
        scope_candidate_material_digest(&Sha256ScopeDigest, &rejected.material).unwrap();
    rejected.id = stable_scope_alternative_id(
        &Sha256ScopeDigest,
        &rejected_baseline.constructor,
        &rejected_baseline.source.digest,
        rejected.kind,
        &rejected.material_digest,
        &rejected.coverage,
    )
    .unwrap();
    rejected_baseline.rejected = vec![RejectedScopeAlternative {
        alternative: rejected.clone(),
        reason_codes: vec!["not_eligible".into()],
    }];
    rejected_baseline.baseline_id = rejected.id.clone();
    rejected_baseline.ordered_ids.push(rejected.id);
    rejected_baseline.ordered_ids.sort();
    rejected_baseline.whole_set_digest = rejected_baseline
        .canonical_whole_set_digest(&Sha256ScopeDigest)
        .unwrap();
    let rejected_record = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: rejected_baseline,
    };
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &rejected_record)
            .await,
        Err(Error::InvalidArguments)
    );
    let prepared = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    unit.prepare_scope_advisory_manifest(workspace, &prepared)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(dispatch).bind(tenant).bind(workspace).bind(opportunity).bind(D).execute(&pool).await.unwrap();
    let request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let answers = NormalizedScopeAdviceAnswers {
        answers: vec![NormalizedScopeAdviceAnswer {
            alternative_id: manifest.baseline_id.clone(),
            choice: ScopeAdviceChoice::Preferred,
            score: ScopeAdviceScoreBand::StrongFit,
            choice_confidence: ConfidenceBasisPoints(9000),
            score_confidence: ConfidenceBasisPoints(8000),
        }],
    };
    let advice = guard_scope_advice(&Sha256ScopeDigest, &manifest, &request, &answers).unwrap();
    let advice_record = GuardedScopeAdviceRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        dispatch_id: dispatch,
        dispatch_material_digest: D.into(),
        config_revision: 1,
        advice: advice.clone(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_guarded_scope_advice(workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(dispatch).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.persist_guarded_scope_advice(workspace, &advice_record)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_guarded_scope_advice(workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;

    let item = ScopeDispositionItem {
        alternative_id: manifest.baseline_id.clone(),
        state: ScopeDispositionItemState::Selected,
    };
    let mut partial = ScopeDispositionRequest {
        request_id: Uuid::new_v4(),
        advice_id: advice.id.clone(),
        expected_revision: 0,
        action: ScopeDispositionAction::Accept,
        selected_id: Some(manifest.baseline_id.clone()),
        items: vec![],
        rationale: "accept".into(),
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: partial.clone(),
            }
        )
        .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: partial.clone()
            }
        )
        .await,
        Err(Error::InvalidArguments)
    );
    partial.items = vec![item];
    let disposition = unit
        .cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: partial.clone(),
            },
        )
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let mut changed_lineage = partial.clone();
    changed_lineage.advice_id = ScopeAdviceId(D.into());
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: changed_lineage
            }
        )
        .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let mut competing = partial.clone();
    competing.request_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: competing
            }
        )
        .await,
        Err(Error::StaleRevision)
    );
    drop(unit);

    let observation = FreshScopeObservation {
        source: manifest.source.clone(),
        manifest: manifest.clone(),
        candidate_set_revision: 3,
        advice_id: advice.id.clone(),
    };
    let preservation = evaluate_scope_preservation(
        &Sha256ScopeDigest,
        &manifest,
        &advice,
        &disposition,
        &observation,
    )
    .unwrap();
    let preservation_id = Uuid::new_v4();
    let preservation_input = ScopePreservationReceiptInput {
        receipt_id: preservation_id,
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        disposition_id: disposition.id,
        observation: observation.clone(),
        result: preservation.clone(),
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_preservation_receipt(workspace, &preservation_input)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.persist_scope_preservation_receipt(workspace, &preservation_input)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let mut changed_preservation = preservation_input.clone();
    changed_preservation.disposition_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_preservation_receipt(workspace, &changed_preservation)
            .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let caller_request = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_receipts(tenant_id,workspace_id,candidate_set_id,operation,request_id,request_payload,result_revision,result_payload) VALUES($1,$2,$3,'save_review',$4,'{}',3,'{}')")
        .bind(tenant).bind(workspace).bind(candidate).bind(caller_request).execute(&pool).await.unwrap();
    let caller = ScopeCallerLinkInput {
        link_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        disposition_id: disposition.id,
        preservation_receipt_id: preservation_id,
        caller_operation: "save_review".into(),
        caller_request_id: caller_request,
        caller_result_revision: 3,
        actor_id: actor,
        session_id: session,
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &caller).await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let failed_preservation_id = Uuid::new_v4();
    let mut failed_result = preservation.clone();
    failed_result.status = ScopePreservationStatus::Failed;
    failed_result.reason_codes = vec!["source_changed".into()];
    sqlx::query("INSERT INTO advisory_scope_preservation_receipt(tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,advice_id,disposition_id,disposition_revision,source_digest,manifest_digest,eligible_set_digest,observed_candidate_set_revision,status,aggregate_schema,observation_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,3,'failed','tect.scope-preservation/1',$13,$14)")
        .bind(tenant).bind(workspace).bind(opportunity).bind(candidate).bind(failed_preservation_id)
        .bind(Uuid::new_v4()).bind(&advice.id.0).bind(disposition.id).bind(disposition.revision)
        .bind(&manifest.source.digest).bind(&manifest.whole_set_digest).bind(&manifest.eligible_set_digest)
        .bind(serde_json::to_value(&observation).unwrap()).bind(serde_json::to_value(&failed_result).unwrap())
        .execute(&pool).await.unwrap();
    let mut failed_caller = caller.clone();
    failed_caller.link_id = Uuid::new_v4();
    failed_caller.request_id = Uuid::new_v4();
    failed_caller.preservation_receipt_id = failed_preservation_id;
    let mut wrong_lineage = caller.clone();
    wrong_lineage.candidate_set_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &failed_caller)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &wrong_lineage)
            .await,
        Err(Error::InputConflict)
    );
    unit.link_scope_advisory_caller(workspace, &caller)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let mut changed_caller = caller.clone();
    changed_caller.preservation_receipt_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &changed_caller)
            .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let mut verifier = ScopeVerifierReceiptInput {
        receipt_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        caller_link_id: caller.link_id,
        actor_id: actor,
        session_id: session,
        verified_revision: 3,
        verifier_digest: D.into(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_verifier_receipt(workspace, &verifier)
            .await,
        Err(Error::InputConflict)
    );
    verifier.session_id = verifier_session;
    unit.persist_scope_verifier_receipt(workspace, &verifier)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let completed_preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(completed_preselection, (true, 0));

    let original = serde_json::to_value(&manifest).unwrap();
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=jsonb_set(aggregate_payload,'{baseline_id}','\"tampered\"') WHERE opportunity_id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert!(
        unit.scope_advisory_manifest(workspace, opportunity)
            .await
            .is_err()
    );
    drop(unit);
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=$1 WHERE opportunity_id=$2")
        .bind(original)
        .bind(opportunity)
        .execute(&pool)
        .await
        .unwrap();
}
