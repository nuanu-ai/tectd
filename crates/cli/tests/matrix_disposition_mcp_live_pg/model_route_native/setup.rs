use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn lineage(
    pool: &PgPool,
    owner: &mut Mcp,
    _store: &PgStore,
    enrolled: &admin::Enrollment,
    socket: &std::path::Path,
    root: &std::path::Path,
    _owner_config: &std::path::Path,
    workspace_key: &str,
    workspace: Uuid,
    repo: &std::path::Path,
) -> Value {
    let (context, candidate, _) = source::ready(pool, owner, repo).await;
    identity(pool).await;
    let verifier = admin::prepare_verifier_enrollment(pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let config = root.join("verifier.json");
    host_file(&config, &verifier.auth);
    let mut independent =
        Mcp::start(socket, &config, &Uuid::new_v4().to_string(), workspace_key).await;
    call(
        pool,
        &mut independent,
        "command",
        "workspace.open",
        json!({}),
    )
    .await;
    let task = Uuid::new_v4();
    identity(pool).await;
    let recorded = record_task(owner, task, &["a", "b"]).await;
    identity(pool).await;
    verify(&mut independent, &recorded, task).await;
    let opportunity = call(
        pool,
        owner,
        "command",
        "engineering.advisory.request",
        json!({"task_id":task,"expected_task_revision":1,"request_key":format!("no-call-{task}")}),
    )
    .await;
    assert_eq!(
        opportunity["state"], "no_call",
        "no Matrix provider installed: {opportunity}"
    );
    let chosen = call(
        pool,
        owner,
        "command",
        "engineering.matrix.disposition.record",
        disposition(
            &recorded,
            task,
            &opportunity,
            "no_call",
            None,
            json!({"outcome":"selected","selected_choice_id":"b"}),
        ),
    )
    .await;
    let verification:String=sqlx::query_scalar("SELECT matrix_verification_digest FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(Uuid::parse_str(opportunity["opportunity_id"].as_str().unwrap()).unwrap()).fetch_one(pool).await.unwrap();
    let selection = json!({"task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],"selected_choice_id":"b","expected_input_digest":recorded["input_digest"],"expected_choice_set_digest":recorded["choice_set_digest"],"expected_verification_digest":verification,"mapped_draft_node_indices":[0]});
    let scope=call(pool,owner,"command","scope.open",json!({"request_id":Uuid::new_v4(),"candidate_set_id":context["candidate_set"]["id"],"candidate_set_revision":context["candidate_set"]["revision"],"candidate_snapshot_id":context["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let mut request = planning_effect::save_request(&scope["created"]["planning"], selection);
    request["draft"]["nodes"][0]["model_route_facts"] = json!({"role":"agent","tool":"code","data_class":"internal","remaining_budget_units":20,"available_latency_ms":100});
    let caller = request["request_id"].clone();
    let saved = call(pool, owner, "command", "slice.candidates.save", request).await;
    let set = &saved["candidate_set"]["id"];
    let work = &saved["draft"]["nodes"][0];
    let effect = call(
        pool,
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({"candidate_set_id":set,"caller_request_id":caller}),
    )
    .await;
    let attestation=call(pool,&mut independent,"command","engineering.matrix.planning_effect.verify",json!({"request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller,"expected_result_revision":effect["material"]["result_revision"],"expected_effect_digest":effect["effect_digest"],"verdict":"matches","summary":"Fixture proves exact selected Work mapping, not candidate model execution."})).await;
    assert_eq!(attestation["verdict"], "matches");
    independent.finish().await;
    json!({"disposition_id":chosen["disposition_id"],"expected_task_id":task,"expected_task_revision":1,"expected_candidate_set_id":set,"expected_caller_request_id":caller,"expected_mapped_work_node_id":work["id"],"expected_mapped_work_node_revision":work["revision"],"request_key":format!("native-model-route-{}",Uuid::new_v4()),"requested_route_id":"route-a"})
}

pub(super) async fn budget(
    pool: &PgPool,
    store: &PgStore,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    workspace: Uuid,
) -> Value {
    use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};
    let actor: Uuid = sqlx::query_scalar("SELECT principal_id FROM hosts WHERE id=$1")
        .bind(auth.host_id)
        .fetch_one(pool)
        .await
        .unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[94u8; 32]).unwrap();
    let hex = |v: &[u8]| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let keys = json!([{"workspace_id":workspace,"owner_id":actor,"public_key_hex":hex(key.public_key().as_ref())}]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let from = now - 60_000;
    let until = now + 600_000;
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 1_000_000,
        elapsed_monotonic_ms: 120_000,
        retry_dispatches: 1,
    };
    let id = Uuid::new_v4();
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings),
        from,
        until,
        ceilings,
        actor,
        "0".repeat(128),
    )
    .unwrap();
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        unsigned.digest().into(),
        from,
        until,
        ceilings,
        actor,
        hex(key
            .sign(&unsigned.approval_signing_message(workspace).unwrap())
            .as_ref()),
    )
    .unwrap();
    identity(pool).await;
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(auth).await.unwrap();
    unit.set_tenant(tenant).await.unwrap();
    unit.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &signed)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    keys
}
