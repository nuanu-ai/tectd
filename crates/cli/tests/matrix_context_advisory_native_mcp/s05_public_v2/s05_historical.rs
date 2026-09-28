//! A real 0104 S05 receipt survives 0113 as an immutable, readable no-send record.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh exact-owned disposable PostgreSQL 18.6 database"]
async fn populated_0104_s05_receipt_survives_0113_public_get_and_run_fails_closed() {
    let (pool, runtime_url) = s05_session_fixture(104).await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let tenant = enrolled.tenant_id;
    let actor = enrolled.principal_id;
    let workspace = Uuid::new_v4();
    let workspace_key = format!("s05-historical-{}", Uuid::new_v4());
    let session = Uuid::new_v4();
    let native_session = Uuid::new_v4().to_string();
    let program = Uuid::new_v4();
    let source_set = Uuid::new_v4();
    let scope = Uuid::new_v4();
    let candidate_set = Uuid::new_v4();
    let caller_request = Uuid::new_v4();
    let task = Uuid::new_v4();
    let opportunity = Uuid::new_v4();
    let disposition = Uuid::new_v4();
    let work_node = Uuid::new_v4();
    let key = format!("s05-0104-{}", Uuid::new_v4());
    let input = input();
    let choices = choice(task);
    let parsed_input: EngineeringMatrixInput = serde_json::from_value(input.clone()).unwrap();
    let parsed_choices: tect_domain::EngineeringChoiceSet =
        serde_json::from_value(choices.clone()).unwrap();
    let input_digest = tect_domain::matrix_input_digest(&parsed_input).unwrap();
    let choice_digest = parsed_choices.canonical_digest(&parsed_input).unwrap();
    let verification_digest = "c".repeat(64);
    let evaluation_digest = "d".repeat(64);

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(&workspace_key)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(enrolled.auth.host_id).bind(workspace)
        .bind(&native_session).execute(&mut *tx).await.unwrap();
    // Matrix selection occurred under disabled advice. The owner may still
    // select a choice after a no-call; optional route advice is enabled later.
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,mode,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,'disabled',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,0,'disabled',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',1,'Historical S05','Synthetic Work','Fixture','One Scope','No execution','Readable receipt','ready',0,1,4096)")
        .bind(program).bind(tenant).bind(workspace)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,origin_result,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'historical source','{}'::jsonb,'{}'::jsonb,1,'ready','finite',0,1,4096)")
        .bind(source_set).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload,origin_result) VALUES($1,$2,$3,$4,1,$5,$6,1,'finite','Historical scope','Synthetic Work','[]'::jsonb,'[]'::jsonb,$7,'{}'::jsonb,'{}'::jsonb)")
        .bind(scope).bind(tenant).bind(workspace).bind(source_set)
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id,revision,status) VALUES($1,$2,$3,$4,2,'draft')")
        .bind(candidate_set).bind(tenant).bind(workspace).bind(scope)
        .execute(&mut *tx).await.unwrap();
    let receipt_request = json!({"request_id":caller_request,"operation":"save_slice_draft"});
    let receipt_result = json!({"candidate_set_id":candidate_set,"revision":2,
        "mapped_nodes":[{"draft_index":0,"node_id":work_node,"node_revision":1}]});
    sqlx::query("INSERT INTO native_planning_receipts(tenant_id,workspace_id,entity_id,operation,request_id,request_payload,result_payload) VALUES($1,$2,$3,'save_slice_draft',$4,$5,$6)")
        .bind(tenant).bind(workspace).bind(candidate_set).bind(caller_request)
        .bind(receipt_request).bind(receipt_result)
        .execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO matrix_tasks(tenant_id,workspace_id,id,current_revision) VALUES($1,$2,$3,1)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO matrix_task_revisions(tenant_id,workspace_id,task_id,revision,request_id,input_schema,canonical_input,input_digest,recorded_by_principal_id,recorded_by_session_id,choice_set_schema,choice_set,choice_set_digest) VALUES($1,$2,$3,1,$4,'tect.engineering-matrix-input/1',$5,$6,$7,$8,'tect.matrix-choice-set/1',$9,$10)")
        .bind(tenant).bind(workspace).bind(task).bind(Uuid::new_v4())
        .bind(input).bind(&input_digest).bind(actor).bind(session)
        .bind(choices).bind(&choice_digest)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason,matrix_task_revision,matrix_choice_set_digest) VALUES($1,$2,$3,'matrix_task',$4,$5,$6,'1','engineering_profile','engineering.profile.before_selection',0,'use_workspace','use_workspace','fixture',$7,$8,'no_call','workspace_disabled',1,$9)")
        .bind(opportunity).bind(tenant).bind(workspace).bind(task).bind(session).bind(actor)
        .bind(Uuid::new_v4().to_string()).bind(&evaluation_digest).bind(&choice_digest)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO advisory_matrix_disposition(tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision,matrix_choice_set_digest,disposition_id,request_id,actor_id,session_id,basis,outcome,selected_choice_id) VALUES($1,$2,$3,$4,1,$5,$6,$7,$8,$9,'no_call','selected','b')")
        .bind(tenant).bind(workspace).bind(opportunity).bind(task).bind(&choice_digest)
        .bind(disposition).bind(Uuid::new_v4()).bind(actor).bind(session)
        .execute(&mut *tx).await.unwrap();
    let route_model = json!({"model":MODEL});
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional',$3,$4,$5,$6)")
        .bind(tenant).bind(workspace).bind(PROFILE).bind(&route_model).bind(actor).bind(session)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE advisory_workspace_config SET revision=1,mode='optional',provider_profile_ref=$3,model_configuration=$4,updated_by_principal_id=$5,updated_by_session_id=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND revision=0")
        .bind(tenant).bind(workspace).bind(PROFILE).bind(route_model).bind(actor).bind(session)
        .execute(&mut *tx).await.unwrap();
    let mapped_nodes = json!([{"draft_index":0,"node_id":work_node,"node_revision":1}]);
    sqlx::query("INSERT INTO matrix_planning_selection_links(tenant_id,workspace_id,candidate_set_id,caller_request_id,scope_id,disposition_id,task_id,task_revision,selected_choice_id,input_digest,choice_set_digest,verification_digest,evaluation_digest,catalogue_version,caller_principal_id,caller_session_id,result_revision,mapped_nodes) VALUES($1,$2,$3,$4,$5,$6,$7,1,'b',$8,$9,$10,$11,'1',$12,$13,2,$14)")
        .bind(tenant).bind(workspace).bind(candidate_set).bind(caller_request)
        .bind(scope).bind(disposition).bind(task).bind(&input_digest).bind(&choice_digest)
        .bind(&verification_digest).bind(&evaluation_digest).bind(actor).bind(session)
        .bind(mapped_nodes).execute(&mut *tx).await.unwrap();

    let caller_text = |field: &str, value: &str| ModelRouteFact::Known {
        value: value.to_owned(),
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: format!("urn:fixture:s05:{field}"),
            work_node_id: work_node,
            work_node_revision: 1,
        },
    };
    let caller_number = |field: &str, value: u64| ModelRouteFact::Known {
        value,
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: format!("urn:fixture:s05:{field}"),
            work_node_id: work_node,
            work_node_revision: 1,
        },
    };
    let host_fact = HostCapabilities.host_capabilities().unwrap();
    let ModelRouteFact::Known {
        provenance: ModelRouteFactProvenance::Host {
            evidence_ref: host_ref,
        },
        ..
    } = &host_fact
    else {
        panic!("host fixture fact")
    };
    let host_ref = host_ref.clone();
    let work_context = ModelRouteWorkContext {
        approved_matrix_selection: tect_domain::MatrixPlanningSelection {
            task_id: task,
            task_revision: 1,
            disposition_id: disposition,
            selected_choice_id: "b".into(),
            expected_input_digest: input_digest,
            expected_choice_set_digest: choice_digest,
            expected_verification_digest: verification_digest,
            mapped_draft_node_indices: vec![0],
        },
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: candidate_set,
            caller_request_id: caller_request,
            mapped_draft_node_index: 0,
            mapped_work_node_id: work_node,
            mapped_work_node_revision: 1,
        },
        context_authority: None,
        role: caller_text("role", "agent"),
        tool: caller_text("tool", "code"),
        data_class: caller_text("data_class", "internal"),
        host_capabilities: host_fact,
        remaining_budget_units: caller_number("remaining_budget_units", 20),
        available_latency_ms: caller_number("available_latency_ms", 100),
    };
    let catalogue = Routes.catalogue().unwrap().unwrap();
    let eligible = catalogue.eligible(&work_context).unwrap();
    assert_eq!(eligible.route_ids, vec!["route-a", "route-b"]);
    let historical = PreparedModelRouteRecommendation {
        workspace_id: workspace,
        request_key: key.clone(),
        origin_session_id: None,
        session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        advisory_config_revision: 1,
        work: work_context,
        catalogue: Some(catalogue),
        eligible: Some(eligible),
        preparation: tect_application::ModelRoutePreparation::Prepared,
        routes: ModelRouteRecord {
            requested_route_id: Some("route-a".into()),
            recommended_route_id: None,
            observed_actual: None,
        },
    };
    sqlx::query("INSERT INTO model_route_preparations(tenant_id,workspace_id,request_key,disposition_id,candidate_set_id,caller_request_id,work_node_id,work_node_revision,task_id,task_revision,advisory_mode,advisory_config_revision,work_digest,catalogue_digest,host_capability_evidence_ref,prepared_payload) VALUES($1,$2,$3,$4,$5,$6,$7,1,$8,1,'optional',1,$9,$10,$11,$12)")
        .bind(tenant).bind(workspace).bind(&key).bind(disposition).bind(candidate_set)
        .bind(caller_request).bind(work_node).bind(task)
        .bind(historical.work.digest().unwrap())
        .bind(historical.catalogue.as_ref().unwrap().digest().unwrap())
        .bind(&host_ref).bind(serde_json::to_value(&historical).unwrap())
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let config: (i64, String) = sqlx::query_as("SELECT revision,mode FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(config, (1, "optional".into()));
    let matrix_basis: (i64, String, String, String, String) = sqlx::query_as("SELECT o.config_revision,o.state,o.primary_reason,d.basis,d.outcome FROM advisory_opportunity o JOIN advisory_matrix_disposition d ON (d.tenant_id,d.workspace_id,d.opportunity_id)=(o.tenant_id,o.workspace_id,o.id) WHERE o.id=$1")
        .bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(
        matrix_basis,
        (
            0,
            "no_call".into(),
            "workspace_disabled".into(),
            "no_call".into(),
            "selected".into()
        )
    );

    let before: (Value, Vec<u8>) = sqlx::query_as("SELECT to_jsonb(p),convert_to(p.prepared_payload::text,'UTF8') FROM model_route_preparations p WHERE workspace_id=$1 AND request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert!(before.0.get("origin_session_id").is_none());
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let ledger: (i64, Option<i64>) =
        sqlx::query_as("SELECT count(*),max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ledger, (113, Some(113)));
    let after: (Value, Vec<u8>, Option<Uuid>) = sqlx::query_as("SELECT to_jsonb(p)-'origin_session_id',convert_to(p.prepared_payload::text,'UTF8'),origin_session_id FROM model_route_preparations p WHERE workspace_id=$1 AND request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert_eq!((&after.0, &after.1), (&before.0, &before.1));
    assert_eq!(after.2, None);

    let socket = root.join("historical-s05.sock");
    let calls = Arc::new(AtomicUsize::new(0));
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_model_route_ranking_provider(Arc::new(FakeJev {
            calls: calls.clone(),
        })),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_file = root.join("owner.json");
    host_file(&owner_file, &enrolled.auth);
    let mut owner = Mcp::start(&socket, &owner_file, &native_session, &workspace_key).await;
    let got = route(
        &mut owner,
        "query",
        "model.route.get",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(got["preparation"]["request_key"], key);
    assert!(got["preparation"]["origin_session_id"].is_null());
    assert_eq!(
        got["preparation"]["work"]["selection_link"]["mapped_work_node_id"],
        json!(work_node)
    );
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "model.route.run",
            json!({"preparation_request_key":key})
        )
        .await["error"]["code"],
        "forbidden"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert_eq!(attempts, 0);
    let budget_rows: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM model_route_budget_reservations WHERE workspace_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(budget_rows, (0, 0));
    let final_row: (Value, Vec<u8>, Option<Uuid>) = sqlx::query_as("SELECT to_jsonb(p)-'origin_session_id',convert_to(p.prepared_payload::text,'UTF8'),origin_session_id FROM model_route_preparations p WHERE workspace_id=$1 AND request_key=$2")
        .bind(workspace).bind(&key).fetch_one(&pool).await.unwrap();
    assert_eq!(final_row, after);
    owner.finish().await;
    server.abort();
}
