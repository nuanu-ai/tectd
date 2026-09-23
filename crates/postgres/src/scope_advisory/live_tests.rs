use super::live_support::{D, manifest, reseal_manifest, rw, set_config};
use super::*;
use crate::{PgStore, admin};
use tect_application::{SetupFiles, SourceInspector, WorkspaceService};

struct UnusedHostAdapters;

#[async_trait::async_trait]
impl SourceInspector for UnusedHostAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<tect_domain::SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl SetupFiles for UnusedHostAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<tect_domain::SetupDirectory> {
        Err(Error::InternalInvariant)
    }

    fn inspect(
        &self,
        _: &tect_domain::SetupDirectory,
        _: usize,
    ) -> Result<tect_domain::FileObservation> {
        Err(Error::InternalInvariant)
    }

    fn publish(
        &self,
        _: &tect_domain::SetupDirectory,
        _: &str,
    ) -> Result<tect_domain::FilePublication> {
        Err(Error::InternalInvariant)
    }
}

struct FixtureCandidateGuidance;

impl tect_application::CandidateGuidance for FixtureCandidateGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: D.into(),
            method: CandidateMethodSnapshot {
                id: "m".into(),
                revision: "4".into(),
                digest: D.into(),
                body: "body".into(),
                origin_refs: vec![],
            },
            registry_revision: "3".into(),
            registry_digest: D.into(),
            rules: vec![],
        })
    }
}

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
    let mut source_refs = [Uuid::new_v4(), Uuid::new_v4()];
    source_refs.sort();
    let opportunity = Uuid::new_v4();
    let dispatch = Uuid::new_v4();
    let request_key = format!("request-{opportunity}");
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("scope-live-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
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
    let frozen_program_body = serde_json::json!({
        "id": program,
        "workspace_id": workspace,
        "status": "open",
        "revision": 4,
        "name": "p",
        "intent": "i",
        "basis": "b",
        "boundaries": "finite",
        "constraints": "c",
        "success": "s",
        "working_notes": null,
        "pending_question": null,
        "current_step": "ready",
        "input_cursor": 2,
        "latest_input": 2
    })
    .to_string();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4)")
        .bind(tenant).bind(workspace).bind(D).bind(frozen_program_body).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(D).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_field','intent',$6,'intent')")
        .bind(source_refs[0]).bind(tenant).bind(workspace).bind(candidate).bind(snapshot)
        .bind(D).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_success',$6,'success')")
        .bind(source_refs[1]).bind(tenant).bind(workspace).bind(candidate).bind(snapshot)
        .bind(D).execute(&pool).await.unwrap();
    let blank_digest = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'   ')")
        .bind(tenant).bind(workspace).bind(blank_digest).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','name',$5,'name')")
        .bind(tenant).bind(workspace).bind(candidate).bind(snapshot).bind(blank_digest)
        .execute(&pool).await.unwrap();
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
        .bind(&request_key).bind(D).execute(&pool).await.unwrap();
    let preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(preselection, (true, 0));

    let manifest = manifest(
        candidate,
        snapshot,
        program,
        &[(source_refs[0], D), (source_refs[1], D)],
    );
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let authority =
        PgScopeAuthorityObserver::new(store.clone(), std::sync::Arc::new(FixtureCandidateGuidance));
    let authority_request = ScopeAuthorityRequest {
        tenant_id: tenant,
        workspace_id: workspace,
        actor_id: actor,
        session_id: session,
        candidate_set_id: candidate,
    };
    let observed = authority.observe(&authority_request).await.unwrap();
    let ScopeAuthorityOutcome::Authorized(observed) = observed else {
        panic!("persisted source must be authorized");
    };
    assert_eq!(observed.source, manifest.source);
    assert_eq!(observed.obligations, manifest.obligations);
    let service_authority = std::sync::Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        std::sync::Arc::new(FixtureCandidateGuidance),
    ));
    let service_supplier = std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        service_authority.clone(),
    ));
    let authored_scope_set = tect_application::AuthoredScopeSet {
        expected_candidate_set_revision: 3,
        baseline_key: "baseline".into(),
        alternatives: vec![tect_application::AuthoredScopeAlternative {
            key: "baseline".into(),
            kind: tect_domain::ScopeDecompositionKind::Cohesive,
            draft: serde_json::from_value(serde_json::json!({
                "boundary": "finite",
                "goals": [{
                    "identity": {"local": "goal"},
                    "text": "Preserve source",
                    "source_ref_id": source_refs[1],
                    "resolution": {"kind": "candidate", "reference": {"local": "candidate"}}
                }],
                "candidates": [{
                    "identity": {"local": "candidate"},
                    "title": "Cohesive",
                    "outcome": "Exact outcome",
                    "trigger": "Exact trigger",
                    "delivered_behavior": "Exact behavior",
                    "proof": "Exact proof",
                    "coverage_goals": [{"local": "goal"}]
                }]
            }))
            .unwrap(),
            covered_source_ref_ids: source_refs.to_vec(),
        }],
    };
    service_supplier
        .supply_authored(&tect_application::ScopeAuthoredManifestRequest {
            tenant_id: tenant,
            observation: observed.clone(),
            authored_scope_set: authored_scope_set.clone(),
        })
        .await
        .unwrap();
    let service = WorkspaceService::new_with_scope_sources(
        std::sync::Arc::new(store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        service_authority,
        service_supplier,
    );
    let no_call = service
        .run_scope_advisory(
            &tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session.to_string(),
                workspace_key: format!("scope-live-{workspace}"),
            },
            &tect_application::RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id: candidate,
                session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                authored_scope_set: Some(authored_scope_set),
            },
        )
        .await
        .unwrap();
    assert_eq!(no_call.opportunity.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        no_call.opportunity.primary_reason,
        AdvisoryReason::CapabilityUnavailable
    );
    assert!(no_call.advice.is_none());
    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(no_call.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dispatch_count, 0);
    assert_eq!(
        authority
            .observe(&ScopeAuthorityRequest {
                actor_id: Uuid::new_v4(),
                ..authority_request
            })
            .await,
        Err(Error::Forbidden)
    );
    let prepared = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert!(
        unit.scope_advisory_manifest_by_request_key(workspace, &request_key)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        unit.scope_advisory_manifest_by_request_key(Uuid::new_v4(), &request_key)
            .await
            .unwrap()
            .is_none()
    );
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
    let make_record = |manifest: ScopeConstructorManifest| ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest,
    };
    let missing =
        super::live_support::manifest(candidate, snapshot, program, &[(source_refs[0], D)]);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(missing))
            .await,
        Err(Error::InvalidSource)
    );
    let extra = super::live_support::manifest(
        candidate,
        snapshot,
        program,
        &[
            (source_refs[0], D),
            (source_refs[1], D),
            (Uuid::new_v4(), D),
        ],
    );
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(extra))
            .await,
        Err(Error::InvalidSource)
    );
    let different_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let altered = super::live_support::manifest(
        candidate,
        snapshot,
        program,
        &[(source_refs[0], D), (source_refs[1], different_digest)],
    );
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(altered))
            .await,
        Err(Error::InvalidSource)
    );
    let mut omitted_coverage = manifest.clone();
    omitted_coverage.emitted[0].coverage.pop();
    reseal_manifest(&mut omitted_coverage);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(omitted_coverage))
            .await,
        Err(Error::InvalidSource)
    );
    let mut stale_snapshot = manifest.clone();
    stale_snapshot.source.snapshot_id = Uuid::new_v4();
    reseal_manifest(&mut stale_snapshot);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(stale_snapshot))
            .await,
        Err(Error::StaleRevision)
    );
    let authored_request_digest = "a".repeat(64);
    unit.prepare_authored_scope_advisory_manifest(workspace, &prepared, &authored_request_digest)
        .await
        .unwrap();
    let stored = unit
        .scope_advisory_manifest_by_request_key(workspace, &request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.record, prepared);
    assert_eq!(
        stored.authored_request_digest.as_deref(),
        Some(authored_request_digest.as_str())
    );
    unit.commit().await.unwrap();
    let mut replay = rw(&store, &enrollment.auth, tenant).await;
    let stored_replay = replay
        .scope_advisory_manifest_by_request_key(workspace, &request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_replay, stored);
    assert_eq!(
        replay
            .prepare_authored_scope_advisory_manifest(workspace, &prepared, &"b".repeat(64),)
            .await,
        Err(Error::InputConflict)
    );
    replay.commit().await.unwrap();

    let stale_opportunity = Uuid::new_v4();
    let stale_request_key = format!("request-{stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&stale_request_key).bind(D).execute(&pool).await.unwrap();
    let stale_prepared = ScopeManifestRecord {
        opportunity_id: stale_opportunity,
        candidate_set_id: candidate,
        config_revision: prepared.config_revision,
        opportunity_material_digest: prepared.opportunity_material_digest.clone(),
        manifest: manifest.clone(),
    };
    let stale_disposition = ScopePreparedAdvisoryDisposition {
        opportunity_id: stale_opportunity,
        candidate_set_id: candidate,
        expected_source_digest: manifest.source.digest.clone(),
        reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut stale_unit = rw(&store, &enrollment.auth, tenant).await;
    stale_unit
        .prepare_scope_advisory_manifest(workspace, &stale_prepared)
        .await
        .unwrap();
    stale_unit
        .finalize_prepared_scope_advisory_without_dispatch(workspace, &stale_disposition)
        .await
        .unwrap();
    stale_unit.commit().await.unwrap();
    let mut stale_replay = rw(&store, &enrollment.auth, tenant).await;
    stale_replay
        .finalize_prepared_scope_advisory_without_dispatch(workspace, &stale_disposition)
        .await
        .unwrap();
    stale_replay.commit().await.unwrap();
    let stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stale_state,
        ("no_call".into(), "deterministic_input_invalid".into())
    );
    let stale_attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_attempts, 0);

    sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(dispatch).bind(tenant).bind(workspace).bind(opportunity).bind(D).execute(&pool).await.unwrap();
    let dispatched_invalidation = ScopePreparedAdvisoryDisposition {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        expected_source_digest: manifest.source.digest.clone(),
        reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut dispatched_unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        dispatched_unit
            .finalize_prepared_scope_advisory_without_dispatch(workspace, &dispatched_invalidation,)
            .await,
        Err(Error::InputConflict)
    );
    dispatched_unit.commit().await.unwrap();
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

    let dispatch_authorization = |opportunity_id, dispatch_id| AdvisoryDispatchAuthorization {
        dispatch_id,
        opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: "fixture".into(),
        model: "jev".into(),
        configuration_snapshot: serde_json::json!({
            "provider_profile_ref": "fixture",
            "model_configuration": { "model": "jev" }
        }),
        configuration_digest: D.into(),
        material_digest: D.into(),
        payload_digest: D.into(),
        request_payload: b"fixture request".to_vec(),
    };

    // Authorization fences a stale authored source before inserting a dispatch.
    let authorize_stale_opportunity = Uuid::new_v4();
    let authorize_stale_request = format!("request-{authorize_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(authorize_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&authorize_stale_request).bind(D).execute(&pool).await.unwrap();
    let authorize_stale_record = ScopeManifestRecord {
        opportunity_id: authorize_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(
            workspace,
            &authorize_stale_record,
            &"c".repeat(64),
        )
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let stale_authorization = dispatch_authorization(authorize_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        crate::advisory::authorize_dispatch_for_test(
            &mut tx,
            tenant,
            workspace,
            1,
            &stale_authorization,
        )
        .await,
        Err(Error::StaleContext)
    );
    tx.commit().await.unwrap();
    let stale_auth_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(authorize_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_auth_dispatches, 0);

    // If the frozen source changes after authorization, dispatch-start is the
    // DB linearization point: it records cancellation and a terminal no-call,
    // leaving send_started_at and provider outcome absent.
    let start_stale_opportunity = Uuid::new_v4();
    let start_stale_request = format!("request-{start_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'4',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(start_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&start_stale_request).bind(D).execute(&pool).await.unwrap();
    let mut current_manifest = manifest.clone();
    current_manifest.source.candidate_set_revision = 4;
    reseal_manifest(&mut current_manifest);
    let start_stale_record = ScopeManifestRecord {
        opportunity_id: start_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: current_manifest,
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(workspace, &start_stale_record, &"d".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let authorization = dispatch_authorization(start_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized =
        crate::advisory::authorize_dispatch_for_test(&mut tx, tenant, workspace, 1, &authorization)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let dispatch_id = authorized.id;
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let started = crate::advisory::start_dispatch_for_test(&mut tx, tenant, workspace, dispatch_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!started.should_send);
    assert_eq!(started.dispatch.state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        started.dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let dispatch_audit: (String, String, Option<String>, bool, bool, bool) = sqlx::query_as(
        "SELECT state,send_certainty,outcome,send_started_at IS NULL,response_payload IS NULL,sealed_at IS NOT NULL FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(dispatch_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch_audit,
        (
            "cancelled".into(),
            "not_sent".into(),
            None,
            true,
            true,
            true
        )
    );
    let opportunity_audit: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(start_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        opportunity_audit,
        ("no_call".into(), "deterministic_input_invalid".into())
    );

    let config_stale_opportunity = Uuid::new_v4();
    let config_stale_request = format!("request-{config_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'5',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(config_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&config_stale_request).bind(D).execute(&pool).await.unwrap();
    let mut config_manifest = manifest.clone();
    config_manifest.source.candidate_set_revision = 5;
    reseal_manifest(&mut config_manifest);
    let config_stale_record = ScopeManifestRecord {
        opportunity_id: config_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: config_manifest,
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(workspace, &config_stale_record, &"e".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let config_stale_authorization =
        dispatch_authorization(config_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized = crate::advisory::authorize_dispatch_for_test(
        &mut tx,
        tenant,
        workspace,
        1,
        &config_stale_authorization,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,2,1,'disabled',NULL,NULL,$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_workspace_config SET revision=2,mode='disabled',provider_profile_ref=NULL,model_configuration=NULL WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let started =
        crate::advisory::start_dispatch_for_test(&mut tx, tenant, workspace, authorized.id)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(!started.should_send);
    assert_eq!(started.dispatch.state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        started.dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let config_stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(config_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        config_stale_state,
        ("invalidated".into(), "configuration_changed".into())
    );
}

#[test]
fn authored_request_digest_requires_lowercase_sha256_hex() {
    assert!(super::valid_authored_request_digest(&"a".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"A".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"g".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"a".repeat(63)));
}

#[test]
fn prepared_scope_disposition_uses_only_pre_dispatch_terminal_reasons() {
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::DeterministicInputInvalid),
        Ok("no_call")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ConfigurationChanged),
        Ok("invalidated")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ProviderUnconfigured),
        Err(Error::InvalidArguments)
    );
    assert!(super::valid_scope_source_digest(&"a".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"A".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"z".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"a".repeat(63)));
}

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn published_source_snapshots_are_permanently_frozen_and_refresh_advances() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let runtime = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("source-freeze-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',1,'p','i','b','finite','c','s','ready',0,1,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,status,boundary,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}','ready','finite',4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'body')")
        .bind(tenant).bind(workspace).bind(D).execute(&pool).await.unwrap();
    for (id, sequence) in [(first, 1_i64), (second, 2)] {
        sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,$5,1,1,1,$6,'{}',$6,'m','1',$6,'body','[]','1',$6,'[]')")
            .bind(id).bind(tenant).bind(workspace).bind(candidate).bind(sequence).bind(D)
            .execute(&pool).await.unwrap();
    }

    // Exercise the trigger through the actual runtime role and tenant policy.
    let mut runtime_tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *runtime_tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','intent',$5,'intent')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&mut *runtime_tx).await.unwrap();
    runtime_tx.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(first)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    let late = sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&pool).await;
    assert!(
        late.is_err(),
        "published snapshot accepted a late reference"
    );

    // A pending refresh can accumulate refs, then publish at a greater sequence.
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(second)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE scope_candidate_sets SET current_snapshot_id=$1,revision=revision+1 WHERE id=$2",
    )
    .bind(second)
    .bind(candidate)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
            .bind(first)
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=NULL WHERE id=$1")
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','constraints',$5,'constraints')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.is_err());

    // Publication holds the same set-row lock as manifest preparation.
    let third = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,3,1,1,1,$5,'{}',$5,'m','1',$5,'body','[]','1',$5,'[]')")
        .bind(third).bind(tenant).bind(workspace).bind(candidate).bind(D)
        .execute(&pool).await.unwrap();
    let mut publisher = pool.begin().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(third)
        .bind(candidate)
        .execute(&mut *publisher)
        .await
        .unwrap();
    let insert_pool = pool.clone();
    let insert = tokio::spawn(async move {
        sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','boundaries',$5,'boundaries')")
            .bind(tenant).bind(workspace).bind(candidate).bind(third).bind(D)
            .execute(&insert_pool).await
    });
    tokio::pin!(insert);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut insert)
            .await
            .is_err(),
        "insert did not wait for publication lock"
    );
    publisher.commit().await.unwrap();
    assert!(
        insert.await.unwrap().is_err(),
        "concurrent append escaped publication freeze"
    );
}
