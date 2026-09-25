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
    assert_eq!(opportunity_count(pool, workspace).await, 0);
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
        .map(|kind| kind.as_str())
        .collect();
    assert_eq!(prepared["eligible_kind_ids"], json!(expected));
    let opportunity = Uuid::parse_str(prepared["opportunity_id"].as_str().unwrap()).unwrap();
    let manifest: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    ).bind(workspace).bind(opportunity).fetch_one(pool).await.unwrap();
    assert_eq!(manifest["schema"], "tect.pipeline-recommendation/1");
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
    assert_eq!(options.len(), 8);
    for (option, kind) in options.iter().zip(expected) {
        assert_eq!(option["id"], kind);
        assert_eq!(option["kind"], kind);
        assert_eq!(option["definition_digest"].as_str().unwrap().len(), 64);
        assert!(!option["completion_contract"].as_str().unwrap().is_empty());
        assert!(option["obligations"].is_array());
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
             catalogue_digest,eligible_kind_ids,verification_contract_digest\
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
    assert_eq!(json!(binding.8), prepared["eligible_kind_ids"]);
    assert_eq!(binding.9, prepared["manifest_digest"]);
    let source_binding: (Uuid, Uuid, String, String) = sqlx::query_as(
        "SELECT planning_snapshot_id,source_snapshot_id,source_snapshot_digest,\
         (SELECT source_revision FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2)\
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
        "SELECT work_item_kind,work_item_id,run_id,phase,step,state,primary_reason\
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
    assert_eq!(opportunity_count(pool, workspace).await, 1);

    let mut skip = base.clone();
    skip["request_key"] = json!(format!("skip-{}", Uuid::new_v4()));
    skip["request_preference"] = json!("skip");
    let no_call = route(owner, "command", "pipeline.recommendation.prepare", skip).await;
    assert_eq!(no_call["state"], "no_call");
    assert_eq!(no_call["reason"], "request_skip");
    assert_eq!(no_call["eligible_kind_ids"], prepared["eligible_kind_ids"]);
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
    assert_eq!(opportunity_count(pool, workspace).await, 2);

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
        &["stale_context"],
    );
    assert_eq!(opportunity_count(pool, workspace).await, 2);
}
