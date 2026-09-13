use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{PipelineCatalogueEntry, PipelineCatalogueSnapshot, PipelineKind};

pub(crate) const CATALOG_REVISION: &str = "1";

struct SlicePipelineStub {
    kind: PipelineKind,
    description: &'static str,
    choose_when: &'static str,
    do_not_choose_when: &'static str,
    expected_result: &'static str,
}

const PIPELINES: [SlicePipelineStub; 7] = [
    SlicePipelineStub {
        kind: PipelineKind::LightweightTddDevelopment,
        description: "A bounded development change with clear expected behavior, a minimally sufficient test cycle, and a verified implementation; this is the default development path.",
        choose_when: "The requested fix, bug resolution, or small vertical feature has a bounded outcome that can be implemented and verified directly.",
        do_not_choose_when: "Evidence shows irreducible design complexity, the work is solely diagnosis or research, or the requested outcome is an operational action or procedure capture.",
        expected_result: "The bounded behavior is implemented and verified against concrete acceptance evidence.",
    },
    SlicePipelineStub {
        kind: PipelineKind::FullDesignToExecution,
        description: "Clarify substantial interconnected complexity in one indivisible vertical outcome and carry the resulting design through verified implementation.",
        choose_when: "Evidence shows that Lightweight is insufficient and no further sensible vertical decomposition can make the outcome independently deliverable.",
        do_not_choose_when: "The only reasons are size, a new-feature label, cross-component work, uncertainty that can be isolated, or use as a generic fallback.",
        expected_result: "A justified indivisible design is implemented and verified, with the reasons against Lightweight and further vertical division recorded.",
    },
    SlicePipelineStub {
        kind: PipelineKind::DebugRootCause,
        description: "Establish the demonstrated cause of a behavioral deviation and the boundary and direction of its correction.",
        choose_when: "The cause of observed incorrect behavior is materially unknown and must be proven before selecting corrective work.",
        do_not_choose_when: "The cause is already established or the candidate silently includes implementing the fix.",
        expected_result: "A demonstrated root cause, evidence, affected boundary, and supported correction direction that can inform follow-up work.",
    },
    SlicePipelineStub {
        kind: PipelineKind::OperationalPreparation,
        description: "Prepare a precise operation plan including prechecks, authority, risks and stop conditions, rollback, and result criteria; the operation remains unperformed.",
        choose_when: "A consequential operation needs a reviewable and bounded plan before execution can be authorized or safely attempted.",
        do_not_choose_when: "The requested outcome is implementation, research, diagnosis, or execution of an already prepared and authorized operation.",
        expected_result: "A complete operation plan with exact target, authority boundary, checks, stops, rollback, and evidence criteria, without performing the operation.",
    },
    SlicePipelineStub {
        kind: PipelineKind::OperationalExecution,
        description: "Perform an authorized bounded operation against an exact target with evidence of the actual result and recovery when necessary.",
        choose_when: "The operation, target, authority, safety conditions, and success evidence are sufficiently specified for execution.",
        do_not_choose_when: "Authority or the operation plan is unresolved, or the outcome is preparatory planning or product implementation.",
        expected_result: "The authorized operation is executed within its boundary and its actual result and any recovery are evidenced.",
    },
    SlicePipelineStub {
        kind: PipelineKind::ResearchToDurableKnowledge,
        description: "Close specific research questions with verifiable findings, sources, contradictions, and gaps, producing reusable knowledge without automatic publication.",
        choose_when: "The bounded outcome is a durable answer to explicit research questions rather than a code or operational change.",
        do_not_choose_when: "Research is merely incidental to implementation, or publication and activation have not been separately requested.",
        expected_result: "Traceable reusable knowledge that answers the stated questions and honestly records conflicts and remaining gaps.",
    },
    SlicePipelineStub {
        kind: PipelineKind::CustomProcedureCapture,
        description: "Capture a repeatable process as a procedure, runbook, or skill candidate with application boundaries and verification.",
        choose_when: "The explicitly requested bounded outcome is a reusable procedure for a recurring process.",
        do_not_choose_when: "Procedure capture was not requested, the process is not sufficiently repeatable, or this would automatically create or activate a skill after ordinary work.",
        expected_result: "A verified reusable procedure candidate with clear applicability and limits, without automatic activation.",
    },
];

pub(crate) fn snapshot() -> PipelineCatalogueSnapshot {
    let entries = PIPELINES
        .iter()
        .map(|pipeline| {
            let modes = crate::pipeline_definitions::delivery_modes(pipeline.kind);
            let executable = modes.is_some();
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
            }
        })
        .collect::<Vec<_>>();
    let digest = digest_entries(&entries);
    PipelineCatalogueSnapshot {
        revision: CATALOG_REVISION.into(),
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
    json!({
        "revision": snapshot.revision,
        "digest": snapshot.digest,
        "implementation_status": if executable_count == snapshot.entries.len() { "executable" } else { "partial" },
        "description_status": if executable_count == snapshot.entries.len() { "refined" } else { "partial" },
        "refinement_required": executable_count != snapshot.entries.len(),
        "executable": executable_count > 0,
        "executable_count": executable_count,
        "pipelines": snapshot.entries,
    })
}

fn digest_entries(entries: &[PipelineCatalogueEntry]) -> String {
    let bytes =
        serde_json::to_vec(&(CATALOG_REVISION, entries)).expect("static catalog serializes");
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalogue_contains_seven_executable_pipelines_and_no_retired_aliases() {
        let snapshot = snapshot();
        let ids = snapshot
            .entries
            .iter()
            .map(|pipeline| pipeline.kind.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 7);
        assert!(!ids.contains("slice.hybrid-implementation-operation"));
        assert!(!ids.contains("slice.research-to-durable-kb"));
        assert_eq!(
            snapshot
                .entries
                .iter()
                .filter(|pipeline| pipeline.executable)
                .count(),
            7
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
        assert_eq!(catalog["executable_count"], 7);
        assert_eq!(catalog["implementation_status"], "executable");
        assert_eq!(catalog["description_status"], "refined");
        assert_eq!(catalog["refinement_required"], false);
        assert!(catalog.get("stages").is_none());
    }
}
