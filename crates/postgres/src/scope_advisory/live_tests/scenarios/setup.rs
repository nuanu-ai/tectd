macro_rules! verify {
    ($actor:ident, $advice:ident, $auth:ident, $authored_scope_set:ident, $authority:ident, $authority_request:ident, $candidate:ident, $dispatch:ident, $enrollment:ident, $manifest:ident, $observation:ident, $observed:ident, $opportunity:ident, $pool:ident, $program:ident, $reason:ident, $request_key:ident, $runtime_pool:ident, $service:ident, $session:ident, $snapshot:ident, $source_refs:ident, $state:ident, $store:ident, $tenant:ident, $verifier_session:ident, $workspace:ident $(,)?) => {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let $pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&$pool, &role).await.unwrap();
    let $enrollment = admin::enroll_host(&$pool, None, vec![]).await.unwrap();
    let $tenant = $enrollment.tenant_id;
    let $actor = $enrollment.principal_id;
    let $workspace = Uuid::new_v4();
    let $session = Uuid::new_v4();
    let $verifier_session = Uuid::new_v4();
    let $program = Uuid::new_v4();
    let $candidate = Uuid::new_v4();
    let $snapshot = Uuid::new_v4();
    let mut $source_refs = [Uuid::new_v4(), Uuid::new_v4()];
    $source_refs.sort();
    let $opportunity = Uuid::new_v4();
    let $dispatch = Uuid::new_v4();
    let $request_key = format!("request-{opportunity}", $opportunity = $opportunity);
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind($workspace)
        .bind($tenant)
        .bind(format!("scope-live-{workspace}", $workspace = $workspace))
        .execute(&$pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind($tenant)
        .bind($workspace)
        .bind($actor)
        .execute(&$pool)
        .await
        .unwrap();
    for id in [$session, $verifier_session] {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind($tenant).bind($enrollment.$auth.host_id).bind($workspace).bind(id.to_string())
            .execute(&$pool).await.unwrap();
    }
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind($program).bind($tenant).bind($workspace).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind($candidate).bind($tenant).bind($workspace).bind($program).bind(Uuid::new_v4()).execute(&$pool).await.unwrap();
    let frozen_program_body = serde_json::json!({
        "id": $program,
        "workspace_id": $workspace,
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
        .bind($tenant).bind($workspace).bind(D).bind(frozen_program_body).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind($snapshot).bind($tenant).bind($workspace).bind($candidate).bind(D).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_field','intent',$6,'intent')")
        .bind($source_refs[0]).bind($tenant).bind($workspace).bind($candidate).bind($snapshot)
        .bind(D).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_success',$6,'success')")
        .bind($source_refs[1]).bind($tenant).bind($workspace).bind($candidate).bind($snapshot)
        .bind(D).execute(&$pool).await.unwrap();
    let blank_digest = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'   ')")
        .bind($tenant).bind($workspace).bind(blank_digest).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','name',$5,'name')")
        .bind($tenant).bind($workspace).bind($candidate).bind($snapshot).bind(blank_digest)
        .execute(&$pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind($snapshot)
        .bind($candidate)
        .execute(&$pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind($opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(&$request_key).bind(D).execute(&$pool).await.unwrap();
    let preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind($opportunity).fetch_one(&$pool).await.unwrap();
    assert_eq!(preselection, (true, 0));

    let $manifest = $manifest(
        $candidate,
        $snapshot,
        $program,
        &[($source_refs[0], D), ($source_refs[1], D)],
    );
    let $store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let $runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let $authority =
        PgScopeAuthorityObserver::new($store.clone(), std::sync::Arc::new(FixtureCandidateGuidance));
    let $authority_request = ScopeAuthorityRequest {
        tenant_id: $tenant,
        workspace_id: $workspace,
        actor_id: $actor,
        session_id: $session,
        candidate_set_id: $candidate,
    };
    let $observed = $authority.observe(&$authority_request).await.unwrap();
    let ScopeAuthorityOutcome::Authorized($observed) = $observed else {
        panic!("persisted source must be authorized");
    };
    assert_eq!($observed.source, $manifest.source);
    assert_eq!($observed.obligations, $manifest.obligations);
    let service_authority = std::sync::Arc::new(PgScopeAuthorityObserver::new(
        $store.clone(),
        std::sync::Arc::new(FixtureCandidateGuidance),
    ));
    let service_supplier = std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
        $store.clone(),
        service_authority.clone(),
    ));
    let $authored_scope_set = tect_application::AuthoredScopeSet {
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
                    "source_ref_id": $source_refs[1],
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
            covered_source_ref_ids: $source_refs.to_vec(),
        }],
    };
    service_supplier
        .supply_authored(&tect_application::ScopeAuthoredManifestRequest {
            tenant_id: $tenant,
            $observation: (*$observed).clone(),
            $authored_scope_set: $authored_scope_set.clone(),
        })
        .await
        .unwrap();
    let $service = WorkspaceService::new_with_scope_sources(
        std::sync::Arc::new($store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        service_authority,
        service_supplier,
    );
    let no_call = $service
        .run_scope_advisory(
            &tect_domain::RequestContext {
                $auth: $enrollment.$auth.clone(),
                native_session_id: $session.to_string(),
                workspace_key: format!("scope-live-{workspace}", $workspace = $workspace),
            },
            &tect_application::RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id: $candidate,
                session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                $authored_scope_set: Some($authored_scope_set.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(no_call.$opportunity.$state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        no_call.$opportunity.primary_reason,
        AdvisoryReason::CapabilityUnavailable
    );
    assert!(no_call.$advice.is_none());
    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(no_call.$opportunity.id)
            .fetch_one(&$pool)
            .await
            .unwrap();
    assert_eq!(dispatch_count, 0);
    let provider_prepares = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let provider_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let budget_service = WorkspaceService::new_with_scope_advisory_adapters(
        std::sync::Arc::new($store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            $store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
        std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
            $store.clone(),
            std::sync::Arc::new(PgScopeAuthorityObserver::new(
                $store.clone(),
                std::sync::Arc::new(FixtureCandidateGuidance),
            )),
        )),
        std::sync::Arc::new(DenyScopeBudget),
        std::sync::Arc::new(CountingCapableProvider(provider_prepares.clone(), provider_calls.clone())),
    );
    let budget_no_call = budget_service
        .run_scope_advisory(
            &tect_domain::RequestContext {
                $auth: $enrollment.$auth.clone(),
                native_session_id: $session.to_string(),
                workspace_key: format!("scope-live-{workspace}", $workspace = $workspace),
            },
            &tect_application::RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id: $candidate,
                session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                $authored_scope_set: Some($authored_scope_set.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        budget_no_call.$opportunity.$state,
        AdvisoryOpportunityState::NoCall
    );
    assert_eq!(
        budget_no_call.$opportunity.primary_reason,
        AdvisoryReason::BudgetPolicyInvalid
    );
    assert!(budget_no_call.$advice.is_none());
    assert_eq!(provider_prepares.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(provider_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let ($state, $reason): (String, String) =
        sqlx::query_as("SELECT state,primary_reason FROM advisory_opportunity WHERE id=$1")
            .bind(budget_no_call.$opportunity.id)
            .fetch_one(&$pool)
            .await
            .unwrap();
    assert_eq!(
        ($state.as_str(), $reason.as_str()),
        ("no_call", "budget_policy_invalid")
    );
    let budget_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(budget_no_call.$opportunity.id)
            .fetch_one(&$pool)
            .await
            .unwrap();
    assert_eq!(budget_dispatches, 0);
    };
}

pub(in super::super) use verify;
