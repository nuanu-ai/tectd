use super::*;
/// Static semantic roles. No client-supplied role is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixFactRole {
    DeclaredRequirement,
    OperatingEvidence,
    Unresolved,
}
pub fn matrix_fact_role(path: &str, input: &EngineeringMatrixInput) -> Result<MatrixFactRole> {
    Ok(match path {
        "/mode" | "/intent" | "/urgency" | "/promised_behavior" | "/promised_proof" => {
            MatrixFactRole::DeclaredRequirement
        }
        "/demand_commitment" | "/latency_commitment" => {
            let fact = if path == "/demand_commitment" {
                &input.demand_commitment
            } else {
                &input.latency_commitment
            };
            match fact {
                MatrixFact::Known {
                    value: CommitmentEvidence::NoCommitment,
                    ..
                } => MatrixFactRole::DeclaredRequirement,
                MatrixFact::Known {
                    value:
                        CommitmentEvidence::WithinVerifiedLimit
                        | CommitmentEvidence::ExceedsVerifiedLimit,
                    ..
                } => MatrixFactRole::OperatingEvidence,
                _ => MatrixFactRole::Unresolved,
            }
        }
        "/envelope/scale"
        | "/criticality"
        | "/affected_guarantees"
        | "/actual_exposure"
        | "/urgent_repair"
        | "/envelope/operational_facts" => MatrixFactRole::OperatingEvidence,
        p if p.starts_with("/envelope/operational_facts/")
            && p.len() > "/envelope/operational_facts/".len() =>
        {
            MatrixFactRole::OperatingEvidence
        }
        _ => return Err(Error::InvalidArguments),
    })
}
