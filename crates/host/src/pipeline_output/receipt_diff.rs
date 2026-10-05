//! Complete receipt diagnostics with stateless, bounded recovery and continuations.
use crate::{json_fragment, responses};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{
    Error, PipelineDefinitionDigestPort, PipelineReceiptDiffRead, PipelineReceiptKind,
    PipelineRunContextQuery, PipelineSkillReadReceipt, Result,
};

const READY_RECEIPTS_BYTES: usize = 1024;
struct ReceiptDigest;
impl PipelineDefinitionDigestPort for ReceiptDigest {
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }
}

fn original_field(kind: PipelineReceiptKind) -> &'static str {
    match kind {
        PipelineReceiptKind::Skill => "output.skill_reads",
        PipelineReceiptKind::Resource => "output.resource_reads",
    }
}
fn submitted_value(query: &PipelineRunContextQuery) -> Result<Value> {
    serde_json::to_value(
        query
            .submitted_receipts
            .as_ref()
            .ok_or(Error::InternalInvariant)?,
    )
    .map_err(|_| Error::TransportUnavailable)
}
fn recovery_action(
    mut params: Value,
    submitted: Value,
    kind: PipelineReceiptKind,
) -> Result<Value> {
    if serde_json::to_vec(&submitted)
        .map_err(|_| Error::TransportUnavailable)?
        .len()
        <= READY_RECEIPTS_BYTES
    {
        params["submitted_receipts"] = submitted;
        responses::action("slice_pipeline_context", params)
    } else {
        crate::api::needs_action(
            "needs_context",
            "slice_pipeline_context",
            params,
            "context_input",
            json!({"fields":[{"path":"arguments.params.submitted_receipts",
                "format":format!("Repeat the unchanged original {} array in full; do not trim or replace it. Required on every receipt_diff request.", original_field(kind))}]}),
        )
    }
}

pub(super) fn encode(
    read: &PipelineReceiptDiffRead,
    capacity: usize,
    query: &PipelineRunContextQuery,
    window: json_fragment::Window<'_>,
) -> Result<Value> {
    let submitted = submitted_value(query)?;
    let pins = json!({"run_id":read.run_id,"view":"receipt_diff","phase_id":read.phase_id,
        "definition_digest":read.definition_digest,"receipt_kind":read.receipt_kind,"submitted_digest":read.submitted_digest});
    let source = json!({"run_id":read.run_id,"phase_id":read.phase_id,"definition_version":read.definition_version,
        "definition_digest":read.definition_digest,"receipt_kind":read.receipt_kind,"submitted_digest":read.submitted_digest});
    json_fragment::encode_with_continuation(
        read,
        Vec::new(),
        capacity,
        window,
        source,
        pins,
        |params| recovery_action(params, submitted.clone(), read.receipt_kind),
    )
}

pub(crate) fn is_receipt_failure(error: &Error) -> bool {
    error.refusal().is_some_and(|refusal| {
        matches!(
            refusal.rule.as_deref(),
            Some("WP6-SKILL-READ-01" | "WP6-RESOURCE-READ-01")
        )
    })
}
pub(crate) fn failure_recovery(error: &Error, arguments: &Value) -> Result<Option<Value>> {
    let kind = match error.refusal().and_then(|refusal| refusal.rule) {
        Some(rule) if rule == "WP6-SKILL-READ-01" => PipelineReceiptKind::Skill,
        Some(rule) if rule == "WP6-RESOURCE-READ-01" => PipelineReceiptKind::Resource,
        _ => return Ok(None),
    };
    let Some(run_id) = arguments
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<uuid::Uuid>().ok())
        .filter(|id| !id.is_nil())
    else {
        return Ok(None);
    };
    let Some(phase_id) = arguments
        .get("phase_id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    else {
        return Ok(None);
    };
    let field = match kind {
        PipelineReceiptKind::Skill => "skill_reads",
        PipelineReceiptKind::Resource => "resource_reads",
    };
    let Some(output) = arguments.get("output").and_then(Value::as_object) else {
        return Ok(None);
    };
    let submitted = output.get(field).cloned().unwrap_or_else(|| json!([]));
    let receipts: Vec<PipelineSkillReadReceipt> =
        serde_json::from_value(submitted.clone()).map_err(|_| Error::InternalInvariant)?;
    let submitted_digest =
        tect_domain::pipeline_receipt_multiset_digest(kind, &receipts, &ReceiptDigest)?;
    recovery_action(
        json!({"run_id":run_id,"view":"receipt_diff","phase_id":phase_id,
        "receipt_kind":kind,"submitted_digest":submitted_digest}),
        submitted,
        kind,
    )
    .map(Some)
}
