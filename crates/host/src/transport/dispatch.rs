async fn execute(request: WireRequest, service: &WorkspaceService) -> WireResponse {
    if let Err(error) = validate_wire_version(&request) {
        return WireResponse::Error { error };
    }
    if request.output_capacity > MAX_FRAME_BYTES {
        return authenticate_invalid_request(service, &request.context, &request.tool_name).await;
    }
    let invocation = match parse_invocation(&request.tool_name, request.arguments) {
        Ok(invocation) => invocation,
        Err(_) => {
            return authenticate_invalid_request(service, &request.context, &request.tool_name)
                .await;
        }
    };

    let result = timeout(OPERATION_TIMEOUT, async {
        let context = &request.context;
        let capacity = request.output_capacity;
        match invocation {
            Invocation::OpenWorkspace => service
                .open_workspace(&request.context)
                .await
                .and_then(|state| program_output::workspace(state, request.output_capacity)),
            Invocation::GetState => crate::slice_dispatch::state(&request.context, service)
                .await
                .and_then(|state| program_output::workspace(state, request.output_capacity)),
            Invocation::Program(invocation) => {
                execute_program(
                    &request.context,
                    invocation,
                    service,
                    request.output_capacity,
                )
                .await
            }
            Invocation::Setup(invocation) => {
                crate::setup_dispatch::execute(
                    &request.context,
                    invocation,
                    service,
                    request.output_capacity,
                )
                .await
            }
            Invocation::ScopeCandidate(invocation) => {
                crate::scope_candidate_dispatch::execute(
                    &request.context,
                    invocation,
                    service,
                    request.output_capacity,
                )
                .await
            }
            Invocation::Slice(invocation) => {
                crate::slice_dispatch::execute(
                    &request.context,
                    invocation,
                    service,
                    request.output_capacity,
                )
                .await
            }
            Invocation::Pipeline(invocation) => {
                crate::pipeline_dispatch::execute(
                    &request.context,
                    invocation,
                    service,
                    request.output_capacity,
                )
                .await
            }
            Invocation::Knowledge(invocation) => {
                crate::knowledge_dispatch::execute(context, invocation, service, capacity).await
            }
            Invocation::KnowledgeLifecycle(invocation) => {
                crate::knowledge_lifecycle_dispatch::execute(context, invocation, service, capacity)
                    .await
            }
            Invocation::KnowledgeSearch(query) => {
                crate::knowledge_search_dispatch::execute(context, query, service, capacity).await
            }
            Invocation::MatrixTask(invocation) => {
                let revision = match invocation {
                    crate::matrix_task_tools::MatrixTaskInvocation::Record(request) => {
                        crate::matrix_task_tools::guard_record_output(&request, capacity)?;
                        service.record_matrix_task(context, &request).await?
                    }
                    crate::matrix_task_tools::MatrixTaskInvocation::Get(task_id) => {
                        service.get_matrix_task(context, task_id).await?
                    }
                };
                Ok(responses::with_actions(
                    crate::matrix_task_tools::revision(revision),
                    Vec::new(),
                    None,
                ))
            }
            Invocation::MatrixVerification(request) => {
                service
                    .authenticate_matrix_verifier_session(context)
                    .await?;
                let receipt =
                    crate::matrix_verification_tools::guarded_verify(&request, capacity, || {
                        service.verify_matrix_task(context, &request)
                    })
                    .await?;
                Ok(responses::with_actions(
                    crate::matrix_verification_tools::receipt(receipt),
                    Vec::new(),
                    None,
                ))
            }
            Invocation::AntiBloat(invocation) => {
                use crate::anti_bloat_tools::AntiBloatInvocation;
                let value = match invocation {
                    AntiBloatInvocation::Prepare { candidate_set_id, expected_revision, preference } =>
                        crate::anti_bloat_tools::review(service.prepare_anti_bloat(
                            context, candidate_set_id, expected_revision, preference,
                        ).await?),
                    AntiBloatInvocation::Run { review_id } => serde_json::json!({
                        "review_id": review_id,
                        "state": crate::anti_bloat_tools::state(&service.run_anti_bloat_once(context, review_id).await?),
                    }),
                    AntiBloatInvocation::Get { review_id } =>
                        crate::anti_bloat_tools::review(service.get_anti_bloat(context, review_id).await?),
                    AntiBloatInvocation::Apply(authored) => serde_json::to_value(
                        service.apply_anti_bloat(context, &authored).await?,
                    ).map_err(Error::invalid_arguments_from)?,
                    AntiBloatInvocation::PreservationGet { review_id } => {
                        let (material, evidence_digest) = service
                            .get_anti_bloat_verification_material(context, review_id).await?;
                        let (verdict, reason) = material.verdict();
                        serde_json::json!({
                            "review_id": review_id,
                            "evidence_digest": evidence_digest,
                            "verdict": verdict,
                            "reason": reason,
                            "material": material,
                        })
                    },
                    AntiBloatInvocation::PreservationVerify(request) => serde_json::to_value(
                        service.verify_anti_bloat_apply(context, &request).await?,
                    ).map_err(Error::invalid_arguments_from)?,
                };
                let response = responses::with_actions(value, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity { return Err(Error::RequestTooLarge); }
                Ok(response)
            }
            Invocation::ModelRoute(invocation) => {
                use crate::model_route_tools::ModelRouteInvocation;
                let value = match invocation {
                    ModelRouteInvocation::Prepare(request) => serde_json::to_value(
                        service.prepare_model_route(context, &request).await?
                    ).map_err(Error::invalid_arguments_from)?,
                    ModelRouteInvocation::Run { preparation_request_key } => serde_json::to_value(
                        service.run_model_route(context, &preparation_request_key).await?
                    ).map_err(Error::invalid_arguments_from)?,
                    ModelRouteInvocation::Get { preparation_request_key } => serde_json::to_value(
                        service.get_model_route(context, &preparation_request_key).await?
                    ).map_err(Error::invalid_arguments_from)?,
                    ModelRouteInvocation::Disposition { disposition_id, decision_id, action, rationale } => serde_json::to_value(
                        service.disposition_model_route(context, decision_id, disposition_id, action, rationale).await?
                    ).map_err(Error::invalid_arguments_from)?,
                };
                let response = responses::with_actions(value, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity { return Err(Error::RequestTooLarge); }
                Ok(response)
            }
            Invocation::MatrixPlanningEffect(invocation) => {
                service
                    .authenticate_matrix_verifier_session(context)
                    .await?;
                let output = match invocation {
                    crate::matrix_planning_effect_tools::MatrixPlanningEffectInvocation::Get {
                        candidate_set_id,
                        caller_request_id,
                    } => crate::matrix_planning_effect_tools::read(
                        service
                            .get_matrix_planning_effect(
                                context,
                                candidate_set_id,
                                caller_request_id,
                            )
                            .await?,
                    ),
                    crate::matrix_planning_effect_tools::MatrixPlanningEffectInvocation::Verify(
                        request,
                    ) => {
                        crate::matrix_planning_effect_tools::guard_verify_output(
                            &request, capacity,
                        )?;
                        crate::matrix_planning_effect_tools::receipt(
                            service
                                .verify_matrix_planning_effect(context, &request)
                                .await?,
                        )
                    }
                };
                let response = responses::with_actions(output, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(response)
            }
            Invocation::PipelineOpenEffect(invocation) => {
                service.authenticate_matrix_verifier_session(context).await?;
                let output = match invocation {
                    crate::pipeline_open_effect_tools::PipelineOpenEffectInvocation::Get { slice_id, open_request_id } => {
                        let (material, digest, principal, session) = service.get_pipeline_open_effect(context, slice_id, open_request_id).await?;
                        crate::pipeline_open_effect_tools::read(material, digest, principal, session)
                    }
                    crate::pipeline_open_effect_tools::PipelineOpenEffectInvocation::Verify(request) => {
                        crate::pipeline_open_effect_tools::guard_verify_output(&request, capacity)?;
                        crate::pipeline_open_effect_tools::receipt(service.verify_pipeline_open_effect(context, &request).await?)
                    }
                };
                let response = responses::with_actions(output, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity { return Err(Error::RequestTooLarge); }
                Ok(response)
            }
            Invocation::PipelinePhaseEffect(invocation) => {
                service.authenticate_matrix_verifier_session(context).await?;
                let output = match invocation {
                    crate::pipeline_phase_effect_tools::PipelinePhaseEffectInvocation::Get { run_id, attempt_id } => {
                        let (material, digest, principal, session) = service.get_pipeline_phase_effect(context, run_id, attempt_id).await?;
                        crate::pipeline_phase_effect_tools::read(material, digest, principal, session)
                    }
                    crate::pipeline_phase_effect_tools::PipelinePhaseEffectInvocation::Verify(request) => {
                        crate::pipeline_phase_effect_tools::guard_verify_output(&request, capacity)?;
                        crate::pipeline_phase_effect_tools::receipt(service.verify_pipeline_phase_effect(context, &request).await?)
                    }
                };
                let response = responses::with_actions(output, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity { return Err(Error::RequestTooLarge); }
                Ok(response)
            }
            Invocation::MatrixAdvisory(invocation) => {
                let receipt = match invocation {
                    crate::matrix_advisory_tools::MatrixAdvisoryInvocation::Request(request) => {
                        let opportunity = crate::matrix_advisory_tools::guarded_request(
                            &request,
                            capacity,
                            || service.request_engineering_advisory(context, &request),
                        )
                        .await?;
                        crate::matrix_advisory_tools::receipt(opportunity)
                    }
                    crate::matrix_advisory_tools::MatrixAdvisoryInvocation::Get {
                        task_id,
                        request_key,
                    } => {
                        let read = service
                            .get_engineering_advisory(context, task_id, &request_key)
                            .await?;
                        crate::matrix_advisory_tools::read(read)
                    }
                };
                let response = responses::with_actions(receipt, Vec::new(), None);
                if responses::encoded_len(&response)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(response)
            }
            Invocation::MatrixDisposition(invocation) => {
                let record = match invocation {
                    crate::matrix_disposition_tools::MatrixDispositionInvocation::Record(
                        request,
                    ) => {
                        crate::matrix_disposition_tools::guarded_record(&request, capacity, || {
                            service.record_matrix_disposition(context, &request)
                        })
                        .await?
                    }
                    crate::matrix_disposition_tools::MatrixDispositionInvocation::Get {
                        task_id,
                        request_id,
                    } => service
                        .get_matrix_disposition_by_request(context, task_id, request_id)
                        .await?
                        .ok_or(Error::NotFound)?,
                };
                let response = responses::with_actions(
                    crate::matrix_disposition_tools::receipt(record),
                    Vec::new(),
                    None,
                );
                if responses::encoded_len(&response)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(response)
            }
            Invocation::Advisory(invocation) => {
                crate::advisory_dispatch::execute(context, invocation, service, capacity).await
            }
            Invocation::PipelineRecommendationPrepare(request) => {
                let prepared = service
                    .prepare_pipeline_recommendation(context, &request)
                    .await?;
                let result = crate::pipeline_recommendation_tools::prepare_receipt(&prepared);
                let result = crate::responses::with_actions(result, Vec::new(), None);
                if crate::responses::encoded_len(&result)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(result)
            }
            Invocation::PipelineRecommendationRun(request) => {
                use tect_application::PipelineRecommendationRun;
                let result = match service.run_pipeline_recommendation(context, &request).await? {
                    PipelineRecommendationRun::NoCall { opportunity_id, reason } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"status":"no_call","reason":reason}),
                    PipelineRecommendationRun::Stale { opportunity_id } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"status":"stale"}),
                    PipelineRecommendationRun::SendUnknown { opportunity_id, dispatch_id } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"dispatch_id":dispatch_id,"status":"send_unknown"}),
                    PipelineRecommendationRun::Ranked { opportunity_id, dispatch_id, ranked_ids } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"dispatch_id":dispatch_id,"status":"ranked","ranked_ids":ranked_ids}),
                    PipelineRecommendationRun::Abstained { opportunity_id, dispatch_id } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"dispatch_id":dispatch_id,"status":"abstained"}),
                };
                let result = crate::responses::with_actions(result, Vec::new(), None);
                if crate::responses::encoded_len(&result)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(result)
            }
            Invocation::PipelineRecommendationDisposition(request) => {
                let saved = service.dispose_pipeline_recommendation(context, &request).await?;
                let result = crate::responses::with_actions(
                    serde_json::to_value(saved).map_err(Error::invalid_arguments_from)?,
                    Vec::new(),
                    None,
                );
                if crate::responses::encoded_len(&result)? > capacity {
                    return Err(Error::RequestTooLarge);
                }
                Ok(result)
            }
            Invocation::KnowledgeMaintenance(invocation) => {
                crate::knowledge_maintenance_dispatch::execute(
                    context, invocation, service, capacity,
                )
                .await
            }
            Invocation::Help(help_request) => {
                service.authenticate_host(&request.context).await?;
                Ok(responses::with_actions(
                    crate::api::help(help_request)?,
                    Vec::new(),
                    None,
                ))
            }
            Invocation::RegisterSource { path } => {
                serialize(service.register_source(&request.context, &path).await)
            }
            Invocation::SelectWorktrees { worktree_ids } => service
                .select_worktrees(&request.context, &worktree_ids)
                .await
                .and_then(|state| program_output::workspace(state, request.output_capacity)),
            Invocation::ListSources { after, limit } => {
                serialize(service.list_sources(&request.context, after, limit).await)
            }
        }
    })
    .await;
    match result {
        Ok(Ok(result)) => match responses::encoded_len(&result) {
            Ok(bytes) if bytes <= request.output_capacity => WireResponse::Ok { result },
            Ok(_) => WireResponse::Error {
                error: Error::RequestTooLarge,
            },
            Err(error) => WireResponse::Error { error },
        },
        Ok(Err(error)) => WireResponse::Error { error },
        Err(_) => WireResponse::Error {
            error: Error::TransportUnavailable,
        },
    }
}
