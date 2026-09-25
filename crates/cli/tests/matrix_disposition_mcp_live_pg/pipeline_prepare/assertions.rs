use super::*;

async fn opportunity_count(pool: &PgPool, workspace: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM advisory_opportunity WHERE workspace_id=$1 AND capability='pipeline_recommendation'",
    ).bind(workspace).fetch_one(pool).await.unwrap()
}

pub(super) async fn exercise_prepare(
    pool: &PgPool,
    workspace: Uuid,
    owner: &mut Mcp,
    independent: &mut Mcp,
    set: Uuid,
    source: &Value,
    work: &Value,
    ready: &Value,
    chosen: &Value,
    matched: &Value,
    task: Uuid,
    pipeline_calls: &Arc<AtomicUsize>,
    definition_drift: &Arc<AtomicBool>,
    root: &std::path::Path,
    socket: &std::path::Path,
) {
    let scope = Uuid::parse_str(ready["scope"]["id"].as_str().unwrap()).unwrap();
    let before: (i64, String) = sqlx::query_as(
        "SELECT revision,status FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(set)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before.1, "ready");
    assert_eq!(opportunity_count(pool, workspace).await, 2);
    let base = json!({
        "candidate_set_id":set,"expected_candidate_set_revision":before.0,
        "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
        "request_key":format!("pipeline-{}", Uuid::new_v4())
    });
    let prepared = route(
        owner,
        "command",
        "pipeline.recommendation.prepare",
        base.clone(),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    assert_eq!(prepared["reason"], "recommendation_prepared");
    let expected: Vec<_> = tect_domain::PipelineKind::CURRENT_SLICE_RUN_KINDS
        .iter()
        .filter(|kind| **kind != PipelineKind::DebugRootCause)
        .map(|kind| kind.as_str())
        .collect();
    let expected_options: Vec<String> = expected
        .iter()
        .map(|kind| super::manifest_option_id_for_kind(&prepared, kind))
        .collect();
    assert_eq!(prepared["eligible_option_ids"], json!(expected_options));
    let opportunity = Uuid::parse_str(prepared["opportunity_id"].as_str().unwrap()).unwrap();
    let manifest: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    ).bind(workspace).bind(opportunity).fetch_one(pool).await.unwrap();
    assert_eq!(manifest["schema"], "tect.pipeline-recommendation/3");
    assert_eq!(manifest["work_id"], work["id"]);
    assert_eq!(manifest["work_revision"], work["revision"]);
    assert_eq!(manifest["matrix_task_id"], task.to_string());
    assert_eq!(manifest["selected_choice_id"], "b");
    assert_eq!(manifest["digest"], prepared["manifest_digest"]);
    assert_eq!(manifest["catalogue_revision"], "4");
    assert!(
        !manifest["mandatory_card_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(manifest["evidence_refs"], json!([]));
    let options = manifest["options"].as_array().unwrap();
    assert_eq!(options.len(), 7);
    assert_eq!(
        manifest["excluded"],
        json!([{
            "kind":PipelineKind::DebugRootCause.as_str(),
            "reason":"incompatible_candidate"
        }])
    );
    assert_eq!(
        manifest["compatibility_policy_digest"],
        explicit_fixture_policy().digest().unwrap()
    );
    let saved_policy_digest: String = sqlx::query_scalar(
        "SELECT compatibility_policy_digest FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    ).bind(workspace).bind(opportunity).fetch_one(pool).await.unwrap();
    assert_eq!(saved_policy_digest, manifest["compatibility_policy_digest"]);
    for (option, kind) in options.iter().zip(expected) {
        let pair_id = super::manifest_option_id_for_kind(&prepared, kind);
        assert_eq!(option["id"], pair_id);
        assert_eq!(option["kind"], kind);
        assert_eq!(
            prepared["options"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["option_id"] == pair_id)
                .unwrap()["verification_plan_id"],
            option["verification_plan"]["id"]
        );
        assert_eq!(
            option["id"],
            format!(
                "{kind}+{}",
                option["verification_plan"]["id"].as_str().unwrap()
            )
        );
        assert_eq!(option["definition_digest"].as_str().unwrap().len(), 64);
        assert!(!option["completion_contract"].as_str().unwrap().is_empty());
        let definition = tect_host::StaticPipelineRecommendationDefinitions
            .definition("4", serde_json::from_value(option["kind"].clone()).unwrap())
            .unwrap()
            .unwrap();
        let expected_plan =
            tect_domain::PipelineVerificationPlan::from_definition(&definition).unwrap();
        assert_eq!(
            option["verification_plan"],
            serde_json::to_value(expected_plan).unwrap()
        );
        assert_eq!(
            option["verification_plan"]["obligations"]
                .as_array()
                .unwrap()
                .len(),
            definition
                .phases
                .iter()
                .filter(|phase| phase.required)
                .count()
        );
    }
    let binding: (
        Uuid,
        i64,
        Uuid,
        i64,
        Uuid,
        Uuid,
        String,
        String,
        Vec<String>,
        String,
    ) = sqlx::query_as(
        "SELECT candidate_set_id,candidate_set_revision,work_node_id,work_node_revision,\
             matrix_disposition_id,match_effect_attestation_id,catalogue_revision,\
             catalogue_digest,eligible_option_ids,verification_contract_digest \
             FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((binding.0, binding.1), (set, before.0));
    assert_eq!(json!(binding.2), work["id"]);
    assert_eq!(json!(binding.3), work["revision"]);
    assert_eq!(json!(binding.4), chosen["disposition_id"]);
    let effect_attestation: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2",
    ).bind(workspace)
     .bind(Uuid::parse_str(matched["request_id"].as_str().unwrap()).unwrap())
     .fetch_one(pool).await.unwrap();
    assert_eq!(binding.5, effect_attestation);
    assert_eq!(binding.6, "4");
    assert_eq!(binding.7, manifest["catalogue_digest"]);
    assert_eq!(json!(binding.8), prepared["eligible_option_ids"]);
    assert_eq!(binding.9, prepared["manifest_digest"]);
    let source_binding: (Uuid, Uuid, String, String) = sqlx::query_as(
        "SELECT planning_snapshot_id,source_snapshot_id,source_snapshot_digest,\
         (SELECT source_revision FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2) \
         FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(json!(source_binding.0), ready["snapshot"]["id"]);
    assert_eq!(json!(source_binding.1), source["snapshot"]["id"]);
    assert_eq!(
        source_binding.3,
        source["candidate_set"]["revision"].to_string()
    );
    let selected_sources_digest: String = sqlx::query_scalar(
        "SELECT selected_sources_digest FROM scope_candidate_snapshots WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(source_binding.1)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        source_binding.2,
        tect_application::pipeline_recommendation_source_digest(
            source_binding.1,
            source_binding.3.parse().unwrap(),
            &selected_sources_digest,
        )
        .unwrap()
    );
    let provenance: (
        String,
        Uuid,
        Option<Uuid>,
        Option<String>,
        Option<String>,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT work_item_kind,work_item_id,run_id,phase,step,state,primary_reason \
             FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(provenance.0, "slice_candidate_node");
    assert_eq!(json!(provenance.1), work["id"]);
    assert!(provenance.2.is_none() && provenance.3.is_none() && provenance.4.is_none());
    assert_eq!(
        (provenance.5.as_str(), provenance.6.as_str()),
        ("prepared", "recommendation_prepared")
    );

    let replay = route(
        owner,
        "command",
        "pipeline.recommendation.prepare",
        base.clone(),
    )
    .await;
    assert_eq!(replay["opportunity_id"], prepared["opportunity_id"]);
    assert_eq!(replay["manifest_digest"], prepared["manifest_digest"]);
    let mut changed = base.clone();
    changed["request_preference"] = json!("skip");
    assert_error(
        &route_error(owner, "command", "pipeline.recommendation.prepare", changed).await,
        &["input_conflict"],
    );
    let mut stale_node = base.clone();
    stale_node["request_key"] = json!(format!("stale-node-{}", Uuid::new_v4()));
    stale_node["expected_work_node_revision"] = json!(work["revision"].as_i64().unwrap() + 1);
    assert_error(
        &route_error(
            owner,
            "command",
            "pipeline.recommendation.prepare",
            stale_node,
        )
        .await,
        &["stale_context"],
    );
    let mut stale_set = base.clone();
    stale_set["request_key"] = json!(format!("stale-set-{}", Uuid::new_v4()));
    stale_set["expected_candidate_set_revision"] = json!(before.0 - 1);
    assert_error(
        &route_error(
            owner,
            "command",
            "pipeline.recommendation.prepare",
            stale_set,
        )
        .await,
        &["stale_context"],
    );
    let forbidden = route_error(
        independent,
        "command",
        "pipeline.recommendation.prepare",
        json!({"candidate_set_id":set,"expected_candidate_set_revision":before.0,
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":format!("verifier-{}", Uuid::new_v4())}),
    )
    .await;
    assert_error(&forbidden, &["forbidden"]);
    assert_eq!(opportunity_count(pool, workspace).await, 3);

    let mut skip = base.clone();
    skip["request_key"] = json!(format!("skip-{}", Uuid::new_v4()));
    skip["request_preference"] = json!("skip");
    let no_call = route(owner, "command", "pipeline.recommendation.prepare", skip).await;
    assert_eq!(no_call["state"], "no_call");
    assert_eq!(no_call["reason"], "request_skip");
    assert_eq!(
        no_call["eligible_option_ids"],
        prepared["eligible_option_ids"]
    );
    let no_call_id = Uuid::parse_str(no_call["opportunity_id"].as_str().unwrap()).unwrap();
    let no_call_manifest: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    ).bind(workspace).bind(no_call_id).fetch_one(pool).await.unwrap();
    assert_eq!(no_call_manifest, manifest);
    for id in [opportunity, no_call_id] {
        let dispatches: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
        )
        .bind(workspace)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(dispatches, 0);
    }
    let slices: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_slices WHERE workspace_id=$1 AND scope_id=$2",
    )
    .bind(workspace)
    .bind(scope)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(slices, 0);
    let after: (i64, String) = sqlx::query_as(
        "SELECT revision,status FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(set)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(after, before);
    assert_eq!(opportunity_count(pool, workspace).await, 4);

    let run_request = json!({"opportunity_id":opportunity});
    let forbidden_run = route_error(
        independent,
        "command",
        "pipeline.recommendation.run",
        run_request.clone(),
    )
    .await;
    assert_error(&forbidden_run, &["forbidden"]);
    assert_eq!(pipeline_calls.load(Ordering::SeqCst), 0);
    let ranked = route(
        owner,
        "command",
        "pipeline.recommendation.run",
        run_request.clone(),
    )
    .await;
    assert_eq!(ranked["status"], "ranked", "{ranked}");
    assert_eq!(ranked["opportunity_id"], prepared["opportunity_id"]);
    assert_eq!(ranked["ranked_ids"], prepared["eligible_option_ids"]);
    assert_eq!(pipeline_calls.load(Ordering::SeqCst), 1);
    let dispatch_id = Uuid::parse_str(ranked["dispatch_id"].as_str().unwrap()).unwrap();
    let dispatch: (String, String, String, String, String, Vec<u8>, Vec<u8>, String) = sqlx::query_as(
        "SELECT state,send_certainty,outcome,material_digest,payload_digest,request_payload,response_payload,\
         pipeline_response_sha256 FROM advisory_dispatch WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(dispatch_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        (&dispatch.0[..], &dispatch.1[..], &dispatch.2[..]),
        ("sealed", "sent", "provider_response")
    );
    assert_eq!(dispatch.3, prepared["manifest_digest"]);
    assert!(!dispatch.5.is_empty());
    let sent: Value = serde_json::from_slice(&dispatch.5).unwrap();
    assert_eq!(sent["eligible_option_ids"], prepared["eligible_option_ids"]);
    assert!(
        !sent["eligible_option_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(PipelineKind::DebugRootCause.as_str()))
    );
    assert_eq!(dispatch.4, format!("{:x}", Sha256::digest(&dispatch.5)));
    assert_eq!(dispatch.7, format!("{:x}", Sha256::digest(&dispatch.6)));
    let saved_rank: tect_domain::PipelineRecommendationRanking =
        serde_json::from_slice(&dispatch.6).unwrap();
    saved_rank
        .validate(&serde_json::from_value(manifest.clone()).unwrap())
        .unwrap();
    let mut reintroduced = ranked["ranked_ids"].as_array().unwrap().clone();
    reintroduced.push(json!(PipelineKind::DebugRootCause.as_str()));
    let hostile_response: PipelineRecommendationRanking = serde_json::from_value(json!({
        "status":"ranked","ranked_ids":reintroduced
    }))
    .unwrap();
    assert!(
        hostile_response
            .validate(&serde_json::from_value(manifest.clone()).unwrap())
            .is_err()
    );
    for stale_id in [
        format!(
            "{}+verification-plan:{}",
            options[0]["kind"].as_str().unwrap(),
            "0".repeat(64)
        ),
        "unknown+verification-plan:unknown".to_owned(),
        options[0]["kind"].as_str().unwrap().to_owned(),
    ] {
        let invalid = PipelineRecommendationRanking::Ranked {
            ranked_ids: vec![stale_id],
        };
        assert!(
            invalid
                .validate(&serde_json::from_value(manifest.clone()).unwrap())
                .is_err()
        );
    }
    assert_eq!(
        serde_json::to_value(saved_rank).unwrap()["ranked_ids"],
        ranked["ranked_ids"]
    );
    assert_error(
        &route_error(owner, "command", "pipeline.recommendation.run", run_request).await,
        &["input_conflict"],
    );
    let no_call_run = route(
        owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":no_call_id}),
    )
    .await;
    assert_eq!(no_call_run["status"], "no_call");
    assert_eq!(no_call_run["reason"], "request_skip");
    assert_eq!(pipeline_calls.load(Ordering::SeqCst), 1);
    let dispatch_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(dispatch_count, 1);
    let slices_after_run: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_slices WHERE workspace_id=$1 AND scope_id=$2",
    )
    .bind(workspace)
    .bind(scope)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(slices_after_run, 0);

    let mut stale_base = base.clone();
    stale_base["request_key"] = json!(format!("stale-run-{}", Uuid::new_v4()));
    let stale_prepared = route(
        owner,
        "command",
        "pipeline.recommendation.prepare",
        stale_base,
    )
    .await;
    assert_eq!(stale_prepared["state"], "prepared");

    let rejected_rank = route(
        owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":stale_prepared["opportunity_id"]}),
    )
    .await;
    assert_eq!(rejected_rank["status"], "ranked");
    let reject_request = json!({
        "request_id":Uuid::new_v4(),
        "opportunity_id":stale_prepared["opportunity_id"],
        "expected_work_revision":work["revision"],
        "manifest_digest":stale_prepared["manifest_digest"],
        "action":"reject_recommendation","rationale":"Retain caller judgment"
    });
    let rejected = route(
        owner,
        "command",
        "pipeline.recommendation.disposition",
        reject_request,
    )
    .await;
    assert!(rejected["selected_kind"].is_null());
    assert!(rejected["selected_option_id"].is_null());
    let rejected_open = json!({
        "request_id":Uuid::new_v4(),"scope_id":scope,
        "scope_revision":ready["scope"]["revision"],
        "candidate_set_id":set,
        "candidate_set_revision":ready["candidate_set"]["revision"],
        "candidate_snapshot_id":ready["snapshot"]["id"],
        "candidate_id":work["id"],"candidate_revision":work["revision"],
        "disposition_id":rejected["id"]
    });
    assert_error(
        &route_error(owner, "command", "slice.open", rejected_open).await,
        &["forbidden"],
    );

    let excluded_baseline_request = json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,
        "expected_work_revision":work["revision"],
        "manifest_digest":prepared["manifest_digest"],
        "action":"use_deterministic_choice","rationale":"Probe excluded saved Work baseline"
    });
    assert_error(
        &route_error(
            owner,
            "command",
            "pipeline.recommendation.disposition",
            excluded_baseline_request,
        )
        .await,
        &["invalid_arguments"],
    );
    let disposition_request = json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,
        "expected_work_revision":work["revision"],
        "manifest_digest":prepared["manifest_digest"],
        "action":"accept_recommendation","rationale":"Use the eligible top rank"
    });
    let forbidden = route_error(
        independent,
        "command",
        "pipeline.recommendation.disposition",
        disposition_request.clone(),
    )
    .await;
    assert_error(&forbidden, &["forbidden"]);
    let disposition = route(
        owner,
        "command",
        "pipeline.recommendation.disposition",
        disposition_request.clone(),
    )
    .await;
    assert_eq!(disposition["request"], disposition_request);
    assert_eq!(disposition["selected_option_id"], ranked["ranked_ids"][0]);
    assert_eq!(disposition["selected_kind"], options[0]["kind"]);
    assert_eq!(disposition["advice"]["status"], "ranked");
    let replay = route(
        owner,
        "command",
        "pipeline.recommendation.disposition",
        disposition_request.clone(),
    )
    .await;
    assert_eq!(replay["id"], disposition["id"]);
    let mut conflict = disposition_request.clone();
    conflict["action"] = json!("reject_recommendation");
    assert_error(
        &route_error(
            owner,
            "command",
            "pipeline.recommendation.disposition",
            conflict,
        )
        .await,
        &["input_conflict"],
    );
    let slices_before_open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_slices WHERE workspace_id=$1 AND scope_id=$2",
    )
    .bind(workspace)
    .bind(scope)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(slices_before_open, 0);
    let open = json!({
        "request_id":Uuid::new_v4(),"scope_id":scope,
        "scope_revision":ready["scope"]["revision"],
        "candidate_set_id":set,
        "candidate_set_revision":ready["candidate_set"]["revision"],
        "candidate_snapshot_id":ready["snapshot"]["id"],
        "candidate_id":work["id"],"candidate_revision":work["revision"],
        "disposition_id":disposition["id"]
    });
    definition_drift.store(true, Ordering::SeqCst);
    assert_error(
        &route_error(owner, "command", "slice.open", open.clone()).await,
        &["stale_context"],
    );
    let slices_after_drift: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_slices WHERE workspace_id=$1 AND scope_id=$2",
    )
    .bind(workspace)
    .bind(scope)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(slices_after_drift, slices_before_open);
    definition_drift.store(false, Ordering::SeqCst);
    let mut wrong = open.clone();
    wrong["candidate_snapshot_id"] = json!(Uuid::new_v4());
    assert_error(
        &route_error(owner, "command", "slice.open", wrong).await,
        &["stale_context", "stale_revision"],
    );
    let opened = route(owner, "command", "slice.open", open.clone()).await;
    assert_eq!(opened["created"]["pipeline"], options[0]["kind"]);
    assert_eq!(
        opened["created"]["selected_option_id"],
        ranked["ranked_ids"][0]
    );
    assert_eq!(
        opened["created"]["verification_plan_id"],
        options[0]["verification_plan"]["id"]
    );
    assert_eq!(
        opened["created"]["verification_plan_schema"],
        options[0]["verification_plan"]["schema"]
    );
    assert_eq!(
        opened["created"]["verification_plan_digest"],
        options[0]["verification_plan"]["digest"]
    );
    assert_eq!(
        opened["created"]["verification_plan_source_definition_version"],
        options[0]["verification_plan"]["source_definition_version"]
    );
    let reopened = route(owner, "command", "slice.open", open.clone()).await;
    assert_eq!(reopened["replay"]["id"], opened["created"]["id"]);

    super::open_effect::exercise(
        pool,
        workspace,
        owner,
        independent,
        &open,
        &opened,
        &disposition,
        chosen,
        matched,
        work,
        &options[0]["kind"],
        root,
        socket,
    )
    .await;

    // A newer authoritative Matrix task revision invalidates this saved path.
    let mut revised_input = input();
    revised_input["promised_behavior"]["value"] = json!("A revised synthetic promise");
    let mut revised_choices = choice(task, &["a", "b"]);
    revised_choices["version"] = json!(2);
    revised_choices["task_revision"] = json!("2");
    route(
        owner,
        "command",
        "task.source.record",
        json!({
            "task_id":task,"revision":2,"expected_current_revision":1,
            "request_id":Uuid::new_v4(),"input":revised_input,
            "choice_set":revised_choices
        }),
    )
    .await;
    let mut stale_source = base;
    stale_source["request_key"] = json!(format!("stale-source-{}", Uuid::new_v4()));
    assert_error(
        &route_error(
            owner,
            "command",
            "pipeline.recommendation.prepare",
            stale_source,
        )
        .await,
        &["stale_context", "not_found"],
    );
    let mut stale_open = open;
    stale_open["request_id"] = json!(Uuid::new_v4());
    assert_error(
        &route_error(owner, "command", "slice.open", stale_open).await,
        &["stale_context"],
    );
    assert_eq!(pipeline_calls.load(Ordering::SeqCst), 2);
    assert_eq!(opportunity_count(pool, workspace).await, 5);
}
