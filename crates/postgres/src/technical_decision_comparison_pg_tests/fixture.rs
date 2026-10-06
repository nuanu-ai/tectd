async fn read_effect_counts(pool: &PgPool, tenant: Uuid) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1),(SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1),(SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1),(SELECT count(*) FROM matrix_verifications WHERE tenant_id=$1),(SELECT count(*) FROM model_route_advisory_attempts WHERE tenant_id=$1)")
        .bind(tenant).fetch_one(pool).await.unwrap()
}

async fn declaration_lock_released(pool: &PgPool, tenant: Uuid, workspace: Uuid, program: Uuid) {
    let key = format!(
        "matrix-requirements:{tenant}:{workspace}:{}",
        serde_json::to_value(RequirementsAnchor::Program {
            program_id: program
        })
        .unwrap()
    );
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let held:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND classid=((hashtextextended($1,0)>>32)&4294967295)::oid AND objid=(hashtextextended($1,0)&4294967295)::oid AND objsubid=1)")
                .bind(&key).fetch_one(pool).await.unwrap();
            if !held {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("dropped comparison UoW must release declaration lock promptly");
}

pub(super) struct ControlFixture {
    pub admin_pool: PgPool,
    pub runtime: PgPool,
    pub owner: admin::Enrollment,
    pub workspace: Uuid,
    pub program: Uuid,
    pub task: Uuid,
    pub sessions: [Uuid; 3],
    pub owner_context: RequestContext,
    pub compare_context: RequestContext,
    pub locator: MatrixRequirementsLocator,
    pub operating: ApprovedMatrixEvidenceArtifact,
    pub approved: ApprovedTechnicalDecisionEvidence,
    pub technical_body: String,
    pub request: CompareTechnicalDeliveryMechanisms,
    pub delegated: Uuid,
}

pub(super) async fn control_fixture() -> ControlFixture {
    // Explicit fresh cluster pins authorize only synthetic test plumbing.
    // They never supply business evidence or owner approval.
    let (admin_pool, runtime) = isolated_pg::connect_and_migrate().await;
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let task = Uuid::new_v4();
    let workspace_key = format!("technical-control-{workspace}");
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(owner.tenant_id)
        .bind(&workspace_key)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(owner.tenant_id)
        .bind(workspace)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'draft',1,'compose',0,1,65536)").bind(program).bind(owner.tenant_id).bind(workspace).execute(&admin_pool).await.unwrap();
    let verifier = admin::prepare_verifier_enrollment(&admin_pool, owner.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let sessions = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let native_ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for ((session, host), native) in [
        (sessions[0], owner.auth.host_id),
        (sessions[1], owner.auth.host_id),
        (sessions[2], verifier.auth.host_id),
    ]
    .into_iter()
    .zip(native_ids)
    {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(session).bind(owner.tenant_id).bind(host).bind(workspace).bind(native.to_string()).execute(&admin_pool).await.unwrap();
    }
    let owner_context = RequestContext {
        auth: owner.auth.clone(),
        native_session_id: native_ids[0].to_string(),
        workspace_key: workspace_key.clone(),
    };
    let compare_context = RequestContext {
        native_session_id: native_ids[1].to_string(),
        ..owner_context.clone()
    };
    let verifier_context = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: native_ids[2].to_string(),
        workspace_key: workspace_key.clone(),
    };
    let guards = Arc::new(NoExternal);
    let base = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime.clone())),
        guards.clone(),
        guards,
    );
    let locator = MatrixRequirementsLocator::Program {
        program_id: program,
    };
    let proposal = base
        .propose_matrix_requirements_context(
            &owner_context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 0,
                patches: vec![
                    DeclaredRequirementValue::Mode(EngineeringMode::Demo),
                    DeclaredRequirementValue::Intent(EngineeringIntent::Other("demo".into())),
                    DeclaredRequirementValue::Urgency("ordinary".into()),
                    DeclaredRequirementValue::PromisedBehavior("demo".into()),
                    DeclaredRequirementValue::PromisedProof("check".into()),
                    DeclaredRequirementValue::NoDemandCommitment,
                    DeclaredRequirementValue::NoLatencyCommitment,
                ]
                .into_iter()
                .map(|value| RequirementDeclarationPatch::Set { value })
                .collect(),
            },
        )
        .await
        .unwrap();
    base.confirm_matrix_requirements_context(
        &owner_context,
        &ConfirmMatrixRequirementsContext {
            request_id: Uuid::new_v4(),
            locator: locator.clone(),
            proposal_revision: 1,
            proposal_digest: proposal.proposal.digest().into(),
            owner_response_ref: "SYNTHETIC CONTROL DECLARATION CONFIRMATION".into(),
        },
    )
    .await
    .unwrap();
    let mut raw = serde_json::to_value(input::matrix_input()).unwrap();
    for field in [
        "mode",
        "intent",
        "urgency",
        "promised_behavior",
        "promised_proof",
        "demand_commitment",
        "latency_commitment",
    ] {
        raw[field] = json!({"state":"absent"});
    }
    let source = base
        .record_matrix_task_with_requirements(
            &owner_context,
            &RecordMatrixTask {
                task_id: task,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input: serde_json::from_value(raw).unwrap(),
                choice_set: Some(EngineeringChoiceSet {
                    schema: MATRIX_CHOICE_SET_SCHEMA.into(),
                    choice_set_id: format!("control-{task}"),
                    version: 1,
                    task_id: task.to_string(),
                    task_revision: "1".into(),
                    decision_question: "Which delivery mechanism?".into(),
                    candidates: ["reuse", "separate"]
                        .into_iter()
                        .map(|id| EngineeringCandidate {
                            candidate_id: id.into(),
                            title: id.into(),
                            approach: format!("{id} mechanism"),
                            assumption_fact_ids: vec!["criticality".into()],
                        })
                        .collect(),
                }),
            },
            &locator,
        )
        .await
        .unwrap();
    let observed = now() - 1;
    let operating_id = Uuid::new_v4();
    let sourced = |fact: Value| json!({"fact":fact,"source_ref":"SYNTHETIC OPERATING CONTROL OBSERVATION","observed_at":observed,"expires_at":observed+3600});
    let saved = serde_json::to_value(&source.revision.input).unwrap();
    let body=serde_json::to_string(&json!({"schema":"tect.matrix-operating-evidence/1","workspace_id":workspace,"task_id":task,"task_revision":1,
        "scale":sourced(saved["envelope"]["scale"].clone()),"operational_facts":sourced(saved["envelope"]["operational_facts"].clone()),
        "criticality":sourced(saved["criticality"].clone()),"affected_guarantees":sourced(saved["affected_guarantees"].clone()),"actual_exposure":sourced(saved["actual_exposure"].clone()),"urgent_repair":sourced(saved["urgent_repair"].clone()),
        })).unwrap();
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        operating_id,
        &body,
        "application/vnd.tect.matrix-operating-evidence+json;version=1",
    )
    .await;
    let operating = ApprovedMatrixEvidenceArtifact {
        tenant_id: owner.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        artifact_id: operating_id,
        artifact_revision: 1,
        sha256: sha(body.as_bytes()),
        policy_version: "synthetic-control-operating/1".into(),
        max_age_seconds: 3600,
    };
    let effective = base
        .get_effective_matrix_requirements_context(&owner_context, &locator)
        .await
        .unwrap();
    let evidence = required_matrix_operating_facts(&effective, &source.revision.input)
        .unwrap()
        .into_iter()
        .map(|f| MatrixEvidenceReference {
            fact_path: f.path,
            evidence_ref: format!("pipeline-evidence:{operating_id}@1"),
        })
        .collect();
    let verify = service(&runtime, &operating, vec![])
        .verify_matrix_task(
            &verifier_context,
            &VerifyMatrixTask {
                task_id: task,
                expected_revision: 1,
                input_digest: source.revision.input_digest.clone(),
                evidence,
            },
        )
        .await
        .unwrap();
    let VerifiedMatrixTask::Context(verification) = verify else {
        panic!("V2 verification required")
    };
    let binding = ServerTechnicalDecisionTaskBinding {
        tenant_id: owner.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        operating_verification_digest: verification.digest.clone(),
        operating_policy_version: verification.policy_version,
        requirements_binding: source.requirements_binding.clone().unwrap(),
        choice_set: source.revision.choice_set.clone().unwrap(),
        choice_set_digest: source.revision.choice_set_digest.clone().unwrap(),
        recorded_by_principal_id: owner.principal_id,
    };
    // Existing independent verifier principal stands in as the separately
    // delegated technical approver in this synthetic control configuration.
    let delegated = verifier.principal_id;
    assert_ne!(delegated, owner.principal_id);
    let (approved, technical_body) = control_approval(binding, delegated, observed);
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        approved.reference.artifact_id,
        &technical_body,
        TECHNICAL_EVIDENCE_FORMAT,
    )
    .await;
    let request = CompareTechnicalDeliveryMechanisms {
        task_id: task,
        expected_task_revision: 1,
        operating_verification_digest: verification.digest,
        evidence_reference: approved.reference.clone(),
    };
    ControlFixture {
        admin_pool,
        runtime,
        owner,
        workspace,
        program,
        task,
        sessions,
        owner_context,
        compare_context,
        locator,
        operating,
        approved,
        technical_body,
        request,
        delegated,
    }
}

