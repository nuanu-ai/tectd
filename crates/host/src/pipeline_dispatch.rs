use crate::pipeline_definitions::StaticPipelineDefinitions;
use crate::pipeline_tools::PipelineInvocation;
use serde_json::{Value, json};
use tect_application::WorkspaceService;
use tect_domain::{Error, RequestContext, Result};

pub(crate) const MIN_KNOWLEDGE_PAGE_BYTES: usize = 8_192;

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: PipelineInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let boundary = invocation.refusal_boundary();
    let result = match invocation {
        PipelineInvocation::Context(query) => {
            let refresh = query.refresh;
            service
                .pipeline_context(context, &query)
                .await
                .and_then(|value| crate::pipeline_output::context(value, capacity, refresh))
        }
        PipelineInvocation::KnowledgePage(query) => {
            knowledge_page(context, service, &query, capacity).await
        }
        PipelineInvocation::Instruction(query) => service
            .pipeline_instruction(context, &query)
            .await
            .and_then(|value| crate::pipeline_output::instruction(value, capacity)),
        PipelineInvocation::Begin(request) => service
            .pipeline_run_begin(
                context,
                &request,
                &StaticPipelineDefinitions,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::begin(value, capacity)),
        PipelineInvocation::Migrate(request) => service
            .pipeline_run_migrate(context, &request, &StaticPipelineDefinitions)
            .await
            .and_then(|value| {
                serde_json::to_value(value)
                    .map(|value| crate::responses::with_actions(value, Vec::new(), None))
                    .map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::Complete(request) => service
            .pipeline_phase_complete(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::Input(request) => service
            .pipeline_input_record(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::EscalateDelivery(request) => service
            .pipeline_delivery_escalate(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::ResolveCheckpoint(request) => service
            .pipeline_checkpoint_resolve(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::checkpoint_resolution(value, capacity)),
        PipelineInvocation::EvidenceRegister(request) => service
            .pipeline_evidence_artifact_register(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::EvidenceFinalize(request) => service
            .pipeline_evidence_artifact_finalize(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::EvidenceRead(request) => service
            .pipeline_evidence_artifact_read(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
    };
    result.map_err(|error| {
        error.normalize_pipeline_refusal(
            boundary.rule,
            boundary.path,
            boundary.expected,
            boundary.next_action,
            boundary.required,
        )
    })
}

async fn knowledge_page(
    context: &RequestContext,
    service: &WorkspaceService,
    query: &tect_domain::PipelineKnowledgePageQuery,
    capacity: usize,
) -> Result<Value> {
    let requested = query.byte_budget.unwrap_or(131_072);
    if !(MIN_KNOWLEDGE_PAGE_BYTES..=262_144).contains(&requested) {
        return Err(Error::InvalidArguments);
    }
    let limit = requested.min(capacity).min(262_144);
    let minimum = empty_knowledge_page_result(query, requested)?;
    if limit < MIN_KNOWLEDGE_PAGE_BYTES || crate::responses::encoded_len(&minimum)? > limit {
        return Err(Error::RequestTooLarge);
    }
    let mut backend_budget = limit.saturating_sub(2_048).max(768);
    loop {
        let page = service
            .pipeline_knowledge_page(context, query, backend_budget)
            .await?;
        let result = knowledge_page_result(page, query, requested)?;
        if crate::responses::encoded_len(&result)? <= limit {
            return Ok(result);
        }
        if backend_budget <= 768 {
            return Err(Error::RequestTooLarge);
        }
        backend_budget = (backend_budget / 2).max(768);
    }
}

fn empty_knowledge_page_result(
    query: &tect_domain::PipelineKnowledgePageQuery,
    requested: usize,
) -> Result<Value> {
    knowledge_page_result(
        json!({"contract_version":"dk-2-paged","manifest_id":query.manifest_id,
            "manifest_digest":query.digest,"resource_count":0,"start_ordinal":0,
            "start_byte_offset":0,"next_ordinal":0,"next_byte_offset":0,
            "complete":true,"delivered_bytes":0,"resources":[],"next_cursor":null}),
        query,
        requested,
    )
}

fn knowledge_page_result(
    page: Value,
    query: &tect_domain::PipelineKnowledgePageQuery,
    requested: usize,
) -> Result<Value> {
    let actions = if let Some(cursor) = page["next_cursor"].as_str() {
        vec![crate::responses::action(
            "slice_pipeline_knowledge_page",
            json!({"run_id":query.run_id,"manifest_id":query.manifest_id,
                "digest":query.digest,"cursor":cursor,"byte_budget":requested}),
        )?]
    } else {
        Vec::new()
    };
    Ok(crate::responses::with_actions(page, actions, None))
}

#[cfg(test)]
mod knowledge_page_tests {
    use super::*;
    use uuid::Uuid;

    fn query() -> tect_domain::PipelineKnowledgePageQuery {
        tect_domain::PipelineKnowledgePageQuery {
            run_id: Uuid::new_v4(),
            manifest_id: Uuid::new_v4(),
            digest: "a".repeat(64),
            cursor: None,
            byte_budget: Some(4096),
        }
    }

    #[test]
    fn continuation_preserves_manifest_and_requested_budget() {
        let query = query();
        let page = json!({"manifest_id":query.manifest_id,"manifest_digest":query.digest,
            "resources":[],"next_cursor":"position-1","complete":false});
        let result = knowledge_page_result(page, &query, 4096).unwrap();
        let action = &result["actions"][0]["arguments"];
        assert_eq!(action["route"], "slice.pipeline.knowledge_page");
        assert_eq!(action["params"]["run_id"], query.run_id.to_string());
        assert_eq!(
            action["params"]["manifest_id"],
            query.manifest_id.to_string()
        );
        assert_eq!(action["params"]["digest"], query.digest);
        assert_eq!(action["params"]["cursor"], "position-1");
        assert_eq!(action["params"]["byte_budget"], 4096);
        let terminal =
            knowledge_page_result(json!({"next_cursor":null,"complete":true}), &query, 4096)
                .unwrap();
        assert!(terminal["actions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn mcp_encoding_can_exceed_backend_page_json() {
        let query = query();
        let page = json!({"resources":[{"resource":{"canonical_text":"\\\"".repeat(500)}}],
            "next_cursor":"position-1","complete":false});
        let page_bytes = serde_json::to_vec(&page).unwrap().len();
        let result = knowledge_page_result(page, &query, 4096).unwrap();
        assert!(page_bytes < 4096);
        assert!(crate::responses::encoded_len(&result).unwrap() > 4096);
    }

    #[test]
    fn advertised_minimum_fits_an_empty_page_response() {
        let mut query = query();
        query.byte_budget = Some(MIN_KNOWLEDGE_PAGE_BYTES);
        let result = empty_knowledge_page_result(&query, MIN_KNOWLEDGE_PAGE_BYTES).unwrap();
        let encoded = crate::responses::encoded_len(&result).unwrap();
        assert!(
            encoded <= MIN_KNOWLEDGE_PAGE_BYTES / 2,
            "empty page uses {encoded} bytes, leaving too little room for page metadata"
        );
        assert!(
            encoded > 768,
            "regression fixture no longer shows the old mismatch"
        );
    }
}
