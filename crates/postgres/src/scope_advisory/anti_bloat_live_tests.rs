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
    let input = AntiBloatInput {
        selected_revision: authored.source.candidate_set_revision + 1,
        selected_id: authored.baseline_id.clone(),
        manifest: authored,
        graph_provenance,
        dependency_digest,
        obligation_links: links,
        non_goal_source_obligation_ids: vec![],
        mandatory_policy_obligation_ids: vec![],
    };
    let review = review_anti_bloat(&Sha256ScopeDigest, &input).unwrap();
    assert_eq!(review.findings.len(), 2);
    assert!(
        review.findings.iter().all(|finding| {
            finding.class == AntiBloatClass::NecessaryResult && !finding.rankable
        })
    );
}

#[tokio::test]
#[ignore = "requires disposable migrated PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL"]
async fn selected_save_activates_exact_source_bound_anti_bloat_review() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        identity.0,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap()
    );
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    let migration: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&admin_pool)
        .await
        .unwrap();
    assert_eq!(
        migration.to_string(),
        std::env::var("TECT_TEST_EXPECTED_MIGRATION_VERSION").unwrap()
    );
    let enrollment = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let actor = enrollment.principal_id;
    let workspace = Uuid::new_v4();
    let other_workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let source_ref = Uuid::new_v4();
    let opportunity = Uuid::new_v4();
    let source_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("anti-bloat-live-{workspace}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(other_workspace)
        .bind(tenant)
        .bind(format!("anti-bloat-other-{other_workspace}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(other_workspace)
        .bind(actor)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(enrollment.auth.host_id).bind(workspace)
        .bind(session.to_string()).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&admin_pool).await.unwrap();
    let program_body = serde_json::json!({
        "id": program, "workspace_id": workspace, "status": "open", "revision": 4,
        "name": "p", "intent": "i", "basis": "b", "boundaries": "finite",
        "constraints": "c", "success": "s", "working_notes": null,
        "pending_question": null, "current_step": "ready", "input_cursor": 2,
        "latest_input": 2
    })
    .to_string();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4)")
        .bind(tenant).bind(workspace).bind(D).bind(program_body)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'s')")
        .bind(tenant).bind(workspace).bind(source_digest)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(D)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_success',$6,'success')")
        .bind(source_ref).bind(tenant).bind(workspace).bind(candidate).bind(snapshot).bind(source_digest)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(snapshot)
        .bind(candidate)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("anti-bloat-{opportunity}")).bind(D).execute(&admin_pool).await.unwrap();

    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let mut before = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    before.authenticate(&enrollment.auth).await.unwrap();
    assert!(
        AntiBloatStore::authoritative_input(&mut before, workspace, candidate, 3)
            .await
            .unwrap()
            .is_none()
    );
    let mut before_app = AntiBloatApplication {
        store: before,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        before_app
            .prepare(
                workspace,
                actor,
                candidate,
                3,
                AdvisoryRequestPreference::UseWorkspace,
            )
            .await,
        Err(Error::NotFound)
    );
    drop(before_app);

    let mut authored = manifest(candidate, snapshot, program, &[(source_ref, source_digest)]);
    authored.constructor = source_authored_identity();
    let draft: ScopeCandidateDraft = serde_json::from_value(serde_json::json!({
        "boundary": "finite",
        "goals": [{
            "identity": {"local": "goal"},
            "text": "Preserve the source result",
            "source_ref_id": source_ref,
            "resolution": {"kind": "candidate", "reference": {"local": "candidate"}}
        }],
        "candidates": [{
            "identity": {"local": "candidate"},
            "title": "Required result",
            "outcome": "Required result",
            "trigger": "Source",
            "delivered_behavior": "Deliver the required result",
            "proof": "Acceptance test",
            "coverage_goals": [{"local": "goal"}]
        }, {
            "identity": {"local": "exploratory"},
            "grounding": {"kind": "exploratory_unrequested", "provenance": "source_authored_v2"},
            "title": "Unrequested exploratory dashboard",
            "outcome": "Optional dashboard",
            "trigger": "Exploration",
            "delivered_behavior": "Show a dashboard",
            "proof": "Optional visual check",
            "coverage_goals": []
        }]
    }))
    .unwrap();
    let seed = authored_seed(
        tenant,
        workspace,
        &authored.source,
        &authored.constructor,
        "baseline",
    )
    .unwrap();
    let mut resolver = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    let resolved = crate::scope_candidates::resolve::resolve_authored(
        resolver.transaction().unwrap(),
        &crate::scope_candidates::resolve::ResolveContext {
            tenant_id: tenant,
            workspace_id: workspace,
            candidate_set_id: candidate,
            snapshot_id: snapshot,
            latest_input: 2,
        },
        &draft,
        None,
        &seed,
        &std::collections::BTreeSet::from([source_ref]),
    )
    .await
    .unwrap();
    drop(resolver);
    authored.emitted[0].material = resolved.clone();
    authored.emitted[0].material_digest =
        scope_candidate_material_digest(&Sha256ScopeDigest, &resolved).unwrap();
    reseal_manifest(&mut authored);
    let record = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: authored.clone(),
    };
    let mut writer = rw(&store, &enrollment.auth, tenant).await;
    writer
        .prepare_authored_scope_advisory_manifest(workspace, &record, D)
        .await
        .unwrap();
    writer.commit().await.unwrap();

    let binding: (String, serde_json::Value, serde_json::Value) = sqlx::query_as(
        "SELECT provenance,obligation_links,mandatory_policy_obligation_ids \
         FROM scope_anti_bloat_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3"
    ).bind(tenant).bind(workspace).bind(candidate).fetch_one(&admin_pool).await.unwrap();
    assert!(
        binding
            .0
            .starts_with("tect.source-authored-graph-binding/1:")
    );
    assert_eq!(binding.1.as_array().unwrap().len(), 1);
    assert_eq!(binding.2, serde_json::json!([]));

    let mut reader = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    reader.authenticate(&enrollment.auth).await.unwrap();
    assert!(
        AntiBloatStore::authoritative_input(&mut reader, workspace, candidate, 3)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        AntiBloatStore::authoritative_input(&mut reader, workspace, candidate, 4)
            .await
            .unwrap()
            .is_none()
    );
    drop(reader);

    let mut app_reader = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    app_reader.authenticate(&enrollment.auth).await.unwrap();
    let mut app = AntiBloatApplication {
        store: app_reader,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        app.prepare(
            workspace,
            actor,
            candidate,
            3,
            AdvisoryRequestPreference::UseWorkspace
        )
        .await,
        Err(Error::NotFound)
    );
    Box::new(app.store).commit().await.unwrap();
    let review_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(review_count, 0);

    let dispatch_id = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','y','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(dispatch_id).bind(tenant).bind(workspace).bind(opportunity).bind(D)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity).execute(&admin_pool).await.unwrap();
    let advice_request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &authored).unwrap();
    let advice = guard_scope_advice(
        &Sha256ScopeDigest,
        opportunity,
        &authored,
        &advice_request,
        &NormalizedScopeAdviceAnswers {
            answers: vec![NormalizedScopeAdviceAnswer {
                alternative_id: authored.baseline_id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score: ScopeAdviceScoreBand::StrongFit,
                choice_confidence: ConfidenceBasisPoints(9000),
                score_confidence: ConfidenceBasisPoints(8000),
            }],
        },
    )
    .unwrap();
    let mut advice_writer = rw(&store, &enrollment.auth, tenant).await;
    advice_writer
        .persist_guarded_scope_advice(
            workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                dispatch_id,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                advice: advice.clone(),
            },
        )
        .await
        .unwrap();
    advice_writer.commit().await.unwrap();
    let mut decider = rw(&store, &enrollment.auth, tenant).await;
    let disposition = decider
        .cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: ScopeDispositionRequest {
                    request_id: Uuid::new_v4(),
                    advice_id: advice.id.clone(),
                    expected_revision: 0,
                    action: ScopeDispositionAction::Accept,
                    selected_id: Some(authored.baseline_id.clone()),
                    items: vec![ScopeDispositionItem {
                        alternative_id: authored.baseline_id.clone(),
                        state: ScopeDispositionItemState::Selected,
                    }],
                    rationale: "Select the source-grounded result".into(),
                },
            },
        )
        .await
        .unwrap();
    decider.commit().await.unwrap();
    let save = SaveCandidateDraft {
        candidate_set_id: candidate,
        revision: 3,
        snapshot_id: snapshot,
        input_cursor: 2,
        request_id: Uuid::new_v4(),
        draft,
        consumed_knowledge: None,
        selected_advisory: Some(SelectedScopeAdvisory {
            opportunity_id: opportunity,
            disposition_id: disposition.id,
            selected_id: authored.baseline_id.clone(),
            alternative_key: "baseline".into(),
        }),
    };
    let mut saver = rw(&store, &enrollment.auth, tenant).await;
    let stored = saver
        .save_selected_candidate_draft(workspace, actor, session, &save)
        .await
        .unwrap();
    assert_eq!(stored.context.candidate_set.revision, 4);
    assert_eq!(stored.draft, Some(resolved.clone()));
    saver.commit().await.unwrap();

    let lineage: (i64, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT b.selected_draft_revision,b.selected_material_digest,b.selected_caller_link_id, \
                b.selected_caller_request_id FROM scope_anti_bloat_bindings b \
         WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
           AND b.candidate_set_revision=4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(candidate)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(lineage.0, 4);
    assert_eq!(lineage.1, authored.emitted[0].material_digest);
    assert_eq!(lineage.3, save.request_id);
    let caller_link: Uuid = sqlx::query_scalar(
        "SELECT link_id FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND request_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(candidate)
    .bind(save.request_id)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(lineage.2, caller_link);

    let mut selected_reader = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    selected_reader
        .authenticate(&enrollment.auth)
        .await
        .unwrap();
    let selected =
        AntiBloatStore::authoritative_input(&mut selected_reader, workspace, candidate, 4)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(selected.selected_revision, 4);
    assert_eq!(selected.selected_id, authored.baseline_id);
    assert_eq!(selected.manifest, authored);
    for (other_workspace, other_revision) in [(workspace, 3), (workspace, 5), (other_workspace, 4)]
    {
        assert!(
            AntiBloatStore::authoritative_input(
                &mut selected_reader,
                other_workspace,
                candidate,
                other_revision
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    let mut selected_app = AntiBloatApplication {
        store: selected_reader,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        selected_app
            .prepare(
                workspace,
                actor,
                candidate,
                3,
                AdvisoryRequestPreference::UseWorkspace
            )
            .await,
        Err(Error::NotFound)
    );
    assert_eq!(
        selected_app
            .prepare(
                other_workspace,
                actor,
                candidate,
                4,
                AdvisoryRequestPreference::UseWorkspace
            )
            .await,
        Err(Error::NotFound)
    );
    let prepared = selected_app
        .prepare(
            workspace,
            actor,
            candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.state,
        tect_application::AntiBloatAttemptState::Prepared
    );
    let stale_review = selected_app
        .prepare(
            workspace,
            actor,
            candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    let exploratory = resolved
        .candidates
        .iter()
        .find(|value| !value.grounding.is_source_grounded())
        .unwrap();
    let finding = prepared
        .review
        .findings
        .iter()
        .find(|value| value.candidate_id == exploratory.id)
        .unwrap();
    assert!(finding.rankable);
    let authored_delta = AntiBloatAuthoredDelta {
        review_id: prepared.review_id,
        finding_id: finding.id.clone(),
        disposition: AntiBloatDisposition::Narrow,
        delta: CandidateDeltaBatch {
            candidate_set_id: candidate,
            expected_revision: 4,
            idempotency_key: format!("live-narrow-{}", prepared.review_id),
            operations: vec![CandidateDeltaOperation::CandidateRemove {
                candidate_id: exploratory.id,
                expected_revision: exploratory.revision,
            }],
        },
    };
    Box::new(selected_app.store).commit().await.unwrap();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1,
    };
    let mut policies = Vec::new();
    for version in [1, 2] {
        let id = Uuid::new_v4();
        let from = now - 60_000;
        let until = now + 600_000;
        let policy = AdvisoryBudgetPolicy::new(
            id,
            version,
            AdvisoryBudgetPolicy::digest_for(id, version, from, until, ceilings),
            from,
            until,
            ceilings,
            actor,
            "a".repeat(128),
        )
        .unwrap();
        let mut install = rw(&store, &enrollment.auth, tenant).await;
        install
            .advisory_budget_policy_store()
            .unwrap()
            .install_budget_policy(workspace, &policy)
            .await
            .unwrap();
        install.commit().await.unwrap();
        policies.push(policy);
    }
    let eligible = prepared
        .review
        .findings
        .iter()
        .filter(|finding| finding.rankable)
        .map(|finding| finding.id.clone())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "review": &prepared.review, "eligible_ids": &eligible,
    }))
    .unwrap();
    let request = AntiBloatPreparedRequest {
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        bytes,
    };
    let mut stale = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        stale
            .anti_bloat_store()
            .unwrap()
            .begin_send(&prepared, &request, &policies[0])
            .await,
        Err(Error::BudgetPolicyInvalid)
    );
    stale.commit().await.unwrap();
    let reservation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3"
    ).bind(tenant).bind(workspace).bind(prepared.review_id)
        .fetch_one(&admin_pool).await.unwrap();
    assert_eq!(reservation_count, 0);
    let mut active = rw(&store, &enrollment.auth, tenant).await;
    assert!(
        active
            .anti_bloat_store()
            .unwrap()
            .begin_send(&prepared, &request, &policies[1])
            .await
            .unwrap()
            .is_some()
    );
    drop(active); // Roll back the accepted reservation so the existing apply proof can continue.
    let mut apply_store = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    apply_store.authenticate(&enrollment.auth).await.unwrap();
    let mut apply = AntiBloatApplication {
        store: apply_store,
        provider: DisabledAntiBloatRankingProvider,
    };
    let receipt = apply.disposition_and_apply(&authored_delta).await.unwrap();
    assert_eq!((receipt.from_revision, receipt.to_revision), (4, 5));
    assert_eq!(receipt.source_digest, authored.source.digest);
    assert_eq!(
        receipt.before_material_digest,
        authored.emitted[0].material_digest
    );
    Box::new(apply.store).commit().await.unwrap();
    let persisted: (i64, serde_json::Value) = sqlx::query_as(
        "SELECT s.revision,d.payload FROM scope_candidate_sets s JOIN scope_candidate_drafts d \
         ON (d.tenant_id,d.workspace_id,d.candidate_set_id,d.set_revision)= \
            (s.tenant_id,s.workspace_id,s.id,s.revision) \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(candidate)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, 5);
    let after: ResolvedCandidateDraft = serde_json::from_value(persisted.1).unwrap();
    assert_eq!(after.candidates.len(), 1);
    assert_eq!(
        after.candidates[0].grounding,
        CandidateGrounding::SourceGrounded
    );
    assert_eq!(after.goals, resolved.goals);
    assert_eq!(
        scope_candidate_material_digest(&Sha256ScopeDigest, &after).unwrap(),
        receipt.after_material_digest
    );
    let link: (Uuid, i64, i64, String, serde_json::Value) = sqlx::query_as(
        "SELECT caller_request_id,from_revision,to_revision,source_digest,caller_receipt \
         FROM scope_anti_bloat_caller_links WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind(tenant).bind(workspace).bind(prepared.review_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        (link.0, link.1, link.2, link.3),
        (
            receipt.caller_request_id,
            4,
            5,
            receipt.source_digest.clone()
        )
    );
    assert_eq!(
        serde_json::from_value::<AntiBloatApplyReceipt>(link.4).unwrap(),
        receipt
    );
    let shadow_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3",
    )
    .bind(tenant).bind(workspace).bind(candidate).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(shadow_count, 0);
    let mut replay_store = PgUnitOfWork::test_begin(&runtime_pool, tenant).await;
    replay_store.authenticate(&enrollment.auth).await.unwrap();
    let mut replay = AntiBloatApplication {
        store: replay_store,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        replay.disposition_and_apply(&authored_delta).await.unwrap(),
        receipt
    );
    let mut wrong = authored_delta.clone();
    wrong.delta.idempotency_key.push_str("-different");
    assert_eq!(
        replay.disposition_and_apply(&wrong).await,
        Err(Error::InputConflict)
    );
    let mut stale = authored_delta.clone();
    stale.review_id = stale_review.review_id;
    stale.delta.idempotency_key.push_str("-stale");
    assert_eq!(
        replay.disposition_and_apply(&stale).await,
        Err(Error::InputConflict)
    );
    Box::new(replay.store).commit().await.unwrap();

    let verifier = admin::prepare_verifier_enrollment(&admin_pool, tenant, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(tenant).bind(verifier.auth.host_id).bind(workspace)
        .bind(verifier_session.to_string()).execute(&admin_pool).await.unwrap();
    let adapters = Arc::new(UnusedVerifierAdapters);
    let service = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime_pool.clone())),
        adapters.clone(),
        adapters,
    );
    let workspace_key = format!("anti-bloat-live-{workspace}");
    let verifier_context = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: verifier_session.to_string(),
        workspace_key: workspace_key.clone(),
    };
    let owner_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key,
    };
    assert_eq!(
        service
            .get_anti_bloat_verification_material(&owner_context, prepared.review_id)
            .await,
        Err(Error::Forbidden),
    );
    let (observed, evidence_digest) = service
        .get_anti_bloat_verification_material(&verifier_context, prepared.review_id)
        .await
        .unwrap();
    assert!(observed.source_fragments_match);
    assert_eq!(observed.after_saved, after);
    assert_eq!(observed.receipt, receipt);
    assert_eq!(observed.verdict().0, AntiBloatVerificationVerdict::Pass);
    let verify = VerifyAntiBloatApply {
        request_id: Uuid::new_v4(),
        review_id: prepared.review_id,
        expected_evidence_digest: evidence_digest.clone(),
    };
    assert_eq!(
        service
            .verify_anti_bloat_apply(&owner_context, &verify)
            .await,
        Err(Error::Forbidden),
    );
    let mut wrong_digest = verify.clone();
    wrong_digest.request_id = Uuid::new_v4();
    wrong_digest.expected_evidence_digest = "f".repeat(64);
    assert_eq!(
        service
            .verify_anti_bloat_apply(&verifier_context, &wrong_digest)
            .await,
        Err(Error::InputConflict),
    );
    let attestation = service
        .verify_anti_bloat_apply(&verifier_context, &verify)
        .await
        .unwrap();
    assert_eq!(attestation.verdict, AntiBloatVerificationVerdict::Pass);
    assert_eq!(
        attestation.reason,
        AntiBloatVerificationReason::FullGraphPreserved
    );
    assert_eq!(attestation.verifier_principal_id, verifier.principal_id);
    assert_ne!(attestation.verifier_principal_id, actor);
    assert_eq!(attestation.evidence_digest, evidence_digest);
    assert_eq!(
        service
            .verify_anti_bloat_apply(&verifier_context, &verify)
            .await
            .unwrap(),
        attestation
    );
    let attestation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_preservation_attestations \
         WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(prepared.review_id)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(attestation_count, 1);
    let unchanged_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    ).bind(tenant).bind(workspace).bind(candidate).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(unchanged_revision, 5);

    // A later native input advances the authoritative set. A new verifier
    // request for the old r4 -> r5 effect must then fail stale.
    let ordinary = RecordCandidateInput {
        candidate_set_id: candidate,
        revision: 5,
        request_id: Uuid::new_v4(),
        input: "A new planning input arrived after anti-bloat attestation".into(),
    };
    let mut ordinary_writer = rw(&store, &enrollment.auth, tenant).await;
    let later = ordinary_writer
        .record_candidate_input(workspace, session, &ordinary, ordinary.input.len() as i64)
        .await
        .unwrap();
    assert_eq!(later.context.candidate_set.revision, 6);
    ordinary_writer.commit().await.unwrap();
    let mut stale_verify = verify.clone();
    stale_verify.request_id = Uuid::new_v4();
    assert_eq!(
        service
            .verify_anti_bloat_apply(&verifier_context, &stale_verify)
            .await,
        Err(Error::StaleRevision),
    );
    assert_eq!(
        service
            .verify_anti_bloat_apply(&verifier_context, &verify)
            .await
            .unwrap(),
        attestation
    );

    let foreign = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let foreign_workspace = Uuid::new_v4();
    let foreign_session = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(foreign_workspace)
        .bind(foreign.tenant_id)
        .bind(format!("anti-bloat-foreign-{foreign_workspace}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    let foreign_verifier =
        admin::prepare_verifier_enrollment(&admin_pool, foreign.tenant_id, foreign_workspace)
            .await
            .unwrap()
            .try_commit()
            .await
            .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(foreign.tenant_id).bind(foreign_verifier.auth.host_id)
        .bind(foreign_workspace).bind(foreign_session.to_string())
        .execute(&admin_pool).await.unwrap();
    let foreign_context = RequestContext {
        auth: foreign_verifier.auth,
        native_session_id: foreign_session.to_string(),
        workspace_key: format!("anti-bloat-foreign-{foreign_workspace}"),
    };
    assert_eq!(
        service
            .get_anti_bloat_verification_material(&foreign_context, prepared.review_id)
            .await,
        Err(Error::NotFound),
    );
}
