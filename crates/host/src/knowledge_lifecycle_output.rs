use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::*;
use uuid::Uuid;

pub(crate) fn lifecycle(
    value: KnowledgeLifecycleResponse,
    query: &KnowledgeLifecycleQuery,
    capacity: usize,
) -> Result<Value> {
    let actions = match &value {
        KnowledgeLifecycleResponse::Current(context) => context_actions(context)?,
        _ => Vec::new(),
    };
    let full = responses::with_actions(json!(value), actions, Some(0));
    if query.fragment.is_none() && responses::encoded_len(&full)? <= capacity {
        Ok(full)
    } else {
        crate::knowledge_lifecycle_encoding::fragment(full, query, capacity)
    }
}
pub(crate) fn unit(
    value: KnowledgeUnitResponse,
    query: &KnowledgeUnitQuery,
    capacity: usize,
) -> Result<Value> {
    let full = responses::with_actions(json!(value), Vec::new(), None);
    if query.fragment.is_none() && responses::encoded_len(&full)? <= capacity {
        Ok(full)
    } else {
        crate::knowledge_lifecycle_encoding::unit_fragment(full, query, capacity)
    }
}
pub(crate) fn begin(value: BeginKnowledgeChangeOutcome, capacity: usize) -> Result<Value> {
    let (context, outcome, changed) = match &value {
        BeginKnowledgeChangeOutcome::Created(v) => (v, "created", true),
        BeginKnowledgeChangeOutcome::Replay(v) => (v, "replay", false),
    };
    context_result(json!(value), outcome, changed, context, capacity)
}
pub(crate) fn mutation(value: KnowledgeChangeMutationOutcome, capacity: usize) -> Result<Value> {
    let (context, outcome, changed) = match &value {
        KnowledgeChangeMutationOutcome::Advanced(v) => (v, "advanced", true),
        KnowledgeChangeMutationOutcome::Replay(v) => (v, "replay", false),
    };
    context_result(json!(value), outcome, changed, context, capacity)
}

fn context_result(
    full: Value,
    outcome: &str,
    changed: bool,
    context: &KnowledgeChangeContext,
    capacity: usize,
) -> Result<Value> {
    let value = responses::with_actions(full, context_actions(context)?, Some(0));
    if responses::encoded_len(&value)? <= capacity {
        return Ok(value);
    }
    within(
        responses::with_actions(
            json!({"outcome":outcome,"changed":changed,"change_id":context.change_id,"run_id":context.run.id,
                "run_revision":context.run.revision,"status":context.run.status,
                "current_phase_id":context.run.current_phase_id}),
            vec![responses::action(
                "knowledge_lifecycle",
                json!({"change_id":context.change_id,"view":"current",
                    "fragment":{"offset":0,"limit":262144}}),
            )?],
            Some(0),
        ),
        capacity,
    )
}
pub(crate) fn commit(value: CommitKnowledgeChangeOutcome, capacity: usize) -> Result<Value> {
    let (change_id, compact) = match &value {
        CommitKnowledgeChangeOutcome::Applied(v) => (
            v.change_id,
            json!({"outcome":"applied","changed":true,"change_id":v.change_id,"run_id":v.run_id,
            "publisher_receipt_id":v.id,"publisher_receipt_digest":v.digest,"workspace_generation":v.workspace_generation}),
        ),
        CommitKnowledgeChangeOutcome::AppliedErased(v) => (
            v.change_id,
            json!({"outcome":"applied_erased","changed":true,"change_id":v.change_id,"run_id":v.run_id,
            "publisher_receipt_id":v.id}),
        ),
        CommitKnowledgeChangeOutcome::Replay(v) => (
            v.change_id,
            json!({"outcome":"replay","changed":false,"change_id":v.change_id,"run_id":v.run_id,
            "publisher_receipt_id":v.id,"publisher_receipt_digest":v.digest,"workspace_generation":v.workspace_generation}),
        ),
    };
    let actions = vec![responses::action(
        "knowledge_lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )?];
    let full = responses::with_actions(json!(value), actions, Some(0));
    if responses::encoded_len(&full)? <= capacity {
        return Ok(full);
    }
    within(
        responses::with_actions(
            compact,
            vec![responses::action(
                "knowledge_lifecycle",
                json!({"change_id":change_id,
                "view":"current","fragment":{"offset":0,"limit":262144}}),
            )?],
            Some(0),
        ),
        capacity,
    )
}
pub(crate) fn settle(
    change_id: Uuid,
    value: SettleKnowledgeChangeEffectsOutcome,
    capacity: usize,
) -> Result<Value> {
    let (report, outcome, changed) = match &value {
        SettleKnowledgeChangeEffectsOutcome::Settled(v) => (v, "settled", true),
        SettleKnowledgeChangeEffectsOutcome::Replay(v) => (v, "replay", false),
    };
    let action = responses::action(
        "knowledge_lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )?;
    let full = responses::with_actions(
        json!(value),
        vec![action],
        if report.required_complete {
            None
        } else {
            Some(0)
        },
    );
    if responses::encoded_len(&full)? <= capacity {
        return Ok(full);
    }
    within(
        responses::with_actions(
            json!({"outcome":outcome,"changed":changed,"change_id":change_id,
                "publisher_receipt_id":report.publisher_receipt_id,
                "required_complete":report.required_complete,"effect_count":report.effects.len()}),
            vec![responses::action(
                "knowledge_lifecycle",
                json!({"change_id":change_id,
                "view":"current","fragment":{"offset":0,"limit":262144}}),
            )?],
            Some(0),
        ),
        capacity,
    )
}

fn context_actions(context: &KnowledgeChangeContext) -> Result<Vec<Value>> {
    let Some(phase) = context.run.current_phase_id else {
        return Ok(Vec::new());
    };
    if phase == KnowledgeChangePhaseId::KcCommit {
        let Some(seal) = &context.ready_to_commit else {
            return Ok(Vec::new());
        };
        let basis = format!(
            "commit:{}:{}:{}",
            seal.run_revision, seal.seal_id, seal.command_digest
        );
        return Ok(vec![responses::action(
            "knowledge_change_commit",
            json!({
                "request_id":request_id(context.change_id,context.run.id,&basis),"change_id":context.change_id,
                "run_id":context.run.id,"run_revision":seal.run_revision,"seal_id":seal.seal_id,
                "plan_revision":seal.plan_revision,"plan_digest":seal.plan_digest,
                "sealed_command_digest":seal.command_digest
            }),
        )?]);
    }
    if phase == KnowledgeChangePhaseId::KcSettleEffects {
        return settle_action(context);
    }
    let basis = format!(
        "phase:{}:{}:{}",
        phase.as_str(),
        context.run.revision,
        context
            .plan
            .as_ref()
            .map(|plan| plan.digest.as_str())
            .unwrap_or("no-plan")
    );
    let mut params = json!({"request_id":request_id(context.change_id,context.run.id,&basis),
        "change_id":context.change_id,"run_id":context.run.id,
        "run_revision":context.run.revision,"phase_id":phase});
    if phase.agent_authored() {
        params["output"] = machine_output(context, phase);
        let (methods, obligations) = required_contract(context, phase);
        let mut context_input = json!({"required_method_reads":methods,"required_obligations":obligations,"fields":[
            {"path":"arguments.params.output.body","format":"Substantive result bound to the supplied exact machine pins."},
            {"path":"arguments.params.output.data","format":format!("Typed {} semantic data matching the phase schema.",phase.as_str())},
            {"path":"arguments.params.output.verdict","format":"Substantive verdict for this exact phase."},
            {"path":"arguments.params.output.method_reads","format":"Acknowledge only required_method_reads actually consumed from the delivered context."},
            {"path":"arguments.params.output.outcome","format":"completed, waiting_input, or blocked."},
            {"path":"arguments.params.output.transition","format":"continue, complete, block, or escalate as allowed by the current contract."},
            {"path":"arguments.params.output.findings","format":"Typed findings; use an empty list only when there are none."},
            {"path":"arguments.params.output.dispositions","format":"Exact dispositions; use an empty list only when none apply."}
        ]});
        if phase == KnowledgeChangePhaseId::KcResultHandoff
            && let Some(proof) = &context.erased_no_change_proof
        {
            context_input["erased_no_change_proof"] = json!(proof);
            context_input["required_result"] = json!({
                "canonical":"no_change","user_outcome":"achieved",
                "remaining_work":[],"publisher_receipt_id":"omit","effects":[]
            });
        }
        return Ok(vec![crate::api::needs_action(
            "needs_context",
            "knowledge_change_phase_complete",
            params,
            "context_input",
            context_input,
        )?]);
    }
    if phase == KnowledgeChangePhaseId::KcPublicationGate {
        return Ok(vec![responses::action(
            "knowledge_change_phase_complete",
            params,
        )?]);
    }
    Ok(Vec::new())
}

fn settle_action(context: &KnowledgeChangeContext) -> Result<Vec<Value>> {
    let (receipt_id, ids): (Uuid, Vec<Uuid>) = if let Some(receipt) = &context.publisher_receipt {
        (
            receipt.id,
            receipt
                .effects
                .iter()
                .filter(|effect| {
                    matches!(
                        effect.status,
                        KnowledgeEffectStatus::Ready
                            | KnowledgeEffectStatus::Pending
                            | KnowledgeEffectStatus::Failed
                    )
                })
                .map(|effect| effect.effect_id)
                .collect(),
        )
    } else if let Some(receipt) = &context.erased_publisher_receipt {
        (
            receipt.id,
            receipt
                .effects
                .iter()
                .filter(|effect| {
                    matches!(
                        effect.status,
                        KnowledgeEffectStatus::Ready
                            | KnowledgeEffectStatus::Pending
                            | KnowledgeEffectStatus::Failed
                    )
                })
                .map(|effect| effect.effect_id)
                .collect(),
        )
    } else {
        return Ok(Vec::new());
    };
    let basis = format!("settle:{}:{receipt_id}:{ids:?}", context.run.revision);
    let params = json!({
        "request_id":request_id(context.change_id,context.run.id,&basis),"change_id":context.change_id,
        "run_id":context.run.id,"run_revision":context.run.revision,
        "publisher_receipt_id":receipt_id,"effect_ids":ids
    });
    if let Some(checkpoint) = context.effects_report.as_ref().and_then(|report| {
        report.effects.iter().find(|effect| {
            effect.kind == KnowledgeEffectKind::BackupDisposition
                && effect.status == KnowledgeEffectStatus::Pending
        })
    }) {
        return Ok(vec![crate::api::needs_action(
            "needs_context",
            "knowledge_change_settle_effects",
            params,
            "context",
            json!({"required_operator_checkpoint":{
                "effect_id":checkpoint.effect_id,"owner_ref":checkpoint.owner_ref,
                "generation":checkpoint.generation,"status":checkpoint.status,
                "detail":checkpoint.detail
            }}),
        )?]);
    }
    Ok(vec![responses::action(
        "knowledge_change_settle_effects",
        params,
    )?])
}

fn machine_output(context: &KnowledgeChangeContext, phase: KnowledgeChangePhaseId) -> Value {
    let consumed_outputs: Vec<_> = context
        .outputs
        .iter()
        .filter(|output| !output.stale && output.output.phase_id.ordinal() < phase.ordinal())
        .map(|output| {
            json!({"phase_id":output.output.phase_id.as_str(),
            "output_revision":output.revision,"digest":output.digest})
        })
        .collect();
    let consumed_inputs: Vec<_> = context
        .inputs
        .iter()
        .map(|input| {
            json!({
        "input_id":input.id,"sequence":input.sequence,"digest":input.digest})
        })
        .collect();
    let baseline_guards: Vec<_> = context
        .baseline
        .as_ref()
        .into_iter()
        .flat_map(|baseline| baseline.targets.iter().chain(&baseline.dependencies))
        .collect();
    let source_digests: Vec<_> = context
        .outputs
        .iter()
        .filter(|output| !output.stale)
        .find_map(|output| match &output.output.data {
            KnowledgeAgentPhaseData::KcQualifyEvidence(evidence) => Some(
                evidence
                    .source_pins
                    .iter()
                    .map(|pin| pin.digest.clone())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let plan = (phase.ordinal() > 3)
        .then_some(context.plan.as_ref())
        .flatten();
    json!({"phase_id":phase,"expected_run_revision":context.run.revision,
        "plan_revision":plan.map(|plan|plan.revision).unwrap_or(0),
        "plan_digest":plan.map(|plan|plan.digest.as_str()).unwrap_or(""),
        "consumed_outputs":consumed_outputs,"consumed_inputs":consumed_inputs,
        "baseline_guards":baseline_guards,"source_digests":source_digests})
}

fn required_contract(
    context: &KnowledgeChangeContext,
    phase: KnowledgeChangePhaseId,
) -> (Vec<Value>, Vec<Value>) {
    let profiles = context
        .plan
        .as_ref()
        .map(|plan| plan.profiles.as_slice())
        .unwrap_or(&[]);
    let definition = context
        .delivered_phases
        .iter()
        .find(|item| item.id == phase)
        .or_else(|| {
            context
                .definition
                .phases
                .iter()
                .find(|item| item.id == phase)
        });
    let methods = definition
        .into_iter()
        .flat_map(|item| &item.methods)
        .filter(|method| {
            !method.id.starts_with("tect:knowledge-profile:")
                || profiles
                    .iter()
                    .any(|profile| profile.method_id() == method.id)
        })
        .map(|method| {
            json!({"instruction_id":method.id,"version":method.version,
            "digest":method.digest,"origin_refs":method.origin_refs})
        })
        .collect();
    let obligations = context
        .plan
        .as_ref()
        .into_iter()
        .flat_map(|plan| &plan.obligations)
        .filter(|item| !item.pending_qualification && item.phase_id == phase)
        .map(|item| {
            json!({"operation_id":item.operation_id,"profile_id":item.profile_id,
            "profile_version":item.profile_version,"profile_digest":item.profile_digest,
            "obligation_id":item.obligation_id,"requirement":item.requirement})
        })
        .collect();
    (methods, obligations)
}

fn request_id(change: Uuid, run: Uuid, basis: &str) -> Uuid {
    let digest = Sha256::digest(format!("tectd-dk2:{change}:{run}:{basis}"));
    let mut bytes = [0; 16];
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

#[cfg(test)]
#[path = "knowledge_lifecycle_output/tests.rs"]
mod tests;
