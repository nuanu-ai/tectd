use crate::{
    Error, PipelineConsumedOutput, PipelineDefinitionSnapshot, PipelineKind,
    PipelinePhaseDefinition, PipelinePhaseOutcome, PipelinePhaseOutputDraft, PipelineTransition,
    Result,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_FOLLOWUP_NODES: usize = 16;
const MAX_FOLLOWUP_DEPENDENCIES: usize = 32;
const MAX_FOLLOWUP_TEXT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineFollowupNodeStatus {
    FutureCandidate,
    SatisfiedByCurrentRun,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineFollowupNode {
    pub local_id: String,
    pub pipeline: PipelineKind,
    pub status: PipelineFollowupNodeStatus,
    pub target: String,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_outputs: Vec<PipelineConsumedOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineFollowupDependency {
    Ordered {
        predecessor: String,
        successor: String,
        condition: String,
    },
    Unresolved {
        node_ids: Vec<String>,
        condition: String,
        owner: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineFollowupProposal {
    pub nodes: Vec<PipelineFollowupNode>,
    pub dependencies: Vec<PipelineFollowupDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCurrentRunSatisfactionContract {
    pub kind: PipelineKind,
    pub evidence_phase_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineFollowupContract {
    pub verdict: String,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub allowed_kinds: Vec<PipelineKind>,
    pub required_kind_groups: Vec<Vec<PipelineKind>>,
    pub minimum_nodes: u32,
    pub minimum_dependencies: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_run: Option<PipelineCurrentRunSatisfactionContract>,
}

pub(crate) fn validate_followup_definitions(
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    let identities = phase
        .followup_contracts
        .iter()
        .map(|contract| (&contract.verdict, contract.outcome, contract.transition))
        .collect::<BTreeSet<_>>();
    if identities.len() != phase.followup_contracts.len() {
        return Err(Error::InvalidArguments);
    }
    for contract in &phase.followup_contracts {
        let allowed = contract
            .allowed_kinds
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let groups = contract
            .required_kind_groups
            .iter()
            .map(|group| group.iter().copied().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();
        let route_exists = phase.verdict_routes.iter().any(|route| {
            route.verdict == contract.verdict
                && route.outcome == contract.outcome
                && route.transition == contract.transition
        });
        let groups_valid = !groups.is_empty()
            && groups.len() == contract.required_kind_groups.len()
            && groups
                .iter()
                .all(|group| !group.is_empty() && group.iter().all(|kind| allowed.contains(kind)));
        let current_valid = contract.current_run.as_ref().is_none_or(|current| {
            current.kind == definition.kind
                && !current.evidence_phase_ids.is_empty()
                && current
                    .evidence_phase_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    == current.evidence_phase_ids.len()
                && current.evidence_phase_ids.iter().all(|id| {
                    definition
                        .phases
                        .iter()
                        .any(|candidate| candidate.id == *id && candidate.ordinal < phase.ordinal)
                })
        });
        if contract.verdict.trim().is_empty()
            || !route_exists
            || allowed.len() != contract.allowed_kinds.len()
            || allowed.is_empty()
            || !groups_valid
            || contract.minimum_nodes < 2
            || contract.minimum_nodes as usize > MAX_FOLLOWUP_NODES
            || contract.minimum_dependencies == 0
            || contract.minimum_dependencies as usize > MAX_FOLLOWUP_DEPENDENCIES
            || !current_valid
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

pub(crate) fn validate_followup_proposal(
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
    outcome: PipelinePhaseOutcome,
    transition: PipelineTransition,
    consumed_outputs: &[PipelineConsumedOutput],
) -> Result<()> {
    let contract = output.verdict.as_ref().and_then(|verdict| {
        phase.followup_contracts.iter().find(|contract| {
            &contract.verdict == verdict
                && contract.outcome == outcome
                && contract.transition == transition
        })
    });
    let Some(contract) = contract else {
        return if output.followup_proposal.is_none() {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        };
    };
    let proposal = output
        .followup_proposal
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    validate_proposal(definition.kind, contract, proposal, consumed_outputs)
}

fn validate_proposal(
    definition_kind: PipelineKind,
    contract: &PipelineFollowupContract,
    proposal: &PipelineFollowupProposal,
    consumed_outputs: &[PipelineConsumedOutput],
) -> Result<()> {
    if proposal.nodes.len() < contract.minimum_nodes as usize
        || proposal.nodes.len() > MAX_FOLLOWUP_NODES
        || proposal.dependencies.len() < contract.minimum_dependencies as usize
        || proposal.dependencies.len() > MAX_FOLLOWUP_DEPENDENCIES
    {
        return Err(Error::InvalidArguments);
    }
    let node_ids = proposal
        .nodes
        .iter()
        .map(|node| &node.local_id)
        .collect::<BTreeSet<_>>();
    let dependency_set = proposal.dependencies.iter().collect::<BTreeSet<_>>();
    let allowed = contract
        .allowed_kinds
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if node_ids.len() != proposal.nodes.len()
        || dependency_set.len() != proposal.dependencies.len()
        || proposal.nodes.iter().any(|node| {
            !valid_text(&node.local_id)
                || !valid_text(&node.target)
                || !valid_text(&node.trigger)
                || !allowed.contains(&node.pipeline)
                || node.source_outputs.iter().collect::<BTreeSet<_>>().len()
                    != node.source_outputs.len()
                || node
                    .source_outputs
                    .iter()
                    .any(|source| !consumed_outputs.contains(source))
        })
        || contract.required_kind_groups.iter().any(|group| {
            !proposal
                .nodes
                .iter()
                .any(|node| group.contains(&node.pipeline))
        })
    {
        return Err(Error::InvalidArguments);
    }
    let current_nodes = proposal
        .nodes
        .iter()
        .filter(|node| node.status == PipelineFollowupNodeStatus::SatisfiedByCurrentRun)
        .collect::<Vec<_>>();
    match &contract.current_run {
        Some(current) => {
            if current_nodes.len() != 1
                || current_nodes[0].pipeline != definition_kind
                || current_nodes[0].pipeline != current.kind
            {
                return Err(Error::InvalidArguments);
            }
            let expected = current
                .evidence_phase_ids
                .iter()
                .filter_map(|id| {
                    consumed_outputs
                        .iter()
                        .find(|source| &source.phase_id == id)
                })
                .cloned()
                .collect::<Vec<_>>();
            if expected.len() != current.evidence_phase_ids.len()
                || current_nodes[0].source_outputs != expected
            {
                return Err(Error::InvalidArguments);
            }
        }
        None if !current_nodes.is_empty() => return Err(Error::InvalidArguments),
        None => {}
    }
    validate_dependencies(
        proposal,
        &node_ids,
        current_nodes.first().map(|node| &node.local_id),
    )
}

fn validate_dependencies(
    proposal: &PipelineFollowupProposal,
    node_ids: &BTreeSet<&String>,
    current_id: Option<&String>,
) -> Result<()> {
    let mut edges = Vec::new();
    for dependency in &proposal.dependencies {
        match dependency {
            PipelineFollowupDependency::Ordered {
                predecessor,
                successor,
                condition,
            } => {
                if predecessor == successor
                    || !node_ids.contains(predecessor)
                    || !node_ids.contains(successor)
                    || !valid_text(condition)
                    || current_id == Some(successor)
                {
                    return Err(Error::InvalidArguments);
                }
                edges.push((predecessor, successor));
            }
            PipelineFollowupDependency::Unresolved {
                node_ids: unresolved,
                condition,
                owner,
            } => {
                if unresolved.len() < 2
                    || unresolved.iter().collect::<BTreeSet<_>>().len() != unresolved.len()
                    || unresolved.iter().any(|id| !node_ids.contains(id))
                    || !valid_text(condition)
                    || !valid_text(owner)
                    || current_id.is_some_and(|current| unresolved.contains(current))
                {
                    return Err(Error::InvalidArguments);
                }
            }
        }
    }
    if has_cycle(node_ids, &edges) {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

fn has_cycle(node_ids: &BTreeSet<&String>, edges: &[(&String, &String)]) -> bool {
    let mut incoming = node_ids
        .iter()
        .map(|id| ((*id).as_str(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for (_, successor) in edges {
        if let Some(count) = incoming.get_mut(successor.as_str()) {
            *count += 1;
        }
    }
    let mut ready = incoming
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        for (_, successor) in edges
            .iter()
            .filter(|(predecessor, _)| predecessor.as_str() == id)
        {
            let count = incoming.get_mut(successor.as_str()).expect("known node");
            *count -= 1;
            if *count == 0 {
                ready.push(successor);
            }
        }
    }
    visited != node_ids.len()
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && value.len() <= MAX_FOLLOWUP_TEXT_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(phase_id: &str, revision: i64) -> PipelineConsumedOutput {
        PipelineConsumedOutput {
            phase_id: phase_id.into(),
            output_revision: revision,
            digest: format!("digest-{revision}"),
        }
    }

    fn contract(current: bool) -> PipelineFollowupContract {
        PipelineFollowupContract {
            verdict: "graph_required".into(),
            outcome: PipelinePhaseOutcome::Completed,
            transition: PipelineTransition::Escalate,
            allowed_kinds: vec![
                PipelineKind::LightweightTddDevelopment,
                PipelineKind::OperationalPreparation,
                PipelineKind::OperationalExecution,
            ],
            required_kind_groups: vec![
                vec![PipelineKind::LightweightTddDevelopment],
                vec![
                    PipelineKind::OperationalPreparation,
                    PipelineKind::OperationalExecution,
                ],
            ],
            minimum_nodes: 2,
            minimum_dependencies: 1,
            current_run: current.then(|| PipelineCurrentRunSatisfactionContract {
                kind: PipelineKind::LightweightTddDevelopment,
                evidence_phase_ids: vec!["verified".into()],
            }),
        }
    }

    fn node(
        local_id: &str,
        pipeline: PipelineKind,
        status: PipelineFollowupNodeStatus,
        source_outputs: Vec<PipelineConsumedOutput>,
    ) -> PipelineFollowupNode {
        PipelineFollowupNode {
            local_id: local_id.into(),
            pipeline,
            status,
            target: format!("target-{local_id}"),
            trigger: format!("trigger-{local_id}"),
            source_outputs,
        }
    }

    fn ordered(predecessor: &str, successor: &str) -> PipelineFollowupDependency {
        PipelineFollowupDependency::Ordered {
            predecessor: predecessor.into(),
            successor: successor.into(),
            condition: "recorded evidence permits this dependency".into(),
        }
    }

    #[test]
    fn current_run_evidence_is_exact_and_cannot_depend_on_future_work() {
        let verified = source("verified", 2);
        let consumed = vec![source("implemented", 1), verified.clone()];
        let mut proposal = PipelineFollowupProposal {
            nodes: vec![
                node(
                    "current",
                    PipelineKind::LightweightTddDevelopment,
                    PipelineFollowupNodeStatus::SatisfiedByCurrentRun,
                    vec![verified.clone()],
                ),
                node(
                    "operation",
                    PipelineKind::OperationalPreparation,
                    PipelineFollowupNodeStatus::FutureCandidate,
                    vec![],
                ),
            ],
            dependencies: vec![ordered("current", "operation")],
        };
        assert!(
            validate_proposal(
                PipelineKind::LightweightTddDevelopment,
                &contract(true),
                &proposal,
                &consumed,
            )
            .is_ok()
        );
        proposal.dependencies = vec![ordered("operation", "current")];
        assert!(
            validate_proposal(
                PipelineKind::LightweightTddDevelopment,
                &contract(true),
                &proposal,
                &consumed,
            )
            .is_err()
        );
        proposal.dependencies = vec![ordered("current", "operation")];
        proposal.nodes[0].source_outputs = vec![source("verified", 99)];
        assert!(
            validate_proposal(
                PipelineKind::LightweightTddDevelopment,
                &contract(true),
                &proposal,
                &consumed,
            )
            .is_err()
        );
    }

    #[test]
    fn future_graph_is_acyclic_or_records_an_unresolved_dependency() {
        let mut proposal = PipelineFollowupProposal {
            nodes: vec![
                node(
                    "implementation",
                    PipelineKind::LightweightTddDevelopment,
                    PipelineFollowupNodeStatus::FutureCandidate,
                    vec![],
                ),
                node(
                    "operation",
                    PipelineKind::OperationalExecution,
                    PipelineFollowupNodeStatus::FutureCandidate,
                    vec![],
                ),
            ],
            dependencies: vec![
                ordered("implementation", "operation"),
                ordered("operation", "implementation"),
            ],
        };
        assert!(
            validate_proposal(
                PipelineKind::OperationalPreparation,
                &contract(false),
                &proposal,
                &[],
            )
            .is_err()
        );
        proposal.dependencies = vec![PipelineFollowupDependency::Unresolved {
            node_ids: vec!["implementation".into(), "operation".into()],
            condition: "planning must resolve the evidence-dependent order".into(),
            owner: "future Slice planning".into(),
        }];
        assert!(
            validate_proposal(
                PipelineKind::OperationalPreparation,
                &contract(false),
                &proposal,
                &[],
            )
            .is_ok()
        );
    }
}
