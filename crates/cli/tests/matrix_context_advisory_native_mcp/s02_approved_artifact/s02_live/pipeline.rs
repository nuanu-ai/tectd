//! No-network S03 preparation from the same Owner-case V2 Matrix effect.
use super::*;
use tect_domain::{EngineeringMode, PipelineRecommendationManifest};

pub(super) struct OwnerCasePipeline<'a> {
    pub pool: &'a PgPool,
    pub runtime_url: &'a str,
    pub keys: BudgetOwnerKeys,
    pub approval: ApprovedMatrixEvidenceArtifact,
    pub workspace_key: &'a str,
    pub owner_host: &'a Path,
    pub root: &'a Path,
    pub task: Uuid,
    pub recorded: &'a Value,
    pub validated: &'a Value,
    pub disposition: &'a Value,
    pub matrix_effect: &'a Value,
    pub saved: &'a Value,
    pub ready: &'a Value,
    pub source_head: &'a str,
}

pub(super) async fn prepare_owner_case(input: OwnerCasePipeline<'_>) {
    let matrix: EngineeringMatrixInput =
        serde_json::from_value(input.recorded["input"].clone()).unwrap();
    assert!(matches!(
        &matrix.mode,
        tect_domain::MatrixFact::Known {
            value: EngineeringMode::Mvp,
            ..
        }
    ));
    let mut policy = crate::s03_v4::policy_for_choice(
        input.task,
        &matrix,
        "matrix-local-evidence-first",
        &[
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ],
    );
    for rule in &mut policy.rules {
        rule.allowed_modes = vec![EngineeringMode::Mvp];
    }
    let policy_digest = policy.digest().unwrap();
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(input.runtime_url).unwrap())
        .await
        .unwrap();
    let provider = JevPipelineSavedResponseParser::new(PipelineProviderIdentity {
        provider: "active-jev-s03-no-network".into(),
        model: MODEL.into(),
        destination: "https://synthetic.invalid/no-send".into(),
        wire_version: WIRE_VERSION.into(),
    })
    .unwrap();
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(input.runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(input.keys),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
            runtime_pool,
            input.approval,
        )))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(
            policy.clone(),
        )))
        .with_pipeline_recommendation_provider(Arc::new(provider)),
    );
    let socket = input.root.join("owner-case-pipeline.sock");
    let server = start_server(&socket, service).await;
    let mut owner = Mcp::start(
        &socket,
        input.owner_host,
        &Uuid::new_v4().to_string(),
        input.workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    owner.call("get_state", json!({})).await;
    let ready = input.ready;
    let work = &input.saved["draft"]["nodes"][0];
    let prepared = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        json!({
            "candidate_set_id":input.saved["candidate_set"]["id"],
            "expected_candidate_set_revision":ready["candidate_set"]["revision"],
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":format!("owner-case-pipeline-{}",Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity = id(&prepared["opportunity_id"]);
    let manifest_json: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE opportunity_id=$1",
    )
    .bind(opportunity)
    .fetch_one(input.pool)
    .await
    .unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(manifest_json).unwrap();
    manifest.validate_digest().unwrap();
    assert!(manifest.has_bound_v2_authority() && manifest.should_call());
    assert_eq!(manifest.matrix_task_id, input.task.to_string());
    assert_eq!(manifest.matrix_task_revision, "1");
    assert_eq!(manifest.selected_choice_id, "matrix-local-evidence-first");
    assert_eq!(manifest.matrix_input_digest, input.recorded["input_digest"]);
    assert_eq!(
        manifest.matrix_choice_set_digest,
        input.recorded["choice_set_digest"]
    );
    assert_eq!(
        manifest.matrix_verification_digest,
        input.validated["verification_digest"]
    );
    assert_eq!(manifest.compatibility_policy_digest, policy_digest);
    assert_eq!(
        manifest.mandatory_card_ids,
        ["EM02-PROTECT@0.1", "EM02-SCOPE@0.1"]
    );
    assert_eq!(
        manifest
            .options
            .iter()
            .map(|option| option.kind)
            .collect::<Vec<_>>(),
        [
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ]
    );
    for option in &manifest.options {
        option.verification_plan.validate().unwrap();
        assert!(!option.verification_plan.obligations.is_empty());
        let rule = policy
            .rules
            .iter()
            .find(|rule| rule.kind == option.kind)
            .unwrap();
        let mut covered: Vec<_> = rule
            .card_coverage
            .iter()
            .map(|coverage| coverage.card_id.as_str())
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, ["EM02-PROTECT@0.1", "EM02-SCOPE@0.1"]);
        for coverage in &rule.card_coverage {
            assert!(
                option
                    .verification_plan
                    .obligations
                    .iter()
                    .any(|obligation| obligation.phase_id == coverage.phase_id)
            );
        }
    }
    let refs: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT b.evidence_ref FROM matrix_verification_bindings b \
         JOIN matrix_verifications v ON (v.tenant_id,v.workspace_id,v.id)= \
             (b.tenant_id,b.workspace_id,b.verification_id) \
         WHERE v.task_id=$1 AND v.task_revision=1 AND v.record_digest=$2 \
           AND v.schema='tect.context-matrix-verification/1' ORDER BY b.evidence_ref",
    )
    .bind(input.task)
    .bind(input.validated["verification_digest"].as_str().unwrap())
    .fetch_all(input.pool)
    .await
    .unwrap();
    assert_eq!(refs.len(), 1);
    assert!(refs[0].starts_with("pipeline-evidence:"));
    assert_eq!(manifest.evidence_refs, refs);
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE verifier_request_id=$1",
    )
    .bind(id(&input.matrix_effect["request_id"]))
    .fetch_one(input.pool)
    .await
    .unwrap();
    assert_ne!(matrix_effect_id, Uuid::nil());
    assert_eq!(
        input.disposition["decision"]["selected_choice_id"],
        "matrix-local-evidence-first"
    );
    let dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(opportunity)
            .fetch_one(input.pool)
            .await
            .unwrap();
    assert_eq!(dispatches, 0, "preparation cannot send to JEV");
    println!(
        "s03_owner_case_prepared source_head={} task={} choice=matrix-local-evidence-first \
         matrix_effect={} manifest_digest={} evidence_refs={} options={} mandatory_cards=EM02-SCOPE@0.1,EM02-PROTECT@0.1 dispatches=0",
        input.source_head,
        input.task,
        matrix_effect_id,
        manifest.digest,
        manifest.evidence_refs.len(),
        manifest.options.len(),
    );
    owner.finish().await;
    server.abort();
}
