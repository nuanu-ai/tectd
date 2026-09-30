//! Synthetic business controls backed by real authenticated PostgreSQL source.
use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

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
struct Catalogue;
impl ModelRouteCatalogueProvider for Catalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: vec![ModelRoute {
                id: "synthetic-luna6".into(),
                provider: "openai".into(),
                model: "gpt-6-luna".into(),
                effort: "xhigh".into(),
                enabled: true,
                allowed_matrix_choice_ids: vec!["reuse".into()],
                allowed_roles: vec!["implementation".into()],
                allowed_tools: vec!["code".into()],
                allowed_data_classes: vec!["internal".into()],
                required_host_capabilities: vec!["owned-stdio".into()],
                minimum_budget_units: 1,
                minimum_latency_ms: 1,
            }],
        }))
    }
}
struct Capabilities;
impl ModelRouteHostCapabilitiesProvider for Capabilities {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["owned-stdio".into()],
        }
        .fact()
    }
}
struct SyntheticRanker;

/// Local control adapter: no network, scientific inference or genuine advice.
struct SyntheticMatrixAdvice;
#[async_trait]
impl MatrixAdviceProvider for SyntheticMatrixAdvice {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef {
                id: "synthetic-control-ranker".into(),
            },
            model_configuration: AdvisoryModelConfiguration {
                model: "SYNTHETIC CONTROL RANKER".into(),
            },
            destination: "local-test-control".into(),
            wire_version: "synthetic-control/1".into(),
            ranking_policy: MatrixRankingPolicy::StrictV1,
        })
    }
    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let binding = request.binding();
        let MatrixVerificationAuthority::ContextV2 {
            digest,
            snapshot_id,
            authority_schema,
            semantic_digest,
        } = &binding.verification
        else {
            return Err(Error::Forbidden);
        };
        let body = serde_json::to_vec(&json!({
            "control":"SYNTHETIC MATRIX CONTROL ONLY",
            "state":{"contract":"tect.context-matrix-verified-evaluation/1","binding":{
                "verification_digest":digest,"evaluation_digest":binding.evaluation_digest,
                "context":{"schema":"tect.context-matrix-verification/1","frozen_snapshot_id":snapshot_id,"authority_schema":authority_schema,"requirements_semantic_digest":semantic_digest}
            }}
        })).map_err(|_| Error::InvalidArguments)?;
        PreparedMatrixAdviceAttempt::new(request, self.identity().unwrap(), body)
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        let raw = b"SYNTHETIC CONTROL: reuse then separate".to_vec();
        Ok(MatrixProviderResponse {
            binding: prepared.binding().clone(),
            provider_profile_ref: prepared.identity().provider_profile_ref.clone(),
            model_configuration: prepared.identity().model_configuration.clone(),
            response_payload_sha256: sha(&raw),
            raw_response_payload: raw,
            ranking: MatrixRanking::Ranked {
                ranked_candidate_ids: vec!["reuse".into(), "separate".into()],
                recommended_candidate_id: "reuse".into(),
            },
            trial_evidence: None,
            input_tokens: Some(4),
            output_tokens: Some(3),
        })
    }
}
#[async_trait]
impl ModelRouteRankingProvider for SyntheticRanker {
    fn required_profile(&self) -> Option<&str> {
        Some("synthetic-control-ranker")
    }
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::new(ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().unwrap(),
            saved.eligible.as_ref().unwrap(),
            "SYNTHETIC CONTROL RANKER",
        )?)
    }
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        Ok(json!({"schema":MODEL_ROUTE_RANKING_WIRE_SCHEMA,"binding_digest":attempted.request.binding_digest,"adviser_model":"SYNTHETIC CONTROL RANKER","outcome":{"kind":"ranked","route_ids":["synthetic-luna6"]}}).to_string().into_bytes())
    }
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        Ok(ModelRouteProviderObservation {
            raw: self.attempt_prepared(attempted, permit).await?,
            response_complete: None,
            original_transport_context: None,
            http_status: None,
            input_tokens: Some(4),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(1),
        })
    }
}
pub(super) struct WireFixture {
    pub control: ControlFixture,
    pub service: Arc<WorkspaceService>,
    pub candidate_set: Uuid,
    pub selection_request: PrepareModelRouteHostSelection,
    pub save: SaveSliceCandidateDraft,
    pub guidance: Guidance,
}
pub(super) async fn build() -> WireFixture {
    let control = control_fixture().await;
    let c = &control;
    let tenant = c.owner.tenant_id;
    let workspace = c.workspace;
    let key = Ed25519KeyPair::from_seed_unchecked(&[111; 32]).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let keys=crate::BudgetOwnerKeys::from_json(&json!([{"workspace_id":workspace,"owner_id":c.owner.principal_id,"public_key_hex":hex(key.public_key().as_ref())}]).to_string()).unwrap();
    let store = PgStore::from_pool(c.runtime.clone()).with_budget_owner_keys(keys);
    let guards = Arc::new(NoExternal);
    let service = Arc::new(
        WorkspaceService::new(Arc::new(store.clone()), guards.clone(), guards)
            .with_matrix_evidence_validator(Arc::new(crate::PgMatrixEvidenceValidator::new(
                c.runtime.clone(),
                c.operating.clone(),
            )))
            .with_technical_decision_evidence_resolver(Arc::new(
                crate::PgTechnicalDecisionEvidenceResolver::new(
                    c.runtime.clone(),
                    vec![c.approved.clone()],
                ),
            ))
            .with_model_route_catalogue_provider(Arc::new(Catalogue))
            .with_model_route_host_capabilities_provider(Arc::new(Capabilities))
            .with_model_route_ranking_provider(Arc::new(SyntheticRanker))
            .with_matrix_advice_provider(Arc::new(SyntheticMatrixAdvice)),
    );
    service
        .configure_advisory(
            &c.owner_context,
            &ConfigureWorkspaceAdvisory {
                expected_revision: 0,
                mode: WorkspaceAdvisoryMode::Optional,
                provider_profile_ref: Some(AdvisoryProviderProfileRef {
                    id: "synthetic-control-ranker".into(),
                }),
                model_configuration: Some(AdvisoryModelConfiguration {
                    model: "SYNTHETIC CONTROL RANKER".into(),
                }),
            },
        )
        .await
        .unwrap();
    install_control_policy(c, &store, &key).await;
    let opportunity = service
        .request_engineering_advisory(
            &c.owner_context,
            &RequestEngineeringAdvisory {
                task_id: c.task,
                expected_task_revision: 1,
                request_key: format!("control-matrix-{}", Uuid::new_v4()),
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::UseWorkspace,
            },
        )
        .await
        .unwrap();
    assert_eq!(opportunity.state, AdvisoryOpportunityState::Advised);
    let advice = service
        .get_engineering_advisory(
            &c.owner_context,
            c.task,
            &opportunity.workflow_occurrence_key,
        )
        .await
        .unwrap()
        .current_advice
        .unwrap();
    let source = service
        .get_matrix_task_source(&c.owner_context, c.task)
        .await
        .unwrap();
    let disposition = service
        .record_matrix_disposition(
            &c.owner_context,
            &RecordMatrixDisposition {
                request_id: Uuid::new_v4(),
                task_id: c.task,
                expected_task_revision: 1,
                expected_input_digest: source.revision.input_digest.clone(),
                expected_choice_set_digest: source.revision.choice_set_digest.clone(),
                opportunity_id: opportunity.id,
                basis: MatrixDispositionBasis::AfterAdvice,
                advice_id: Some(advice.advice_id),
                advice_digest: Some(advice.advice_digest),
                decision: MatrixDispositionDecision::Selected {
                    selected_choice_id: "reuse".into(),
                },
            },
        )
        .await
        .unwrap();
    let selection = MatrixPlanningSelection {
        task_id: c.task,
        task_revision: 1,
        disposition_id: disposition.disposition_id,
        selected_choice_id: "reuse".into(),
        expected_input_digest: source.revision.input_digest,
        expected_choice_set_digest: source.revision.choice_set_digest.unwrap(),
        expected_verification_digest: c.request.operating_verification_digest.clone(),
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
    let prepared = service
        .prepare_model_route(
            &c.compare_context,
            &PrepareModelRouteRecommendation {
                workspace_id: Uuid::nil(),
                disposition_id: disposition.disposition_id,
                expected_task_id: c.task,
                expected_task_revision: 1,
                expected_candidate_set_id: candidate_set,
                expected_caller_request_id: save.request_id,
                expected_mapped_work_node_id: work.id(),
                expected_mapped_work_node_revision: work.revision(),
                request_key: format!("wire-control-route-{}", Uuid::new_v4()),
                requested_route_id: Some("synthetic-luna6".into()),
                origin_session_id: None,
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::UseWorkspace,
            },
        )
        .await
        .unwrap();
    assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
    let view = service
        .run_model_route(&c.compare_context, &prepared.request_key)
        .await
        .unwrap();
    let decision = view.decision.unwrap();
    assert!(
        matches!(decision.outcome,ModelRouteDecisionOutcome::Recommended{ref route_id} if route_id=="synthetic-luna6")
    );
    let accepted = service
        .disposition_model_route(
            &c.compare_context,
            decision.id,
            Uuid::new_v4(),
            ModelRouteDispositionAction::Accept,
            "SYNTHETIC CONTROL ACCEPTANCE ONLY".into(),
        )
        .await
        .unwrap();
    let eligible = prepared.eligible.as_ref().unwrap();
    let selection_request = PrepareModelRouteHostSelection {
        preparation_request_key: prepared.request_key,
        decision_id: decision.id,
        disposition_id: accepted.id,
        expected_task_id: c.task,
        expected_task_revision: 1,
        expected_work_context_digest: eligible.work_context_digest.clone(),
        expected_catalogue_digest: eligible.catalogue_digest.clone(),
        selected_route_id: "synthetic-luna6".into(),
        input_sha256: sha(PROMPT.as_bytes()),
        invocation_key: format!("wire-owned-control-{}", Uuid::new_v4()),
    };
    WireFixture {
        control,
        service,
        candidate_set,
        selection_request,
        save,
        guidance,
    }
}

async fn install_control_policy(c: &ControlFixture, store: &PgStore, key: &Ed25519KeyPair) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let policy_id = Uuid::new_v4();
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 4,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1,
    };
    let digest =
        AdvisoryBudgetPolicy::digest_for(policy_id, 1, now - 60_000, now + 600_000, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        policy_id,
        1,
        digest.clone(),
        now - 60_000,
        now + 600_000,
        ceilings,
        c.owner.principal_id,
        "0".repeat(128),
    )
    .unwrap();
    let signature = key.sign(&unsigned.approval_signing_message(c.workspace).unwrap());
    let hex = signature
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let policy = AdvisoryBudgetPolicy::new(
        policy_id,
        1,
        digest,
        now - 60_000,
        now + 600_000,
        ceilings,
        c.owner.principal_id,
        hex,
    )
    .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&c.owner.auth).await.unwrap();
    tx.set_tenant(c.owner.tenant_id).await.unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(c.workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
