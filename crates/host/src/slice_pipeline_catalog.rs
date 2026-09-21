use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::{KnowledgeLifecycleDefinitionProvider, PipelineDefinitionProvider};
use tect_domain::{
    PipelineCatalogueEntry, PipelineCatalogueSnapshot, PipelineExecutionOwner, PipelineKind,
};

pub(crate) const CATALOG_REVISION: &str = "4";

#[derive(Clone, Copy)]
struct SlicePipelineStub {
    kind: PipelineKind,
    description: &'static str,
    choose_when: &'static str,
    do_not_choose_when: &'static str,
    expected_result: &'static str,
    execution_owner: PipelineExecutionOwner,
}

const KNOWN_PIPELINES: [SlicePipelineStub; 10] = [
    SlicePipelineStub {
        kind: PipelineKind::LightweightTddDevelopment,
        description: "A bounded development change with clear expected behavior, a minimally sufficient test cycle, and a verified implementation; this is the default development path.",
        choose_when: "The requested fix, bug resolution, or small vertical feature has a bounded outcome that can be implemented and verified directly.",
        do_not_choose_when: "Evidence shows irreducible design complexity, the work is solely diagnosis or research, or the requested outcome is an operational action or procedure capture.",
        expected_result: "The bounded behavior is implemented and verified against concrete acceptance evidence.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::FullDesignToExecution,
        description: "Clarify substantial interconnected complexity in one indivisible vertical outcome and carry the resulting design through verified implementation.",
        choose_when: "Evidence shows that Lightweight is insufficient and no further sensible vertical decomposition can make the outcome independently deliverable.",
        do_not_choose_when: "The only reasons are size, a new-feature label, cross-component work, uncertainty that can be isolated, or use as a generic fallback.",
        expected_result: "A justified indivisible design is implemented and verified, with the reasons against Lightweight and further vertical division recorded.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::DebugRootCause,
        description: "Establish the demonstrated cause of a behavioral deviation and the boundary and direction of its correction.",
        choose_when: "The cause of observed incorrect behavior is materially unknown and must be proven before selecting corrective work.",
        do_not_choose_when: "The cause is already established or the candidate silently includes implementing the fix.",
        expected_result: "A demonstrated root cause, evidence, affected boundary, and supported correction direction that can inform follow-up work.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::OperationalPreparation,
        description: "Prepare a precise operation plan including prechecks, authority, risks and stop conditions, rollback, and result criteria; the operation remains unperformed.",
        choose_when: "A consequential operation needs a reviewable and bounded plan before execution can be authorized or safely attempted.",
        do_not_choose_when: "The requested outcome is implementation, research, diagnosis, or execution of an already prepared and authorized operation.",
        expected_result: "A complete operation plan with exact target, authority boundary, checks, stops, rollback, and evidence criteria, without performing the operation.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::OperationalExecution,
        description: "Perform an authorized bounded operation against an exact target with evidence of the actual result and recovery when necessary.",
        choose_when: "The operation, target, authority, safety conditions, and success evidence are sufficiently specified for execution.",
        do_not_choose_when: "Authority or the operation plan is unresolved, or the outcome is preparatory planning or product implementation.",
        expected_result: "The authorized operation is executed within its boundary and its actual result and any recovery are evidenced.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::ResearchToDurableKnowledge,
        description: "Close specific research questions with verifiable findings, sources, contradictions, and gaps, producing reusable knowledge without automatic publication.",
        choose_when: "The bounded outcome is a durable answer to explicit research questions rather than a code or operational change.",
        do_not_choose_when: "Research is merely incidental to implementation, or publication and activation have not been separately requested.",
        expected_result: "Traceable reusable knowledge that answers the stated questions and honestly records conflicts and remaining gaps.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::CustomProcedureCapture,
        description: "Capture a repeatable process as a procedure, runbook, or skill candidate with application boundaries and verification.",
        choose_when: "The explicitly requested bounded outcome is a reusable procedure for a recurring process.",
        do_not_choose_when: "Procedure capture was not requested, the process is not sufficiently repeatable, or this would automatically create or activate a skill after ordinary work.",
        expected_result: "A verified reusable procedure candidate with clear applicability and limits, without automatic activation.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::PromoteToDurableKnowledge,
        description: "Publish or change reusable durable knowledge from available evidence through one qualified Knowledge Change.",
        choose_when: "The explicitly requested bounded outcome is durable creation, revision, revalidation, replacement, withdrawal or erasure of known material.",
        do_not_choose_when: "The main unanswered work is new research, brainstorming, product implementation or operational execution, or durable publication is outside current task authority.",
        expected_result: "The exact durable outcome and required delivery/impact/erasure effects are evidenced by the Knowledge Change result and backend receipts.",
        execution_owner: PipelineExecutionOwner::KnowledgeChange,
    },
    SlicePipelineStub {
        kind: PipelineKind::Research,
        description: "Answer explicit research questions with traceable evidence, qualified sources, contradictions, bounded negative findings and a usable conclusion.",
        choose_when: "The outcome requires substantial evidence collection and synthesis rather than a small incidental lookup, implementation, diagnosis or choosing among primarily value-dependent alternatives.",
        do_not_choose_when: "The task is a simple lookup, an unknown behavioral failure, a product change, an operational action, or only durable publication of already known material.",
        expected_result: "A verified operational research result: answered, bounded negative_result, or explicitly contract-permitted inconclusive, with limitations and optional separate publication handoff.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
    SlicePipelineStub {
        kind: PipelineKind::DeepBrainstorming,
        description: "Frame a consequential decision, explore viable alternatives, test assumptions and reconcile trade-offs into an explicit decision or recommendation.",
        choose_when: "The main uncertainty concerns intent, alternatives, criteria or trade-offs and needs sustained exploration before execution or publication.",
        do_not_choose_when: "A brief clarification is enough, evidence gathering is the main outcome, or a ready design must now be implemented.",
        expected_result: "A traceable selected, recommended or rejected decision disposition with rationale, unresolved conditions and next work; pending decisions remain unfinished when a decision was requested.",
        execution_owner: PipelineExecutionOwner::SlicePipelineRun,
    },
];

#[cfg(test)]
const HISTORICAL_CATALOGUE_KINDS: [PipelineKind; 7] = PipelineKind::HISTORICAL_SLICE_RUN_KINDS;
#[cfg(test)]
const HISTORICAL_PROMOTION_CATALOGUE_KINDS: [PipelineKind; 8] = [
    PipelineKind::LightweightTddDevelopment,
    PipelineKind::FullDesignToExecution,
    PipelineKind::DebugRootCause,
    PipelineKind::OperationalPreparation,
    PipelineKind::OperationalExecution,
    PipelineKind::ResearchToDurableKnowledge,
    PipelineKind::CustomProcedureCapture,
    PipelineKind::PromoteToDurableKnowledge,
];
const CURRENT_CATALOGUE_KINDS: [PipelineKind; 9] = [
    PipelineKind::LightweightTddDevelopment,
    PipelineKind::FullDesignToExecution,
    PipelineKind::DebugRootCause,
    PipelineKind::OperationalPreparation,
    PipelineKind::OperationalExecution,
    PipelineKind::Research,
    PipelineKind::DeepBrainstorming,
    PipelineKind::CustomProcedureCapture,
    PipelineKind::PromoteToDurableKnowledge,
];

pub(crate) fn snapshot() -> PipelineCatalogueSnapshot {
    build_snapshot(CATALOG_REVISION, &CURRENT_CATALOGUE_KINDS)
}

fn build_snapshot(revision: &str, kinds: &[PipelineKind]) -> PipelineCatalogueSnapshot {
    let entries = kinds
        .iter()
        .map(|kind| {
            let pipeline = KNOWN_PIPELINES
                .iter()
                .find(|pipeline| pipeline.kind == *kind)
                .expect("catalogue kind has a known description");
            let modes = if pipeline.execution_owner == PipelineExecutionOwner::KnowledgeChange {
                Some((
                    tect_domain::PipelineDeliveryMode::Whole,
                    vec![
                        tect_domain::PipelineDeliveryMode::Whole,
                        tect_domain::PipelineDeliveryMode::Phasewise,
                    ],
                ))
            } else {
                crate::pipeline_definitions::delivery_modes_v07(pipeline.kind)
                    .or_else(|| crate::pipeline_definitions::delivery_modes(pipeline.kind))
            };
            let executable = true;
            PipelineCatalogueEntry {
                kind: pipeline.kind,
                description: pipeline.description.into(),
                implementation_status: if executable { "executable" } else { "stub" }.into(),
                description_status: if executable { "refined" } else { "provisional" }.into(),
                refinement_required: !executable,
                choose_when: pipeline.choose_when.into(),
                do_not_choose_when: pipeline.do_not_choose_when.into(),
                expected_result: pipeline.expected_result.into(),
                executable,
                default_delivery_mode: modes.as_ref().map(|(default, _)| *default),
                allowed_delivery_modes: modes.map_or_else(Vec::new, |(_, allowed)| allowed),
                execution_owner: pipeline.execution_owner,
            }
        })
        .collect::<Vec<_>>();
    let digest = digest_entries(revision, &entries);
    PipelineCatalogueSnapshot {
        revision: revision.into(),
        digest,
        entries,
    }
}

pub(crate) fn value() -> Value {
    let snapshot = snapshot();
    let executable_count = snapshot
        .entries
        .iter()
        .filter(|entry| entry.executable)
        .count();
    let definition = crate::knowledge_lifecycle_definitions::StaticKnowledgeLifecycleDefinitions
        .definition()
        .expect("static Knowledge Change definition validates");
    let promotion_body = include_str!("../knowledge-methods/promotion-slice.md");
    let knowledge_change_phase_count = definition.phases.len();
    let slice_run_phase_count: usize = PipelineKind::CURRENT_SLICE_RUN_KINDS
        .into_iter()
        .map(|kind| {
            crate::pipeline_definitions::StaticPipelineDefinitions
                .definition(kind)
                .expect("static Slice pipeline definition validates")
                .phases
                .len()
        })
        .sum();
    json!({
        "revision": snapshot.revision,
        "digest": snapshot.digest,
        "implementation_status": if executable_count == snapshot.entries.len() { "executable" } else { "partial" },
        "description_status": if executable_count == snapshot.entries.len() { "refined" } else { "partial" },
        "refinement_required": executable_count != snapshot.entries.len(),
        "executable": executable_count > 0,
        "executable_count": executable_count,
        "pipelines": snapshot.entries,
        "knowledge_change_entry":{"route":"knowledge.change_begin","context_route":"knowledge.lifecycle","definition":definition},
        "promotion_method":{"version":"0.2.0-dk2.1","digest":hex(&Sha256::digest(promotion_body.as_bytes())),
            "source_ref":"crates/host/knowledge-methods/promotion-slice.md","body":promotion_body},
        "phase_counts":{"slice_pipeline_run_phases":slice_run_phase_count,
            "knowledge_change_phases":knowledge_change_phase_count},
    })
}

fn digest_entries(revision: &str, entries: &[PipelineCatalogueEntry]) -> String {
    let bytes = serde_json::to_vec(&(revision, entries)).expect("static catalog serializes");
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalogue_contains_nine_current_entries_with_one_knowledge_owner() {
        let snapshot = snapshot();
        let ids = snapshot
            .entries
            .iter()
            .map(|pipeline| pipeline.kind.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 9);
        assert!(!ids.contains("slice.hybrid-implementation-operation"));
        assert!(!ids.contains("slice.research-to-durable-kb"));
        assert!(!ids.contains("slice.research-to-durable-knowledge"));
        assert!(ids.contains("slice.research"));
        assert!(ids.contains("slice.deep-brainstorming"));
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|pipeline| pipeline.executable)
                .count(),
            9
        );
        let lightweight = snapshot
            .entries
            .iter()
            .find(|pipeline| pipeline.kind == PipelineKind::LightweightTddDevelopment)
            .unwrap();
        assert_eq!(lightweight.implementation_status, "executable");
        assert_eq!(lightweight.description_status, "refined");
        assert!(!lightweight.refinement_required);
        assert!(lightweight.default_delivery_mode.is_some());
        assert_eq!(lightweight.allowed_delivery_modes.len(), 2);
        assert!(snapshot.entries.iter().all(|pipeline| {
            pipeline.executable
                && pipeline.implementation_status == "executable"
                && pipeline.description_status == "refined"
                && !pipeline.refinement_required
        }));
        let catalog = value();
        assert_eq!(catalog["executable"], true);
        assert_eq!(catalog["executable_count"], 9);
        assert_eq!(catalog["implementation_status"], "executable");
        assert_eq!(catalog["description_status"], "refined");
        assert_eq!(catalog["refinement_required"], false);
        assert!(catalog.get("stages").is_none());
        let promotion = snapshot
            .entries
            .iter()
            .find(|entry| entry.kind == PipelineKind::PromoteToDurableKnowledge)
            .unwrap();
        assert_eq!(
            promotion.execution_owner,
            PipelineExecutionOwner::KnowledgeChange
        );
        assert_eq!(
            catalog["knowledge_change_entry"]["route"],
            "knowledge.change_begin"
        );
        assert_eq!(
            catalog["knowledge_change_entry"]["definition"]["phases"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        assert_eq!(catalog["phase_counts"]["slice_pipeline_run_phases"], 127);
        assert_eq!(catalog["phase_counts"]["knowledge_change_phases"], 12);
        assert_eq!(
            catalog["promotion_method"]["source_ref"],
            "crates/host/knowledge-methods/promotion-slice.md"
        );
        assert_eq!(
            catalog["promotion_method"]["digest"],
            hex(&Sha256::digest(
                catalog["promotion_method"]["body"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            ))
        );
        let research = snapshot
            .entries
            .iter()
            .find(|entry| entry.kind == PipelineKind::Research)
            .unwrap();
        assert_eq!(
            research.description,
            "Answer explicit research questions with traceable evidence, qualified sources, contradictions, bounded negative findings and a usable conclusion."
        );
        assert_eq!(
            research.choose_when,
            "The outcome requires substantial evidence collection and synthesis rather than a small incidental lookup, implementation, diagnosis or choosing among primarily value-dependent alternatives."
        );
        assert_eq!(
            research.do_not_choose_when,
            "The task is a simple lookup, an unknown behavioral failure, a product change, an operational action, or only durable publication of already known material."
        );
        assert_eq!(
            research.expected_result,
            "A verified operational research result: answered, bounded negative_result, or explicitly contract-permitted inconclusive, with limitations and optional separate publication handoff."
        );
        let brainstorming = snapshot
            .entries
            .iter()
            .find(|entry| entry.kind == PipelineKind::DeepBrainstorming)
            .unwrap();
        assert_eq!(
            brainstorming.description,
            "Frame a consequential decision, explore viable alternatives, test assumptions and reconcile trade-offs into an explicit decision or recommendation."
        );
        assert_eq!(
            brainstorming.choose_when,
            "The main uncertainty concerns intent, alternatives, criteria or trade-offs and needs sustained exploration before execution or publication."
        );
        assert_eq!(
            brainstorming.do_not_choose_when,
            "A brief clarification is enough, evidence gathering is the main outcome, or a ready design must now be implemented."
        );
        assert_eq!(
            brainstorming.expected_result,
            "A traceable selected, recommended or rejected decision disposition with rationale, unresolved conditions and next work; pending decisions remain unfinished when a decision was requested."
        );
    }

    #[test]
    fn historical_revision_one_remains_the_original_seven_slice_run_entries() {
        let historical = build_snapshot("1", &HISTORICAL_CATALOGUE_KINDS);
        historical.validate().unwrap();
        assert_eq!(historical.revision, "1");
        assert_eq!(historical.entries.len(), 7);
        assert!(
            historical
                .entries
                .iter()
                .all(|entry| entry.execution_owner == PipelineExecutionOwner::SlicePipelineRun)
        );
        assert!(
            !historical
                .entries
                .iter()
                .any(|entry| entry.kind == PipelineKind::PromoteToDurableKnowledge)
        );
    }

    #[test]
    fn historical_promotion_catalogue_remains_the_exact_eight_entry_set() {
        let historical = build_snapshot("3", &HISTORICAL_PROMOTION_CATALOGUE_KINDS);
        historical.validate().unwrap();
        assert_eq!(historical.entries.len(), 8);
        assert!(
            historical
                .entries
                .iter()
                .any(|entry| entry.kind == PipelineKind::ResearchToDurableKnowledge)
        );
        assert!(
            historical
                .entries
                .iter()
                .any(|entry| entry.kind == PipelineKind::PromoteToDurableKnowledge)
        );
        assert!(!historical.entries.iter().any(|entry| matches!(
            entry.kind,
            PipelineKind::Research | PipelineKind::DeepBrainstorming
        )));
    }
}
