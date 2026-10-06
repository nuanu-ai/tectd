macro_rules! anti_core_part_0 { ($active:ident, $actor:ident, $adapters:ident, $admin_pool:ident, $advice:ident, $advice_request:ident, $advice_writer:ident, $after:ident, $app:ident, $app_reader:ident, $apply:ident, $apply_store:ident, $attestation:ident, $attestation_count:ident, $authored:ident, $authored_delta:ident, $before:ident, $before_app:ident, $binding:ident, $bytes:ident, $caller_link:ident, $candidate:ident, $ceilings:ident, $decider:ident, $dispatch_id:ident, $disposition:ident, $draft:ident, $eligible:ident, $enrollment:ident, $evidence_digest:ident, $exploratory:ident, $finding:ident, $foreign:ident, $foreign_context:ident, $foreign_session:ident, $foreign_verifier:ident, $foreign_workspace:ident, $from:ident, $id:ident, $install:ident, $later:ident, $lineage:ident, $link:ident, $now:ident, $observed:ident, $opportunity:ident, $ordinary:ident, $ordinary_writer:ident, $other_workspace:ident, $owner_context:ident, $persisted:ident, $policies:ident, $policy:ident, $prepared:ident, $program:ident, $program_body:ident, $reader:ident, $receipt:ident, $record:ident, $replay:ident, $replay_store:ident, $request:ident, $reservation_count:ident, $resolved:ident, $resolver:ident, $review_count:ident, $runtime_pool:ident, $runtime_url:ident, $save:ident, $saver:ident, $seed:ident, $selected:ident, $selected_app:ident, $selected_reader:ident, $service:ident, $session:ident, $shadow_count:ident, $snapshot:ident, $source_digest:ident, $source_ref:ident, $stale:ident, $stale_review:ident, $stale_verify:ident, $store:ident, $stored:ident, $tenant:ident, $unchanged_revision:ident, $until:ident, $verifier:ident, $verifier_context:ident, $verifier_session:ident, $verify:ident, $workspace:ident, $workspace_key:ident, $writer:ident, $wrong:ident, $wrong_digest:ident) => {
    let ($admin_pool, $runtime_pool) = crate::technical_decision_comparison_pg_tests::isolated_pg::connect_and_migrate().await;
    let $runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").expect("part0.rs:3");
    let $enrollment = admin::enroll_host(&$admin_pool, None, vec![]).await.expect("part0.rs:4");
    let $tenant = $enrollment.tenant_id;
    let $actor = $enrollment.principal_id;
    let $workspace = Uuid::new_v4();
    let $other_workspace = Uuid::new_v4();
    let $session = Uuid::new_v4();
    let $program = Uuid::new_v4();
    let $candidate = Uuid::new_v4();
    let $snapshot = Uuid::new_v4();
    let $source_ref = Uuid::new_v4();
    let $opportunity = Uuid::new_v4();
    let $source_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind($workspace)
        .bind($tenant)
        .bind(format!("anti-bloat-live-{workspace}", $workspace=$workspace))
        .execute(&$admin_pool)
        .await
        .expect("part0.rs:23");
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind($tenant)
        .bind($workspace)
        .bind($actor)
        .execute(&$admin_pool)
        .await
        .expect("part0.rs:30");
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind($other_workspace)
        .bind($tenant)
        .bind(format!("anti-bloat-other-{other_workspace}", $other_workspace=$other_workspace))
        .execute(&$admin_pool)
        .await
        .expect("part0.rs:37");
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind($tenant)
        .bind($other_workspace)
        .bind($actor)
        .execute(&$admin_pool)
        .await
        .expect("part0.rs:44");
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind($session).bind($tenant).bind($enrollment.auth.host_id).bind($workspace)
        .bind($session.to_string()).execute(&$admin_pool).await.expect("part0.rs:47");
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind($program).bind($tenant).bind($workspace).execute(&$admin_pool).await.expect("part0.rs:49");
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'review_required','finite',2,2,4096)")
        .bind($candidate).bind($tenant).bind($workspace).bind($program).bind(Uuid::new_v4())
        .execute(&$admin_pool).await.expect("part0.rs:52");
    let $program_body = serde_json::json!({
        "id": $program, "workspace_id": $workspace, "status": "open", "revision": 4,
        "name": "p", "intent": "i", "basis": "b", "boundaries": "finite",
        "constraints": "c", "success": "s", "working_notes": null,
        "pending_question": null, "current_step": "ready", "input_cursor": 2,
        "latest_input": 2
    })
    .to_string();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4)")
        .bind($tenant).bind($workspace).bind(D).bind($program_body)
        .execute(&$admin_pool).await.expect("part0.rs:63");
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'s')")
        .bind($tenant).bind($workspace).bind($source_digest)
        .execute(&$admin_pool).await.expect("part0.rs:66");
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind($snapshot).bind($tenant).bind($workspace).bind($candidate).bind(D)
        .execute(&$admin_pool).await.expect("part0.rs:69");
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_success',$6,'success')")
        .bind($source_ref).bind($tenant).bind($workspace).bind($candidate).bind($snapshot).bind($source_digest)
        .execute(&$admin_pool).await.expect("part0.rs:72");
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind($snapshot)
        .bind($candidate)
        .execute(&$admin_pool)
        .await
        .expect("part0.rs:78");
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$admin_pool).await.expect("part0.rs:80");
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$admin_pool).await.expect("part0.rs:82");
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$admin_pool).await.expect("part0.rs:84");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind($opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(format!("anti-bloat-{opportunity}", $opportunity=$opportunity)).bind(D).execute(&$admin_pool).await.expect("part0.rs:87");

    let $store = PgStore::connect(&$runtime_url, 4).await.expect("part0.rs:89");
    let mut $before = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    $before.authenticate(&$enrollment.auth).await.expect("part0.rs:91");
    assert!(
        AntiBloatStore::authoritative_input(&mut $before, $workspace, $candidate, 3)
            .await
            .expect("part0.rs:95")
            .is_none()
    );
    let mut $before_app = AntiBloatApplication {
        $store: $before,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        $before_app
            .prepare(
                $workspace,
                $actor,
                $candidate,
                3,
                AdvisoryRequestPreference::UseWorkspace,
            )
            .await,
        Err(Error::NotFound)
    );
    drop($before_app);

    let mut $authored = manifest($candidate, $snapshot, $program, &[($source_ref, $source_digest)]);
    $authored.constructor = source_authored_identity();
    let $draft: ScopeCandidateDraft = serde_json::from_value(serde_json::json!({
        "boundary": "finite",
        "goals": [{
            "identity": {"local": "goal"},
            "text": "Preserve the source result",
            "source_ref_id": $source_ref,
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
    .expect("part0.rs:145");
    let $seed = authored_seed(
        $tenant,
        $workspace,
        &$authored.source,
        &$authored.constructor,
        "baseline",
    )
    .expect("part0.rs:153");
    let mut $resolver = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    let $resolved = crate::scope_candidates::resolve::resolve_authored(
        $resolver.transaction().expect("part0.rs:156"),
        &crate::scope_candidates::resolve::ResolveContext {
            tenant_id: $tenant,
            workspace_id: $workspace,
            candidate_set_id: $candidate,
            snapshot_id: $snapshot,
            latest_input: 2,
        },
        &$draft,
        None,
        &$seed,
        &std::collections::BTreeSet::$from([$source_ref]),
        &$authored.constructor,
    )
    .await
    .expect("part0.rs:170");
    drop($resolver);
    $authored.emitted[0].material = $resolved.clone();
    $authored.emitted[0].material_digest =
        scope_candidate_material_digest(&Sha256ScopeDigest, &$resolved).expect("part0.rs:174");
    reseal_manifest(&mut $authored);
    let $record = ScopeManifestRecord {
        opportunity_id: $opportunity,
        candidate_set_id: $candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: $authored.clone(),
    };
    let mut $writer = rw(&$store, &$enrollment.auth, $tenant).await;
    $writer
        .prepare_authored_scope_advisory_manifest($workspace, &$record, D)
        .await
        .expect("part0.rs:187");
    $writer.commit().await.expect("part0.rs:188");

    let $binding: (String, serde_json::Value, serde_json::Value) = sqlx::query_as(
        "SELECT provenance,obligation_links,mandatory_policy_obligation_ids \
         FROM scope_anti_bloat_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3"
    ).bind($tenant).bind($workspace).bind($candidate).fetch_one(&$admin_pool).await.expect("part0.rs:193");
    assert!(
        $binding
            .0
            .starts_with("tect.source-authored-graph-binding/1:")
    );
    assert_eq!($binding.1.as_array().expect("part0.rs:199").len(), 1);
    assert_eq!($binding.2, serde_json::json!([]));

    let mut $reader = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    $reader.authenticate(&$enrollment.auth).await.expect("part0.rs:203");
    assert!(
        AntiBloatStore::authoritative_input(&mut $reader, $workspace, $candidate, 3)
            .await
            .expect("part0.rs:207")
            .is_none()
    );
    assert!(
        AntiBloatStore::authoritative_input(&mut $reader, $workspace, $candidate, 4)
            .await
            .expect("part0.rs:213")
            .is_none()
    );
    drop($reader);

    let mut $app_reader = PgUnitOfWork::test_begin(&$runtime_pool, $tenant).await;
    $app_reader.authenticate(&$enrollment.auth).await.expect("part0.rs:219");
    let mut $app = AntiBloatApplication {
        $store: $app_reader,
        provider: DisabledAntiBloatRankingProvider,
    };
    assert_eq!(
        $app.prepare(
            $workspace,
            $actor,
            $candidate,
            3,
            AdvisoryRequestPreference::UseWorkspace
        )
        .await,
        Err(Error::NotFound)
    );
    Box::new($app.$store).commit().await.expect("part0.rs:235");
    let $review_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind($tenant)
    .bind($workspace)
    .fetch_one(&$admin_pool)
    .await
    .expect("part0.rs:243");
    assert_eq!($review_count, 0);


}; }
