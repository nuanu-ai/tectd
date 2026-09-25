use super::live_support::{D, manifest, reseal_manifest, rw};
use super::*;
use crate::{PgStore, admin, store::PgUnitOfWork};
use tect_application::{
    AntiBloatApplication, AntiBloatAttemptState, AntiBloatNoCall, AntiBloatStore,
    DisabledAntiBloatRankingProvider, UnitOfWork,
};

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
    let (links, dependency_digest, graph_provenance) = authored_graph_binding(&authored).unwrap();
    assert_eq!(links.len(), 2);
    let input = AntiBloatInput {
        selected_id: authored.baseline_id.clone(),
        manifest: authored,
        graph_provenance,
        dependency_digest,
        obligation_links: links,
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
async fn authored_manifest_writes_binding_and_pg_reader_prepares_no_call() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let enrollment = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let actor = enrollment.principal_id;
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let source_ref = Uuid::new_v4();
    let opportunity = Uuid::new_v4();

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
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(enrollment.auth.host_id).bind(workspace)
        .bind(session.to_string()).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'intent')")
        .bind(tenant).bind(workspace).bind(D).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(D)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_field','intent',$6,'intent')")
        .bind(source_ref).bind(tenant).bind(workspace).bind(candidate).bind(snapshot).bind(D)
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

    let mut authored = manifest(candidate, snapshot, program, &[(source_ref, D)]);
    authored.constructor = source_authored_identity();
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
    let input = AntiBloatStore::authoritative_input(&mut reader, workspace, candidate, 3)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(input.manifest, authored);
    assert_eq!(input.graph_provenance, binding.0);
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
    let prepared = app
        .prepare(
            workspace,
            actor,
            candidate,
            3,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.state,
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings)
    );
    let no_send = app.prepare_send(prepared.review_id).await.unwrap();
    assert_eq!(no_send.state, prepared.state);
    assert!(no_send.permit.is_none());
    Box::new(app.store).commit().await.unwrap();
    let review_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(review_count, 1);
}
