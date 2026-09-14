use crate::*;
use std::collections::BTreeSet;

const MAX_PHASE_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMPACT_ITEMS: usize = 4096;
const MAX_OBLIGATION_RECEIPTS: usize = DK2_MAX_OPERATIONS * 64;

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn texts(values: &[String], max_items: usize, max_bytes: usize) -> bool {
    values.len() <= max_items && values.iter().all(|value| text(value, max_bytes))
}

fn target(value: &KnowledgeImpactTarget) -> bool {
    text(&value.reference, 4096) && text(&value.owner_ref, 1024) && text(&value.effect, 4096)
}

fn impact(values: &[KnowledgeImpactTarget]) -> bool {
    values.len() <= MAX_IMPACT_ITEMS && values.iter().all(target)
}

fn baseline(value: &KnowledgeBaselineManifest) -> bool {
    value.targets.len() <= DK2_MAX_OPERATIONS
        && value.dependencies.len() <= DK2_MAX_OPERATIONS
        && value.identity_matches.len() <= DK2_MAX_OPERATIONS
        && texts(&value.source_availability, DK2_MAX_LIST_ITEMS, 4096)
        && texts(&value.assessment_conflicts, DK2_MAX_LIST_ITEMS, 4096)
        && texts(&value.assessment_gaps, DK2_MAX_LIST_ITEMS, 4096)
        && texts(&value.conflicts, DK2_MAX_LIST_ITEMS, 4096)
        && texts(&value.missing_context, DK2_MAX_LIST_ITEMS, 4096)
        && (value.digest.is_empty() || text(&value.digest, 256))
        && value
            .targets
            .iter()
            .chain(&value.dependencies)
            .all(|guard| {
                !guard.unit_id.is_nil()
                    && guard.revision >= 1
                    && text(&guard.rdf_digest, 256)
                    && text(&guard.unit_iri, 4096)
                    && text(&guard.revision_iri, 4096)
            })
        && value.identity_matches.iter().all(|item| {
            text(&item.client_label, 128)
                && !item.unit_id.is_nil()
                && item.revision >= 1
                && text(&item.basis, 4096)
        })
}

fn evidence(value: &KnowledgeEvidenceManifest) -> bool {
    value.claims.len() <= DK2_MAX_OPERATIONS
        && value.source_pins.len() <= DK2_MAX_LIST_ITEMS
        && texts(&value.unresolved_gaps, DK2_MAX_LIST_ITEMS, 4096)
        && text(&value.source_pin_digest, 256)
        && value.claims.iter().all(|claim| {
            !claim.operation_id.is_nil()
                && text(&claim.claim, DK2_MAX_SOURCE_BYTES)
                && claim.source_indexes.len() <= DK2_MAX_LIST_ITEMS
                && claim
                    .source_indexes
                    .iter()
                    .all(|index| (*index as usize) < value.source_pins.len())
                && claim.source_indexes.iter().collect::<BTreeSet<_>>().len()
                    == claim.source_indexes.len()
                && texts(&claim.assumptions, DK2_MAX_LIST_ITEMS, 4096)
                && texts(&claim.gaps, DK2_MAX_LIST_ITEMS, 4096)
        })
        && value.source_pins.iter().enumerate().all(|(index, pin)| {
            pin.source_index == index as u32
                && text(&pin.digest, 256)
                && text(&pin.evidence_scope, 4096)
                && text(&pin.source_iri, 4096)
                && pin
                    .observed_at
                    .as_deref()
                    .is_none_or(|date| crate::knowledge_time::parse_rfc3339(date).is_some())
        })
}

fn receipts(value: &KnowledgeObligationReceipts) -> bool {
    value.receipts.len() <= MAX_OBLIGATION_RECEIPTS
        && texts(
            &value.unresolved_obligation_ids,
            MAX_OBLIGATION_RECEIPTS,
            256,
        )
        && value.receipts.iter().all(|receipt| {
            !receipt.operation_id.is_nil()
                && text(&receipt.obligation_id, 256)
                && text(&receipt.reason, 4096)
                && text(&receipt.changeset_digest, 256)
                && receipt.method_reads.len() <= DK2_MAX_LIST_ITEMS
                && texts(&receipt.input_digests, DK2_MAX_LIST_ITEMS, 256)
                && receipt.reused_receipt_id.is_none_or(|id| !id.is_nil())
        })
}

fn impact_plan(value: &KnowledgeImpactPlan) -> bool {
    texts(&value.synchronous_changes, MAX_IMPACT_ITEMS, 4096)
        && impact(&value.affected_contexts)
        && impact(&value.derivations)
        && impact(&value.owned_copies)
        && impact(&value.followups)
        && texts(&value.blocking_conflicts, MAX_IMPACT_ITEMS, 4096)
        && (value.digest.is_empty() || text(&value.digest, 256))
}

fn review(value: &KnowledgeReviewReceipt) -> bool {
    texts(&value.reviewed_digests, DK2_MAX_LIST_ITEMS, 256)
        && value.covered_operation_ids.len() <= DK2_MAX_OPERATIONS
        && texts(&value.covered_obligation_ids, MAX_OBLIGATION_RECEIPTS, 256)
        && value.findings.len() <= DK2_MAX_LIST_ITEMS
        && value
            .findings
            .iter()
            .map(|finding| &finding.id)
            .collect::<BTreeSet<_>>()
            .len()
            == value.findings.len()
        && text(&value.summary, DK2_MAX_SOURCE_BYTES)
        && value.findings.iter().all(|finding| {
            text(&finding.id, 256)
                && text(&finding.summary, 4096)
                && text(&finding.owner_ref, 1024)
                && finding
                    .closure_output_digest
                    .as_deref()
                    .is_none_or(|digest| text(digest, 256))
                && (finding.closed == finding.closure_output_digest.is_some())
        })
}

impl KnowledgeAgentPhaseOutputDraft {
    pub(crate) fn validate_bounded_with_binding_provenance(&self, resolved: bool) -> Result<()> {
        if serde_json::to_vec(self)
            .map_err(|_| Error::InvalidArguments)?
            .len()
            > MAX_PHASE_OUTPUT_BYTES
            || self.consumed_outputs.len() > DK2_MAX_LIST_ITEMS
            || self.consumed_inputs.len() > DK2_MAX_LIST_ITEMS
            || self.baseline_guards.len() > DK2_MAX_OPERATIONS * 2
            || !texts(&self.source_digests, DK2_MAX_LIST_ITEMS, 256)
            || self.method_reads.len() > DK2_MAX_LIST_ITEMS
            || self.findings.len() > DK2_MAX_LIST_ITEMS
            || !texts(&self.dispositions, DK2_MAX_LIST_ITEMS, 4096)
            || self
                .phasewise_reason
                .as_deref()
                .is_some_and(|reason| !text(reason, 4096))
            || self.plan_revision < 0
            || (!self.plan_digest.is_empty() && !text(&self.plan_digest, 256))
        {
            return Err(Error::CapacityExceeded);
        }
        let valid = match &self.data {
            KnowledgeAgentPhaseData::KcIntake(value) => {
                text(&value.bounded_outcome, DK2_MAX_SOURCE_BYTES)
                    && value.operation_hints.len() <= DK2_MAX_OPERATIONS
                    && text(&value.authority_boundary, 4096)
            }
            KnowledgeAgentPhaseData::KcResolveBaseline(value) => baseline(value),
            KnowledgeAgentPhaseData::KcQualifyPlan(value) => value.validate().is_ok(),
            KnowledgeAgentPhaseData::KcQualifyEvidence(value) => evidence(value),
            KnowledgeAgentPhaseData::KcPrepareChange(value) => {
                if resolved {
                    value.validate().is_ok()
                } else {
                    value.validate_before_binding_resolution().is_ok()
                }
            }
            KnowledgeAgentPhaseData::KcDomainChecks(value) => receipts(value),
            KnowledgeAgentPhaseData::KcImpactPlan(value) => impact_plan(value),
            KnowledgeAgentPhaseData::KcReviewReconcile(value) => review(value),
            KnowledgeAgentPhaseData::KcResultHandoff(value) => {
                text(&value.summary, DK2_MAX_SOURCE_BYTES)
                    && texts(&value.remaining_work, DK2_MAX_LIST_ITEMS, 4096)
                    && value.effects.len() <= DK2_MAX_LIST_ITEMS
            }
        };
        valid.then_some(()).ok_or(Error::InvalidArguments)
    }
}
