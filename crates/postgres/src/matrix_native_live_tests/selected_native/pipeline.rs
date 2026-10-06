use super::*;
use tect_application::{
    MatrixPlanningEffectVerdict, PipelineOpenEffectVerdict, PipelineRecommendationRun,
    PreparePipelineRecommendation, RunPipelineRecommendation, VerifyMatrixPlanningEffect,
    VerifyPipelineOpenEffect,
};
include!("pipeline_fixture.rs");

#[path = "pipeline_positive.rs"]
mod positive;

#[path = "pipeline_plan_binding_tests.rs"]
mod plan_binding_tests;

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 18.6 and explicit separate admin/runtime identities"]
async fn selected_no_call_pipeline_open_persists_plan_and_distinct_verifier() {
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
        let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
        let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
        assert_ne!(admin_url, runtime_url);
        let admin_pool = PgPool::connect(&admin_url).await.unwrap();
        admin::migrate(&admin_pool, &role).await.unwrap();
        let version: String = sqlx::query_scalar("SHOW server_version_num")
            .fetch_one(&admin_pool)
            .await
            .unwrap();
        assert_eq!(version, "180006");
        let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();
        let current_user: String = sqlx::query_scalar("SELECT current_user")
            .fetch_one(&runtime_pool)
            .await
            .unwrap();
        assert_eq!(current_user, role);
        let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
        let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
        let owner_context = context(
            &owner.auth,
            &format!("pipeline-selected-{}", Uuid::new_v4()),
        );
        let initial = service(Arc::clone(&store));
        let workspace = initial
            .open_workspace(&owner_context)
            .await
            .unwrap()
            .workspace
            .unwrap()
            .id;
        let (program, source, effective) =
            bound_source(&initial, &admin_pool, &owner, &owner_context, workspace).await;
        let (service, selection, matrix_sends) = matrix::selected(
            store,
            &admin_pool,
            &owner,
            &owner_context,
            workspace,
            &source,
            &effective,
        )
        .await;
        let definitions = definitions();
        let guidance = Guidance(definitions.clone());
        let planning =
            native::planning_with_guidance(&service, &owner_context, program, &guidance).await;
        let save = native::save_request(&planning, selection);
        let saved = service
            .save_slice_candidate_draft(&owner_context, &save, &guidance, &native::Guard)
            .await
            .unwrap();
        let work = saved.draft.as_ref().unwrap().nodes[0].clone();
        assert!(matches!(
            &work,
            SliceCandidateNode::Work {
                pipeline: PipelineKind::DebugRootCause,
                ..
            }
        ));
        let verifier = admin::prepare_verifier_enrollment(&admin_pool, owner.tenant_id, workspace)
            .await
            .unwrap()
            .try_commit()
            .await
            .unwrap();
        assert_ne!(verifier.principal_id, owner.principal_id);
        let verifier_context = context(&verifier.auth, &owner_context.workspace_key);
        service.open_workspace(&verifier_context).await.unwrap();
        let effect = service
            .get_matrix_planning_effect(&verifier_context, save.candidate_set_id, save.request_id)
            .await
            .unwrap();
        service
            .verify_matrix_planning_effect(
                &verifier_context,
                &VerifyMatrixPlanningEffect {
                    request_id: Uuid::new_v4(),
                    candidate_set_id: save.candidate_set_id,
                    caller_request_id: save.request_id,
                    expected_result_revision: saved.candidate_set.revision,
                    expected_effect_digest: effect.effect_digest,
                    verdict: MatrixPlanningEffectVerdict::Matches,
                    summary: "Synthetic fixture independent saved Matrix effect".into(),
                },
            )
            .await
            .unwrap();
        let ready = service
            .review_slice_candidate_set(
                &owner_context,
                &ReviewSliceCandidateSet {
                    scope_id: saved.scope.id,
                    candidate_set_id: saved.candidate_set.id,
                    revision: saved.candidate_set.revision,
                    snapshot_id: saved.snapshot.id,
                    input_cursor: saved.candidate_set.input_cursor,
                    request_id: Uuid::new_v4(),
                    consumed_knowledge: native::knowledge(saved.planning_knowledge.as_ref()),
                    review: SliceCandidateReviewDraft {
                        verdict: SlicePlanReviewVerdict::Ready,
                        summary: "Fixture ready".into(),
                        findings: vec![],
                    },
                },
                &guidance,
                &native::Guard,
            )
            .await
            .unwrap();
        let cards = service
            .compose_matrix_cards(
                &owner_context,
                source.revision.task_id,
                source.revision.revision,
            )
            .await
            .unwrap();
        let callbacks = Arc::new(AtomicUsize::new(0));
        let service = service
            .with_pipeline_recommendation_definitions(Arc::new(definitions.clone()))
            .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(policy(
                &source,
                &cards,
                &definitions,
            ))))
            .with_pipeline_recommendation_provider(Arc::new(NoTransport(Arc::clone(&callbacks))));
        service
            .configure_advisory(
                &owner_context,
                &ConfigureWorkspaceAdvisory {
                    expected_revision: 1,
                    mode: WorkspaceAdvisoryMode::Disabled,
                    provider_profile_ref: None,
                    model_configuration: None,
                },
            )
            .await
            .unwrap();
        let prepare = PreparePipelineRecommendation {
            candidate_set_id: ready.candidate_set.id,
            expected_candidate_set_revision: ready.candidate_set.revision,
            work_node_id: work.id(),
            expected_work_node_revision: work.revision(),
            request_key: format!("pipeline-off-{}", Uuid::new_v4()),
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        };
        let prepared = service
            .prepare_pipeline_recommendation(&owner_context, &prepare)
            .await
            .unwrap();
        assert_eq!(prepared.opportunity.state, AdvisoryOpportunityState::NoCall);
        assert_eq!(
            prepared.opportunity.primary_reason,
            AdvisoryReason::WorkspaceDisabled
        );
        assert_eq!(
            prepared.opportunity.capability,
            AdvisoryCapability::PipelineRecommendation
        );
        assert_eq!(
            prepared.opportunity.decision_point,
            AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        );
        assert_eq!(prepared.opportunity.target_kind, "slice_candidate_node");
        assert_eq!(prepared.opportunity.target_id, Some(work.id()));
        assert!(prepared.manifest.has_bound_v2_authority());
        assert_eq!(prepared.manifest.options.len(), 1);
        assert_eq!(
            prepared.manifest.excluded.len(),
            PipelineKind::CURRENT_SLICE_RUN_KINDS.len() - 1
        );
        let run_advice = RunPipelineRecommendation {
            opportunity_id: prepared.opportunity.id,
        };
        assert_eq!(
            service
                .run_pipeline_recommendation(&owner_context, &run_advice)
                .await
                .unwrap(),
            PipelineRecommendationRun::NoCall {
                opportunity_id: prepared.opportunity.id,
                reason: AdvisoryReason::WorkspaceDisabled,
            }
        );
        let disposition_request = PipelineDispositionRequest {
            request_id: Uuid::new_v4(),
            opportunity_id: prepared.opportunity.id,
            expected_work_revision: work.revision(),
            manifest_digest: prepared.manifest.digest.clone(),
            action: PipelineRecommendationDisposition::UseDeterministicChoice,
            rationale: "Synthetic no-call deterministic fixture".into(),
        };
        let disposition = service
            .dispose_pipeline_recommendation(&owner_context, &disposition_request)
            .await
            .unwrap();
        assert_eq!(disposition.advice, PipelineDispositionAdvice::NoCall);
        let option = &prepared.manifest.options[0];
        assert_eq!(
            disposition.selected_option_id.as_deref(),
            Some(option.id.as_str())
        );
        let open = OpenSlice {
            request_id: Uuid::new_v4(),
            scope_id: ready.scope.id,
            scope_revision: ready.scope.revision,
            candidate_set_id: ready.candidate_set.id,
            candidate_set_revision: ready.candidate_set.revision,
            candidate_snapshot_id: ready.snapshot.id,
            candidate_id: work.id(),
            candidate_revision: work.revision(),
            disposition_id: Some(disposition.id),
        };
        let OpenSliceOutcome::Created(slice) =
            service.slice_open(&owner_context, &open).await.unwrap()
        else {
            panic!("fresh open required");
        };
        slice.validate_verification_plan_binding().unwrap();
        let plan = &option.verification_plan;
        let read = readback(&runtime_pool, owner.tenant_id, workspace).await;
        assert_eq!(
            read["pipeline_advice_contexts"][0]["manifest_payload"],
            serde_json::to_value(&prepared.manifest).unwrap()
        );
        let persisted_slice = &read["native_slices"][0];
        let persisted_disposition = &read["pipeline_advice_dispositions"][0];
        for (column, value) in [
            ("selected_option_id", option.id.as_str()),
            ("verification_plan_id", plan.id.as_str()),
            ("verification_plan_schema", plan.schema.as_str()),
            ("verification_plan_digest", plan.digest.as_str()),
            (
                "verification_plan_source_definition_version",
                plan.source_definition_version.as_str(),
            ),
            (
                "verification_plan_source_definition_digest",
                plan.source_definition_digest.as_str(),
            ),
        ] {
            assert_eq!(persisted_slice[column], json!(value));
        }
        assert_eq!(
            persisted_disposition["selected_option_id"],
            json!(option.id)
        );
        assert_eq!(
            persisted_disposition["verification_plan_id"],
            json!(plan.id)
        );
        assert_eq!(
            persisted_disposition["verification_plan_version"],
            json!(plan.source_definition_version)
        );
        assert_eq!(
            persisted_disposition["verification_plan_digest"],
            json!(plan.digest)
        );
        assert_eq!(
            persisted_disposition["verification_plan_source_definition_digest"],
            json!(plan.source_definition_digest)
        );
        assert_eq!(
            persisted_disposition["disposition_id"],
            json!(disposition.id)
        );
        assert_eq!(
            persisted_disposition["session_id"],
            json!(prepared.opportunity.session_id)
        );
        let (material, digest, verifier_id, verifier_session) = service
            .get_pipeline_open_effect(&verifier_context, slice.id, open.request_id)
            .await
            .unwrap();
        assert_eq!(material.slice, slice);
        assert_eq!(material.open_request, open);
        assert_eq!(material.disposition, disposition);
        assert_eq!(material.manifest_digest, prepared.manifest.digest);
        assert_eq!(verifier_id, verifier.principal_id);
        assert_ne!(verifier_id, material.caller_principal_id);
        assert_ne!(verifier_id, material.matrix_owner_principal_id);
        let verify = VerifyPipelineOpenEffect {
            request_id: Uuid::new_v4(),
            slice_id: slice.id,
            open_request_id: open.request_id,
            expected_effect_digest: digest,
            verdict: PipelineOpenEffectVerdict::Matches,
            summary: "Independent synthetic fixture opened option-plan pair".into(),
        };
        let attestation = service
            .verify_pipeline_open_effect(&verifier_context, &verify)
            .await
            .unwrap();
        assert_eq!(attestation.verifier_principal_id, verifier_id);
        assert_eq!(attestation.verifier_session_id, verifier_session);
        let stable = readback(&runtime_pool, owner.tenant_id, workspace).await;
        for table in [
            "pipeline_advice_contexts",
            "pipeline_advice_dispositions",
            "native_slices",
            "pipeline_open_effect_attestations",
        ] {
            assert_eq!(stable[table].as_array().unwrap().len(), 1);
        }
        assert_eq!(
            stable["pipeline_open_effect_attestations"][0]["effect_digest"],
            json!(verify.expected_effect_digest)
        );
        assert_eq!(
            stable["pipeline_open_effect_attestations"][0]["verifier_principal_id"],
            json!(verifier_id)
        );
        assert_eq!(stable["advisory_dispatch"], json!([]));
        assert_eq!(stable["pipeline_advice_interpretations"], json!([]));
        assert_eq!(stable["slice_pipeline_runs"], json!([]));
        assert!(matches!(
            service
                .get_pipeline_open_effect(&owner_context, slice.id, open.request_id)
                .await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            service
                .verify_pipeline_open_effect(&owner_context, &verify)
                .await,
            Err(Error::Forbidden)
        ));
        assert_eq!(
            service
                .prepare_pipeline_recommendation(&owner_context, &prepare)
                .await
                .unwrap(),
            prepared
        );
        assert_eq!(
            service
                .dispose_pipeline_recommendation(&owner_context, &disposition_request)
                .await
                .unwrap(),
            disposition
        );
        assert_eq!(
            service.slice_open(&owner_context, &open).await.unwrap(),
            OpenSliceOutcome::Replay(slice.clone())
        );
        assert_eq!(
            service
                .verify_pipeline_open_effect(&verifier_context, &verify)
                .await
                .unwrap(),
            attestation
        );
        assert_eq!(
            readback(&runtime_pool, owner.tenant_id, workspace).await,
            stable
        );
        let begin = BeginPipelineRun {
            request_id: Uuid::new_v4(),
            scope_id: slice.scope_id,
            slice_id: slice.id,
            slice_revision: slice.revision,
            delivery_mode: Some(definitions.0.default_mode),
            definition_version: Some(plan.source_definition_version.clone()),
            inquiry: None,
            source_checkpoint: slice.source_checkpoint.clone(),
            qualification_reason: "Pinned fixture plan".into(),
        };
        plan_binding_tests::reject_definition_drift(
            (&admin_pool, &runtime_pool),
            &service,
            &owner_context,
            (owner.tenant_id, workspace),
            &begin,
            &definitions,
        )
        .await;
        let BeginPipelineRunOutcome::Created(begun) = service
            .pipeline_run_begin(&owner_context, &begin, &definitions, &RunGuard)
            .await
            .unwrap()
        else {
            panic!("fresh run");
        };
        assert_eq!(begun.run.definition_digest, plan.source_definition_digest);
        let rows = readback(&runtime_pool, owner.tenant_id, workspace).await;
        assert_eq!(rows["slice_pipeline_runs"].as_array().unwrap().len(), 1);
        for (column, value) in [
            ("selected_option_id", option.id.as_str()),
            ("verification_plan_id", plan.id.as_str()),
            (
                "verification_plan_version",
                plan.source_definition_version.as_str(),
            ),
            ("verification_plan_digest", plan.digest.as_str()),
        ] {
            assert_eq!(rows["slice_pipeline_runs"][0][column], json!(value));
        }
        plan_binding_tests::replay_and_legacy_null(
            (&admin_pool, &runtime_pool),
            &service,
            &owner_context,
            (owner.tenant_id, workspace),
            &begin,
            &begun,
            &definitions,
        )
        .await;
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        assert_eq!(matrix_sends.load(Ordering::SeqCst), 0);
        runtime_pool.close().await;
        admin_pool.close().await;
    })
    .await
    .expect("finite 90s PG fixture deadline");
}
