//! Controlled DEV Matrix/Scope seed only. No S02 technical evidence or approval.
use super::*;
pub(super) struct BaseFixture {
    pub(super) admin_pool: PgPool,
    pub(super) runtime: PgPool,
    pub(super) owner: admin::Enrollment,
    pub(super) workspace: Uuid,
    pub(super) program: Uuid,
    pub(super) task: Uuid,
    pub(super) sessions: [Uuid; 3],
    pub(super) owner_context: RequestContext,
    pub(super) compare_context: RequestContext,
    pub(super) operating: crate::ApprovedMatrixEvidenceArtifact,
    pub(super) verification_digest: String,
}
pub(super) async fn base_fixture() -> BaseFixture {
    // Explicit fresh cluster pins authorize only synthetic test plumbing.
    // They never supply business evidence or owner approval.
    let (admin_pool, runtime) = isolated_pg::connect_and_migrate().await;
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let task = Uuid::new_v4();
    let workspace_key = format!("s05-dev-{workspace}");
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
                    DeclaredRequirementValue::Mode(EngineeringMode::Mvp),
                    DeclaredRequirementValue::Intent(EngineeringIntent::Other("Active JEV as optional TectD V2 advisor".into())),
                    DeclaredRequirementValue::Urgency("Finish and verify this sprint; not an emergency production repair".into()),
                    DeclaredRequirementValue::PromisedBehavior("JEV is optional; skip/off prevents provider send; actual JEV calls are durably auditable; advice never auto-applies.".into()),
                    DeclaredRequirementValue::PromisedProof("PG/CI tests plus real JEV outcomes; agent disposition; separate effect verification where selected".into()),
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
            owner_response_ref: "INHERITED S00-S05 OWNER DECLARATIONS UNCHANGED; OPERATOR-ISSUED DEV CONTEXT; NOT TONY CRYPTOGRAPHIC APPROVAL"
                .into(),
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
    let verify = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime.clone())),
        Arc::new(NoExternal),
        Arc::new(NoExternal),
    )
    .with_matrix_evidence_validator(Arc::new(crate::PgMatrixEvidenceValidator::new(
        runtime.clone(),
        operating.clone(),
    )))
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

    BaseFixture {
        admin_pool,
        runtime,
        owner,
        workspace,
        program,
        task,
        sessions,
        owner_context,
        compare_context,
        operating,
        verification_digest: verification.digest,
    }
}
async fn artifact(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    id: Uuid,
    body: &str,
    format: &str,
) {
    sqlx::query("INSERT INTO pipeline_evidence_artifacts(tenant_id,workspace_id,artifact_id,revision,digest,size,format,provenance,target,readiness,body,request_id) VALUES($1,$2,$3,1,$4,$5,$6,'DEV OPERATING SCAFFOLD; NOT MEASURED','test only','ready',$7,$8)")
        .bind(tenant).bind(workspace).bind(id).bind(sha(body.as_bytes())).bind(body.len() as i64)
        .bind(format).bind(body).bind(Uuid::new_v4()).execute(pool).await.unwrap();
}
#[derive(Clone)]
pub(super) struct Guidance(pub SlicePlanningSnapshotMaterial);
impl NativePlanningGuidance for Guidance {
    fn snapshot(
        &self,
        _: &ScopeOpenBasis,
        _: &[SlicePlanningInput],
        _: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        Ok(self.0.clone())
    }
}
impl NativePlanningOutputGuard for Guidance {
    fn check_context(&self, value: &SliceCandidateContext) -> Result<()> {
        assert!(value.candidate_set.revision >= 1);
        Ok(())
    }
    fn check_open_scope(&self, _: &OpenScopeOutcome) -> Result<()> {
        Ok(())
    }
}
fn guidance() -> Guidance {
    Guidance(SlicePlanningSnapshotMaterial {
        method: CandidateMethodSnapshot {
            id: "synthetic-control-method".into(),
            revision: "1".into(),
            digest: sha(b"synthetic method"),
            body: "SYNTHETIC CONTROL METHOD".into(),
            origin_refs: vec![],
        },
        registry_revision: "1".into(),
        registry_digest: sha(b"synthetic rules"),
        rules: vec![],
        catalogue: PipelineCatalogueSnapshot {
            revision: "synthetic-control/1".into(),
            digest: sha(b"synthetic catalogue"),
            entries: PipelineKind::HISTORICAL_SLICE_RUN_KINDS
                .into_iter()
                .map(|kind| PipelineCatalogueEntry {
                    kind,
                    description: "SYNTHETIC CONTROL PIPELINE".into(),
                    implementation_status: "stub".into(),
                    description_status: "provisional".into(),
                    refinement_required: true,
                    choose_when: "test fixture".into(),
                    do_not_choose_when: "business execution".into(),
                    expected_result: "test saved Work".into(),
                    executable: false,
                    default_delivery_mode: None,
                    allowed_delivery_modes: vec![],
                    execution_owner: PipelineExecutionOwner::SlicePipelineRun,
                })
                .collect(),
        },
    })
}

pub(super) async fn save_work(
    c: &BaseFixture,
    service: &WorkspaceService,
    disposition_id: Uuid,
) -> (Uuid, SaveSliceCandidateDraft, Uuid, i64) {
    let tenant = c.owner.tenant_id;
    let workspace = c.workspace;
    let source = service
        .get_matrix_task_source(&c.owner_context, c.task)
        .await
        .unwrap();
    let selection = MatrixPlanningSelection {
        task_id: c.task,
        task_revision: 1,
        disposition_id,
        selected_choice_id: "reuse".into(),
        expected_input_digest: source.revision.input_digest,
        expected_choice_set_digest: source.revision.choice_set_digest.unwrap(),
        expected_verification_digest: c.verification_digest.clone(),
        mapped_draft_node_indices: vec![0],
    };
    let source_set = Uuid::new_v4();
    let source_snapshot = Uuid::new_v4();
    let scope = Uuid::new_v4();
    let candidate_set = Uuid::new_v4();
    let planning = Uuid::new_v4();
    let digest = sha(b"SYNTHETIC SCOPE CONTROL");
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'SYNTHETIC CONTROL','{}',1,'ready','finite',1,1,4096)")
        .bind(source_set).bind(tenant).bind(workspace).bind(c.program).bind(Uuid::new_v4()).execute(&c.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'SYNTHETIC SCOPE CONTROL')").bind(tenant).bind(workspace).bind(&digest).execute(&c.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,1,1,1,$5,'{}',$5,'synthetic','1',$5,'SYNTHETIC','[]','1',$5,'[]')")
        .bind(source_snapshot).bind(tenant).bind(workspace).bind(source_set).bind(&digest).execute(&c.admin_pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(source_set).bind(source_snapshot).execute(&c.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,1,$5,$6,1,'finite','SYNTHETIC CONTROL SCOPE','TEST SAVE','[]','[]',$7,'{}')")
        .bind(scope).bind(tenant).bind(workspace).bind(source_set).bind(source_snapshot).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&c.admin_pool).await.unwrap();
    sqlx::query("INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id,revision,status,input_cursor,latest_input) VALUES($1,$2,$3,$4,1,'draft',0,0)").bind(candidate_set).bind(tenant).bind(workspace).bind(scope).execute(&c.admin_pool).await.unwrap();
    sqlx::query("UPDATE native_scopes SET slice_candidate_set_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(scope).bind(candidate_set).execute(&c.admin_pool).await.unwrap();
    let guidance = guidance();
    let g = &guidance.0;
    g.catalogue.validate().unwrap();
    sqlx::query("INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue,result_ids) VALUES($1,$2,$3,$4,1,1,1,$5,0,$6,$7,$8,$9,$10,'{}')")
        .bind(planning).bind(tenant).bind(workspace).bind(candidate_set).bind(source_snapshot).bind(serde_json::to_value(&g.method).unwrap()).bind(&g.registry_revision).bind(&g.registry_digest).bind(serde_json::to_value(&g.rules).unwrap()).bind(serde_json::to_value(&g.catalogue).unwrap()).execute(&c.admin_pool).await.unwrap();
    sqlx::query("UPDATE slice_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(candidate_set).bind(planning).execute(&c.admin_pool).await.unwrap();
    let save = SaveSliceCandidateDraft {
        scope_id: scope,
        candidate_set_id: candidate_set,
        revision: 1,
        snapshot_id: planning,
        input_cursor: 0,
        request_id: Uuid::new_v4(),
        consumed_knowledge: None,
        matrix_selection: Some(selection),
        draft: SliceCandidateDraft {
            coverage_summary: "SYNTHETIC CONTROL WORK".into(),
            supersessions: vec![],
            nodes: vec![SliceCandidateDraftNode::Work {
                identity: SliceDraftIdentity {
                    local: Some("control-work".into()),
                    candidate_id: None,
                    revision: None,
                },
                change_rationale: None,
                title: "CONTROL".into(),
                outcome: "TEST ONLY".into(),
                includes: vec![],
                excludes: vec![],
                dependencies: vec![],
                proof: vec!["fixture".into()],
                pipeline: PipelineKind::LightweightTddDevelopment,
                pipeline_reason: "synthetic".into(),
                why_lightweight_insufficient: None,
                why_further_vertical_split_not_viable: None,
                source_result_ids: vec![],
                source_checkpoint: None,
                model_route_facts: Some(Box::new(ModelRouteCallerFacts {
                    role: Some("implementation".into()),
                    tool: Some("code".into()),
                    data_class: Some("internal".into()),
                    remaining_budget_units: Some(20),
                    available_latency_ms: Some(100),
                })),
            }],
        },
    };
    let saved = service
        .save_slice_candidate_draft(&c.owner_context, &save, &guidance, &guidance)
        .await
        .unwrap();
    let work = &saved.draft.as_ref().unwrap().nodes[0];
    (candidate_set, save, work.id(), work.revision())
}
