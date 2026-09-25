//! Synthetic Matrix authority and current native Work save via production APIs; no model runs.
use super::positive_input::matrix_input;
use super::*;
use crate::{PgStore, admin};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tect_application::{
    MatrixPlanningMappedNode, MatrixPlanningSelectionLink, RecordMatrixDisposition, SetupFiles,
    SourceInspector, Store, TransactionMode, WorkspaceService,
};
use tect_domain::{
    CandidateMethodSnapshot, EngineeringCandidate, EngineeringChoiceSet, EvidenceValidationOutcome,
    MATRIX_CHOICE_SET_SCHEMA, MATRIX_VERIFICATION_SCHEMA, MatrixDispositionBasis,
    MatrixDispositionDecision, MatrixEvidenceBinding, MatrixVerificationRecord,
    ModelRouteCallerFacts, OwnerReportedEngineeringMatrixFacts, PipelineCatalogueEntry,
    PipelineCatalogueSnapshot, PipelineExecutionOwner, PipelineKind, SaveSliceCandidateDraft,
    SliceCandidateDraft, SliceCandidateDraftNode, SliceDraftIdentity,
    compose_independently_verified_owner_matrix, evaluate_matrix_verification,
    matrix_verified_disposition_digest, required_matrix_facts,
};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) struct UnusedAdapters;
#[async_trait]
impl SourceInspector for UnusedAdapters {
    async fn inspect(
        &self,
        _: &str,
        _: &[String],
    ) -> tect_domain::Result<tect_domain::SourceLocation> {
        Err(Error::InternalInvariant)
    }
}
impl SetupFiles for UnusedAdapters {
    fn resolve_directory(
        &self,
        _: &str,
        _: &[String],
    ) -> tect_domain::Result<tect_domain::SetupDirectory> {
        Err(Error::InternalInvariant)
    }
    fn inspect(
        &self,
        _: &tect_domain::SetupDirectory,
        _: usize,
    ) -> tect_domain::Result<tect_domain::FileObservation> {
        Err(Error::InternalInvariant)
    }
    fn publish(
        &self,
        _: &tect_domain::SetupDirectory,
        _: &str,
    ) -> tect_domain::Result<tect_domain::FilePublication> {
        Err(Error::InternalInvariant)
    }
}

struct SyntheticEvidence {
    now: i64,
}
#[async_trait]
impl tect_application::MatrixEvidenceValidator for SyntheticEvidence {
    fn policy_version(&self) -> &str {
        "slice05-test-policy/1"
    }
    async fn validate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &tect_domain::RequiredMatrixFact,
        evidence_ref: &str,
        _: i64,
    ) -> tect_domain::Result<MatrixEvidenceBinding> {
        Ok(MatrixEvidenceBinding {
            fact_path: fact.path.clone(),
            value_digest: fact.value_digest.clone(),
            evidence_ref: evidence_ref.into(),
            content_digest: sha(evidence_ref.as_bytes()),
            source: "synthetic-test-source".into(),
            subject: "slice05-matrix-task".into(),
            observed_at: self.now - 1,
            expires_at: self.now + 3600,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
    }

    async fn revalidate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &tect_domain::RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> tect_domain::Result<()> {
        if binding.fact_path == fact.path
            && binding.value_digest == fact.value_digest
            && binding.source == "synthetic-test-source"
            && binding.expires_at > now
        {
            Ok(())
        } else {
            Err(Error::StaleContext)
        }
    }
}

fn pipeline_catalogue() -> PipelineCatalogueSnapshot {
    PipelineCatalogueSnapshot {
        revision: "slice05-test/1".into(),
        digest: "test-catalogue-digest".into(),
        entries: PipelineKind::HISTORICAL_SLICE_RUN_KINDS
            .into_iter()
            .map(|kind| PipelineCatalogueEntry {
                kind,
                description: "Synthetic test pipeline".into(),
                implementation_status: "stub".into(),
                description_status: "provisional".into(),
                refinement_required: true,
                choose_when: "fixture".into(),
                do_not_choose_when: "otherwise".into(),
                expected_result: "saved work".into(),
                executable: false,
                default_delivery_mode: None,
                allowed_delivery_modes: vec![],
                execution_owner: PipelineExecutionOwner::SlicePipelineRun,
            })
            .collect(),
    }
}

pub(super) struct Fixture {
    pub(super) tenant: Uuid,
    pub(super) workspace: Uuid,
    pub(super) owner: admin::Enrollment,
    pub(super) invocation_session: Uuid,
    pub(super) task: Uuid,
    pub(super) selection: MatrixPlanningSelection,
    pub(super) candidate_set: Uuid,
    pub(super) caller_request: Uuid,
    pub(super) work_node: Uuid,
    pub(super) work_revision: i64,
}

pub(super) async fn fixture(admin_pool: &PgPool, runtime_pool: &PgPool) -> Fixture {
    let owner = admin::enroll_host(admin_pool, None, vec![]).await.unwrap();
    let tenant = owner.tenant_id;
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let verifier = Uuid::new_v4();
    let verifier_host = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    let task = Uuid::new_v4();
    let opportunity = Uuid::new_v4();
    let source_set = Uuid::new_v4();
    let source_snapshot = Uuid::new_v4();
    let program = Uuid::new_v4();
    let scope = Uuid::new_v4();
    let candidate_set = Uuid::new_v4();
    let planning_snapshot = Uuid::new_v4();
    let source_candidate = Uuid::new_v4();

    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("route-positive-{workspace}"))
        .execute(admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(owner.principal_id)
        .execute(admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(owner.auth.host_id).bind(workspace)
        .bind(session.to_string()).execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO principals(id,tenant_id,role) VALUES($1,$2,'verifier')")
        .bind(verifier)
        .bind(tenant)
        .execute(admin_pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO hosts(id,tenant_id,principal_id,credential_digest) VALUES($1,$2,$3,$4)",
    )
    .bind(verifier_host)
    .bind(tenant)
    .bind(verifier)
    .bind(sha(verifier_host.to_string().as_bytes()))
    .execute(admin_pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(verifier)
        .execute(admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(verifier_session).bind(tenant).bind(verifier_host).bind(workspace)
        .bind(verifier_session.to_string()).execute(admin_pool).await.unwrap();

    let input = matrix_input();
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: format!("choice-{}", Uuid::new_v4().simple()),
        version: 1,
        task_id: task.to_string(),
        task_revision: "1".into(),
        decision_question: "Which source-authored plan?".into(),
        candidates: vec![EngineeringCandidate {
            candidate_id: "choice-a".into(),
            title: "Plan A".into(),
            approach: "Source-authored plan".into(),
            assumption_fact_ids: vec![],
        }],
    };
    let input_digest =
        tect_application::canonical_matrix_input_digest(&serde_json::to_value(&input).unwrap())
            .unwrap();
    let choice_digest = choice.canonical_digest(&input).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let evidence = SyntheticEvidence { now };
    let mut verification = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: task.to_string(),
        task_revision: "1".into(),
        input_digest: input_digest.clone(),
        owner_principal: owner.principal_id.to_string(),
        verifier_principal: verifier.to_string(),
        policy_version: "slice05-test-policy/1".into(),
        bindings: required_matrix_facts(&input)
            .unwrap()
            .iter()
            .map(|fact| MatrixEvidenceBinding {
                fact_path: fact.path.clone(),
                value_digest: fact.value_digest.clone(),
                evidence_ref: "synthetic-ref".into(),
                content_digest: sha(b"synthetic-ref"),
                source: "synthetic-test-source".into(),
                subject: "slice05-matrix-task".into(),
                observed_at: now - 1,
                expires_at: now + 3600,
                validation_outcome: EvidenceValidationOutcome::Accepted,
            })
            .collect(),
        digest: String::new(),
    };
    verification.digest = verification.canonical_digest().unwrap();
    let validated =
        evaluate_matrix_verification(&task.to_string(), "1", &input, &verification, now).unwrap();
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        task.to_string(),
        "1".into(),
        input.clone(),
    )
    .unwrap();
    let composition = compose_independently_verified_owner_matrix(&reported, &validated).unwrap();
    let evaluation_digest =
        matrix_verified_disposition_digest(&input, &composition, &choice, &validated).unwrap();

    let mut runtime = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *runtime)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO matrix_tasks(tenant_id,workspace_id,id,current_revision) VALUES($1,$2,$3,1)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task)
    .execute(&mut *runtime)
    .await
    .unwrap();
    sqlx::query("INSERT INTO matrix_task_revisions(tenant_id,workspace_id,task_id,revision,request_id,input_schema,canonical_input,input_digest,choice_set_schema,choice_set,choice_set_digest,recorded_by_principal_id,recorded_by_session_id) VALUES($1,$2,$3,1,$4,'tect.engineering-matrix-input/1',$5,$6,$7,$8,$9,$10,$11)")
        .bind(tenant).bind(workspace).bind(task).bind(Uuid::new_v4())
        .bind(serde_json::to_value(&input).unwrap()).bind(&input_digest)
        .bind(MATRIX_CHOICE_SET_SCHEMA).bind(serde_json::to_value(&choice).unwrap())
        .bind(&choice_digest).bind(owner.principal_id).bind(session)
        .execute(&mut *runtime).await.unwrap();
    runtime.commit().await.unwrap();
    let mut runtime = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *runtime)
        .await
        .unwrap();
    let verification_id: Uuid = sqlx::query_scalar("INSERT INTO matrix_verifications(tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES($1,$2,$3,1,$4,'tect.matrix-verification/1',$5,$6,$7,'matrix_facts_verified','slice05-test-policy/1',$8) RETURNING id")
        .bind(tenant).bind(workspace).bind(task).bind(&input_digest).bind(owner.principal_id)
        .bind(verifier).bind(verifier_session).bind(&verification.digest)
        .fetch_one(&mut *runtime).await.unwrap();
    for binding in &verification.bindings {
        sqlx::query("INSERT INTO matrix_verification_bindings(tenant_id,workspace_id,verification_id,fact_path,value_digest,evidence_ref,content_digest,source,subject,observed_at,expires_at,validation_outcome) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'accepted')")
            .bind(tenant).bind(workspace).bind(verification_id).bind(&binding.fact_path)
            .bind(&binding.value_digest).bind(&binding.evidence_ref).bind(&binding.content_digest)
            .bind(&binding.source).bind(&binding.subject).bind(binding.observed_at).bind(binding.expires_at)
            .execute(&mut *runtime).await.unwrap();
    }
    runtime.commit().await.unwrap();

    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,mode,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,'optional',$3,$4)")
        .bind(tenant).bind(workspace).bind(owner.principal_id).bind(session)
        .execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,0,'optional',$3,$4)")
        .bind(tenant).bind(workspace).bind(owner.principal_id).bind(session)
        .execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'matrix_task',$4,$5,$6,'1','engineering_profile','engineering.profile.before_selection',1,$7,$8,0,'use_workspace','skip','slice05-test-policy/1',$9,$10,'no_call','request_skip')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(task).bind(session)
        .bind(owner.principal_id).bind(&choice_digest).bind(&verification.digest)
        .bind(format!("matrix-nocall-{}", Uuid::new_v4())).bind(&evaluation_digest)
        .execute(admin_pool).await.unwrap();

    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind(source_set).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(admin_pool).await.unwrap();
    let source_digest = sha(b"synthetic source body");
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'synthetic source body')")
        .bind(tenant).bind(workspace).bind(&source_digest)
        .execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'test-method','1',$5,'synthetic','[]','1',$5,'[]')")
        .bind(source_snapshot).bind(tenant).bind(workspace).bind(source_set).bind(&source_digest)
        .execute(admin_pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(source_set).bind(source_snapshot)
        .execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,3,$5,$6,1,'finite','Scope','Ship', '[]'::jsonb,'[]'::jsonb,$7,'{}'::jsonb)")
        .bind(scope).bind(tenant).bind(workspace).bind(source_set).bind(source_snapshot)
        .bind(source_candidate).bind(Uuid::new_v4())
        .execute(admin_pool).await.unwrap();
    sqlx::query("INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id,revision,status,input_cursor,latest_input) VALUES($1,$2,$3,$4,1,'draft',0,0)")
        .bind(candidate_set).bind(tenant).bind(workspace).bind(scope)
        .execute(admin_pool).await.unwrap();
    sqlx::query("UPDATE native_scopes SET slice_candidate_set_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(scope).bind(candidate_set)
        .execute(admin_pool).await.unwrap();
    let catalogue = pipeline_catalogue();
    catalogue.validate().unwrap();
    let method = CandidateMethodSnapshot {
        id: "test-method".into(),
        revision: "1".into(),
        digest: "a".repeat(64),
        body: "synthetic".into(),
        origin_refs: vec![],
    };
    sqlx::query("INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue,result_ids) VALUES($1,$2,$3,$4,1,1,3,$5,0,$6,'1',$7,'[]'::jsonb,$8,'{}')")
        .bind(planning_snapshot).bind(tenant).bind(workspace).bind(candidate_set)
        .bind(source_snapshot).bind(serde_json::to_value(&method).unwrap())
        .bind("b".repeat(64)).bind(serde_json::to_value(&catalogue).unwrap())
        .execute(admin_pool).await.unwrap();
    sqlx::query("UPDATE slice_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate_set).bind(planning_snapshot)
        .execute(admin_pool).await.unwrap();

    let store = PgStore::from_pool(runtime_pool.clone());
    let adapters = Arc::new(UnusedAdapters);
    let service = WorkspaceService::new(Arc::new(store.clone()), adapters.clone(), adapters)
        .with_matrix_evidence_validator(Arc::new(evidence));
    let owner_context = tect_domain::RequestContext {
        auth: owner.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("route-positive-{workspace}"),
    };
    let disposition = service
        .record_matrix_disposition(
            &owner_context,
            &RecordMatrixDisposition {
                request_id: Uuid::new_v4(),
                task_id: task,
                expected_task_revision: 1,
                expected_input_digest: input_digest.clone(),
                expected_choice_set_digest: Some(choice_digest.clone()),
                opportunity_id: opportunity,
                basis: MatrixDispositionBasis::NoCall,
                advice_id: None,
                advice_digest: None,
                decision: MatrixDispositionDecision::Selected {
                    selected_choice_id: "choice-a".into(),
                },
            },
        )
        .await
        .unwrap();
    let selection = MatrixPlanningSelection {
        task_id: task,
        task_revision: 1,
        disposition_id: disposition.disposition_id,
        selected_choice_id: "choice-a".into(),
        expected_input_digest: input_digest,
        expected_choice_set_digest: choice_digest,
        expected_verification_digest: verification.digest,
        mapped_draft_node_indices: vec![0],
    };
    let caller_request = Uuid::new_v4();
    let request = SaveSliceCandidateDraft {
        scope_id: scope,
        candidate_set_id: candidate_set,
        revision: 1,
        snapshot_id: planning_snapshot,
        input_cursor: 0,
        request_id: caller_request,
        draft: SliceCandidateDraft {
            coverage_summary: "one selected work".into(),
            nodes: vec![SliceCandidateDraftNode::Work {
                identity: SliceDraftIdentity {
                    local: Some("work".into()),
                    candidate_id: None,
                    revision: None,
                },
                model_route_facts: Some(ModelRouteCallerFacts {
                    role: Some("agent".into()),
                    tool: Some("code".into()),
                    data_class: Some("internal".into()),
                    remaining_budget_units: Some(20),
                    available_latency_ms: Some(100),
                }),
                change_rationale: None,
                title: "Implement".into(),
                outcome: "Ship".into(),
                includes: vec![],
                excludes: vec![],
                dependencies: vec![],
                proof: vec!["test".into()],
                pipeline: PipelineKind::LightweightTddDevelopment,
                pipeline_reason: "fixture".into(),
                why_lightweight_insufficient: None,
                why_further_vertical_split_not_viable: None,
                source_result_ids: vec![],
                source_checkpoint: None,
            }],
            supersessions: vec![],
        },
        consumed_knowledge: None,
        matrix_selection: Some(selection.clone()),
    };
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&owner.auth).await.unwrap();
    tx.set_tenant(tenant).await.unwrap();
    let saved = tx
        .save_slice_candidate_draft(workspace, &request)
        .await
        .unwrap();
    let work = &saved.draft.as_ref().unwrap().nodes[0];
    let work_node = work.id();
    let work_revision = work.revision();
    tx.matrix_planning_selection_store()
        .unwrap()
        .link_matrix_planning_selection(
            workspace,
            &MatrixPlanningSelectionLink {
                selection: selection.clone(),
                evaluation_digest,
                catalogue_version: composition.catalogue_version.into(),
                caller_principal_id: owner.principal_id,
                caller_session_id: session,
                scope_id: scope,
                candidate_set_id: candidate_set,
                caller_request_id: caller_request,
                result_revision: saved.candidate_set.revision,
                mapped_nodes: vec![MatrixPlanningMappedNode {
                    draft_index: 0,
                    node_id: work_node,
                    node_revision: work_revision,
                }],
            },
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Fixture {
        tenant,
        workspace,
        owner,
        invocation_session: session,
        task,
        selection,
        candidate_set,
        caller_request,
        work_node,
        work_revision,
    }
}
