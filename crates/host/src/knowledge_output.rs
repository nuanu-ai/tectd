use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{
    Error, KnowledgeChange, KnowledgeChangeStage, KnowledgeContext, KnowledgeOperation,
    PrepareKnowledgeChangeOutcome, PublishKnowledgeChangeOutcome, RefreshPipelineKnowledgeOutcome,
    Result, ReviewKnowledgeChangeOutcome,
};
use uuid::Uuid;

pub(crate) fn context(value: KnowledgeContext, capacity: usize) -> Result<Value> {
    let action = crate::api::needs_action(
        "needs_context",
        "knowledge_change_prepare",
        json!({"request_id":Uuid::new_v4(),"operation":"create",
            "expected_generation":value.generation}),
        "context_input",
        json!({"fields":[
            {"path":"arguments.params.draft","format":"Complete source-derived General-DK execution constraint with exact source snapshot and typed binding."},
            {"path":"arguments.params.reason","format":"Nonblank reason for this exact change."},
            {"path":"arguments.params.authority_basis","format":"Nonblank current authority basis; authenticated owner identity remains authoritative."}
        ]}),
    )?;
    within(
        responses::with_actions(json!(value), vec![action], Some(0)),
        capacity,
    )
}

pub(crate) fn change(value: KnowledgeChange, capacity: usize) -> Result<Value> {
    let actions = change_actions(&value)?;
    within(
        responses::with_actions(json!(value), actions, Some(0)),
        capacity,
    )
}

pub(crate) fn prepare(value: PrepareKnowledgeChangeOutcome, capacity: usize) -> Result<Value> {
    let actions = match &value {
        PrepareKnowledgeChangeOutcome::Prepared(change)
        | PrepareKnowledgeChangeOutcome::Replay(change) => change_actions(change)?,
        PrepareKnowledgeChangeOutcome::Duplicate { existing_unit_id } => vec![responses::action(
            "knowledge_context",
            json!({"unit_id":existing_unit_id}),
        )?],
    };
    within(
        responses::with_actions(json!(value), actions, Some(0)),
        capacity,
    )
}

pub(crate) fn review(value: ReviewKnowledgeChangeOutcome, capacity: usize) -> Result<Value> {
    let change = match &value {
        ReviewKnowledgeChangeOutcome::Approved(change)
        | ReviewKnowledgeChangeOutcome::Rejected(change)
        | ReviewKnowledgeChangeOutcome::Replay(change) => change,
    };
    let actions = change_actions(change)?;
    within(
        responses::with_actions(json!(value), actions, Some(0)),
        capacity,
    )
}

pub(crate) fn publish(value: PublishKnowledgeChangeOutcome, capacity: usize) -> Result<Value> {
    let actions = match &value {
        PublishKnowledgeChangeOutcome::Published(receipt)
        | PublishKnowledgeChangeOutcome::Replay(receipt) => vec![responses::action(
            "knowledge_context",
            json!({"unit_id":receipt.unit_id,"revision":receipt.unit_revision}),
        )?],
        PublishKnowledgeChangeOutcome::Duplicate { existing_unit_id } => vec![responses::action(
            "knowledge_context",
            json!({"unit_id":existing_unit_id}),
        )?],
    };
    within(
        responses::with_actions(json!(value), actions, Some(0)),
        capacity,
    )
}

pub(crate) fn refresh(value: RefreshPipelineKnowledgeOutcome, capacity: usize) -> Result<Value> {
    let manifest = match &value {
        RefreshPipelineKnowledgeOutcome::Refreshed(manifest)
        | RefreshPipelineKnowledgeOutcome::Replay(manifest) => manifest,
    };
    within(
        responses::with_actions(
            json!(value),
            vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":manifest.run_id}),
            )?],
            Some(0),
        ),
        capacity,
    )
}

fn change_actions(change: &KnowledgeChange) -> Result<Vec<Value>> {
    match change.stage {
        KnowledgeChangeStage::ReviewRequired => Ok(vec![crate::api::needs_action(
            "needs_context",
            "knowledge_change_review",
            json!({"request_id":request_id(change,"review"),"change_id":change.id,
                "change_revision":change.change_revision,"proposal_digest":change.proposal_digest,
                "method_read":{"id":change.review_method.id,"version":change.review_method.version,
                    "digest":change.review_method.digest}}),
            "context_input",
            json!({"fields":[
                {"path":"arguments.params.verdict","format":"approve or reject after semantic review of the exact proposal and source."},
                {"path":"arguments.params.review_summary","format":"Substantive findings covering source, modality, conditions, exceptions, binding, authority, and contradictions."}
            ]}),
        )?]),
        KnowledgeChangeStage::ReadyToPublish => Ok(vec![responses::action(
            "knowledge_change_publish",
            json!({"request_id":request_id(change,"publish"),"change_id":change.id,
                "change_revision":change.change_revision,"proposal_digest":change.proposal_digest}),
        )?]),
        KnowledgeChangeStage::Rejected => {
            let params = if change.operation == KnowledgeOperation::Create {
                json!({})
            } else {
                json!({"unit_id":change.unit_id})
            };
            Ok(vec![responses::action("knowledge_context", params)?])
        }
        KnowledgeChangeStage::Committed => Ok(vec![responses::action(
            "knowledge_context",
            json!({"unit_id":change.unit_id,"revision":change.proposed_unit_revision}),
        )?]),
    }
}

fn request_id(change: &KnowledgeChange, operation: &str) -> Uuid {
    let digest = Sha256::digest(format!(
        "tectd-knowledge:{}:{}:{operation}",
        change.id, change.change_revision
    ));
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn within<T: Serialize>(value: T, capacity: usize) -> Result<Value> {
    let value = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    if responses::encoded_len(&value)? > capacity {
        Err(Error::RequestTooLarge)
    } else {
        Ok(value)
    }
}
