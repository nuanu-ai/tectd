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
    let operation_timeout = operation_timeout_for(&invocation);

    let capture = diagnostics::capture(&invocation);
    diagnostics::timed(capture, operation_timeout, request.output_capacity, async {
        let context = &request.context;
        let capacity = request.output_capacity;
        match invocation {
            Invocation::OpenWorkspace => service
                .open_workspace_prepared(&request.context, |state| {
                    crate::workspace_output::opened(state, request.output_capacity)
                }).await,
            Invocation::WorkspaceState(query) => {
                crate::workspace_state::execute(context, service, query, capacity).await
            }
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
            Invocation::AntiBloat(invocation) => {
                crate::anti_bloat_tools::dispatch(context, invocation, service, capacity).await
            }
            Invocation::MatrixAdvisory(invocation) => {
                crate::matrix_advisory_tools::dispatch(context, invocation, service, capacity).await
            }
            Invocation::MatrixDisposition(invocation) => {
                crate::matrix_disposition_tools::dispatch(context, invocation, service, capacity)
                    .await
            }
            Invocation::MatrixTask(invocation) => {
                crate::matrix_task_dispatch::task(context, invocation, service, capacity).await
            }
            Invocation::ModelRoute(invocation) => {
                use crate::model_route_tools::ModelRouteInvocation;
                let value = match invocation {
                    ModelRouteInvocation::HostSelection(request) => {
                        let selected = service.prepare_model_route_host_selection(context, &request).await?;
                        return model_route_output(serde_json::json!({"material": selected.material(),
                            "material_json": selected.material_json(), "material_sha256": selected.material_sha256(),
                            "authorization_scope":"current_authenticated_read_only_snapshot"}), capacity);
                    }
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
                model_route_output(value, capacity)
            }
            Invocation::MatrixRequirementsContext(invocation) => {
                crate::matrix_task_dispatch::requirements(context, invocation, service, capacity)
                    .await
            }
            Invocation::MatrixVerification(request) => {
                crate::matrix_task_dispatch::verify(context, request, service, capacity).await
            }
            Invocation::Advisory(invocation) => {
                crate::advisory_dispatch::execute(context, invocation, service, capacity).await
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
                    PipelineRecommendationRun::BudgetExhausted { opportunity_id, dispatch_id } =>
                        serde_json::json!({"opportunity_id":opportunity_id,"dispatch_id":dispatch_id,"status":"budget_exhausted"}),
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
                crate::planning_read::help(help_request, capacity)
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
    .await
}

fn model_route_output(value: Value, capacity: usize) -> Result<Value> {
    let response = responses::with_actions(value, Vec::new(), None);
    if responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(response)
}
