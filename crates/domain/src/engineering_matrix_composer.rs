//! Pure EM02-INITIAL@0.1 mandatory composition. Source verification belongs to the caller.

use crate::{
    CommitmentEvidence, EngineeringIntent, EngineeringMatrixInput, EngineeringMode, Error,
    FactProvenance, MatrixFact, OperationalFacts, Result,
};
use serde::{Deserialize, Serialize};

pub const ENGINEERING_MATRIX_CATALOGUE_VERSION: &str = "EM02-INITIAL@0.1";

/// The caller asserts that every fact and provenance was checked against this
/// exact task revision. This value does not verify task state by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedEngineeringMatrixFacts {
    task_id: String,
    task_revision: String,
    input: EngineeringMatrixInput,
}

impl VerifiedEngineeringMatrixFacts {
    pub fn from_verified_task_revision(
        task_id: String,
        task_revision: String,
        input: EngineeringMatrixInput,
    ) -> Result<Self> {
        if task_id.trim().is_empty()
            || task_revision.trim().is_empty()
            || task_id.len() > 256
            || task_revision.len() > 256
        {
            return Err(Error::InvalidArguments);
        }
        input.validate()?;
        Ok(Self {
            task_id,
            task_revision,
            input,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatrixEvidenceState {
    Absent,
    KnownEmpty,
    Unknown,
    Gap,
    Conflict,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedMatrixEvidence {
    pub field: String,
    pub state: MatrixEvidenceState,
    pub provenance: Option<FactProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandatoryMatrixCard {
    pub id: &'static str,
    pub catalogue_version: &'static str,
    pub summary: &'static str,
    pub body: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineeringMatrixComposition {
    pub catalogue_version: &'static str,
    pub task_id: String,
    pub task_revision: String,
    pub mandatory_cards: Vec<MandatoryMatrixCard>,
    pub unresolved_evidence: Vec<UnresolvedMatrixEvidence>,
}

impl EngineeringMatrixComposition {
    pub fn is_resolved(&self) -> bool {
        self.unresolved_evidence.is_empty()
    }
}

const SCOPE: MandatoryMatrixCard = MandatoryMatrixCard {
    id: "EM02-SCOPE@0.1",
    catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
    summary: "Record scope, operating facts, promised behavior and proof in every mode.",
    body: "Record the owner-established mode, evidenced operating envelope, criticality, intent and urgency, plus promised behavior and proof. Demo delivers the real simplest behavior; fake external behavior requires explicit operator approval. MVP narrows supported scope, never correctness. Mode alone waives nothing.",
};
const PROTECT: MandatoryMatrixCard = MandatoryMatrixCard {
    id: "EM02-PROTECT@0.1",
    catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
    summary: "Preserve affected payment, secret and data guarantees with proof.",
    body: "When payments, secrets or data guarantees are affected, preserve every affected guarantee and its proof in every mode.",
};
const OPERATE: MandatoryMatrixCard = MandatoryMatrixCard {
    id: "EM02-OPERATE@0.1",
    catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
    summary: "Verify detection, recovery and environment for actual exposure.",
    body: "When actual users or workloads are exposed, require detection, containment/recovery and environmental verification proportionate to the facts. This policy does not authorize release.",
};
const CAPACITY: MandatoryMatrixCard = MandatoryMatrixCard {
    id: "EM02-CAPACITY@0.1",
    catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
    summary: "Evidence and mitigate the specific demand or latency constraint.",
    body: "When a stated demand or latency commitment lacks evidence or exceeds a verified limit, require evidence and mitigation for the specific constraint. Do not invent numeric tiers or infer high scale from the Production label.",
};
const HOTFIX: MandatoryMatrixCard = MandatoryMatrixCard {
    id: "EM02-HOTFIX@0.1",
    catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
    summary: "Narrow the urgent Production repair and prove regression and recovery.",
    body: "When the owner establishes an urgent repair of an existing Production failure, require a narrow fix, focused regression, recovery/rollback and a revisit of the disposition. Existing duties remain; urgency does not authorize release.",
};

/// Conditional cards are emitted only for established triggers. Any unresolved
/// evidence is explicit, so a partial card set cannot be treated as complete.
pub fn compose_engineering_matrix(
    verified: &VerifiedEngineeringMatrixFacts,
) -> EngineeringMatrixComposition {
    let input = &verified.input;
    let mut unresolved = Vec::new();
    for (field, fact) in [
        ("mode", fact_state(&input.mode)),
        ("envelope.scale", fact_state(&input.envelope.scale)),
        ("criticality", fact_state(&input.criticality)),
        ("intent", fact_state(&input.intent)),
        ("urgency", fact_state(&input.urgency)),
        ("promised_behavior", fact_state(&input.promised_behavior)),
        ("promised_proof", fact_state(&input.promised_proof)),
    ] {
        if let Some((state, provenance)) = fact {
            push_issue(&mut unresolved, field, state, provenance);
        }
    }
    match &input.envelope.operational_facts {
        OperationalFacts::Absent => push_issue(
            &mut unresolved,
            "envelope.operational_facts",
            MatrixEvidenceState::Absent,
            None,
        ),
        OperationalFacts::KnownEmpty { .. } => {}
        OperationalFacts::Reported { entries } => {
            for entry in entries {
                if let Some((state, provenance)) = fact_state(&entry.fact) {
                    push_issue(
                        &mut unresolved,
                        &format!("envelope.operational_facts.{}", entry.name),
                        state,
                        provenance,
                    );
                }
            }
        }
    }
    let mut cards = vec![SCOPE];
    match &input.affected_guarantees {
        MatrixFact::Known { .. } => cards.push(PROTECT),
        MatrixFact::KnownEmpty { .. } => {}
        fact => add_fact_issue(&mut unresolved, "affected_guarantees", fact),
    }
    match &input.actual_exposure {
        MatrixFact::Known { value: true, .. } => cards.push(OPERATE),
        MatrixFact::Known { value: false, .. } => {}
        fact => add_fact_issue(&mut unresolved, "actual_exposure", fact),
    }
    let mut capacity_required = false;
    for (field, fact) in [
        ("demand_commitment", &input.demand_commitment),
        ("latency_commitment", &input.latency_commitment),
    ] {
        match fact {
            MatrixFact::Known {
                value: CommitmentEvidence::LacksEvidence | CommitmentEvidence::ExceedsVerifiedLimit,
                ..
            } => capacity_required = true,
            MatrixFact::Known { .. } => {}
            fact => add_fact_issue(&mut unresolved, field, fact),
        }
    }
    if capacity_required {
        cards.push(CAPACITY);
    }
    if matches!(
        input.intent,
        MatrixFact::Known {
            value: EngineeringIntent::ProductionHotfix,
            ..
        }
    ) {
        match (&input.mode, &input.urgent_repair) {
            (
                MatrixFact::Known {
                    value: EngineeringMode::Production,
                    ..
                },
                MatrixFact::Known { value: true, .. },
            ) => cards.push(HOTFIX),
            (
                _,
                MatrixFact::Known {
                    value: false,
                    provenance,
                },
            ) => push_issue(
                &mut unresolved,
                "urgent_repair",
                MatrixEvidenceState::Conflict,
                Some(provenance.clone()),
            ),
            (_, fact) => add_fact_issue(&mut unresolved, "urgent_repair", fact),
        }
    } else {
        add_fact_issue(&mut unresolved, "urgent_repair", &input.urgent_repair);
    }
    EngineeringMatrixComposition {
        catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
        task_id: verified.task_id.clone(),
        task_revision: verified.task_revision.clone(),
        mandatory_cards: cards,
        unresolved_evidence: unresolved,
    }
}

fn fact_state<T>(fact: &MatrixFact<T>) -> Option<(MatrixEvidenceState, Option<FactProvenance>)> {
    match fact {
        MatrixFact::Known { .. } => None,
        MatrixFact::Absent => Some((MatrixEvidenceState::Absent, None)),
        MatrixFact::KnownEmpty { provenance } => {
            Some((MatrixEvidenceState::KnownEmpty, Some(provenance.clone())))
        }
        MatrixFact::Unknown { provenance } => {
            Some((MatrixEvidenceState::Unknown, Some(provenance.clone())))
        }
        MatrixFact::Gap { provenance } => {
            Some((MatrixEvidenceState::Gap, Some(provenance.clone())))
        }
        MatrixFact::Conflict { provenance } => {
            Some((MatrixEvidenceState::Conflict, Some(provenance.clone())))
        }
        MatrixFact::Invalid { provenance } => {
            Some((MatrixEvidenceState::Invalid, Some(provenance.clone())))
        }
    }
}

fn add_fact_issue<T>(
    unresolved: &mut Vec<UnresolvedMatrixEvidence>,
    field: &str,
    fact: &MatrixFact<T>,
) {
    if let Some((state, provenance)) = fact_state(fact) {
        push_issue(unresolved, field, state, provenance);
    }
}

fn push_issue(
    unresolved: &mut Vec<UnresolvedMatrixEvidence>,
    field: &str,
    state: MatrixEvidenceState,
    provenance: Option<FactProvenance>,
) {
    if !unresolved.iter().any(|issue| issue.field == field) {
        unresolved.push(UnresolvedMatrixEvidence {
            field: field.to_owned(),
            state,
            provenance,
        });
    }
}

#[cfg(test)]
#[path = "engineering_matrix_composer_tests.rs"]
mod tests;
