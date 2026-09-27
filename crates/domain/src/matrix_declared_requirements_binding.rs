use super::*;
/// Inject absent declarations; reject unresolved or conflicting caller claims.
/// Matching claims retain their provenance but acquire no independent verification.
pub fn bind_matrix_requirements_input(
    context: &EffectiveMatrixRequirements,
    input: &EngineeringMatrixInput,
) -> Result<EngineeringMatrixInput> {
    input.validate()?;
    if !missing_required_matrix_declarations(context).is_empty() {
        return Err(Error::InvalidArguments);
    }
    let mut output = input.clone();
    for resolved in context.values.values() {
        let provenance = FactProvenance(format!("requirements:{}", context.semantic_digest));
        match &resolved.value {
            DeclaredRequirementValue::Mode(value) => bind(&mut output.mode, *value, provenance)?,
            DeclaredRequirementValue::Intent(value) => {
                bind(&mut output.intent, value.clone(), provenance)?
            }
            DeclaredRequirementValue::Urgency(value) => {
                bind(&mut output.urgency, value.clone(), provenance)?
            }
            DeclaredRequirementValue::PromisedBehavior(value) => {
                bind(&mut output.promised_behavior, value.clone(), provenance)?
            }
            DeclaredRequirementValue::PromisedProof(value) => {
                bind(&mut output.promised_proof, value.clone(), provenance)?
            }
            DeclaredRequirementValue::NoDemandCommitment => bind(
                &mut output.demand_commitment,
                CommitmentEvidence::NoCommitment,
                provenance,
            )?,
            DeclaredRequirementValue::NoLatencyCommitment => bind(
                &mut output.latency_commitment,
                CommitmentEvidence::NoCommitment,
                provenance,
            )?,
        }
    }
    output.validate()?;
    Ok(output)
}
fn bind<T: PartialEq>(
    fact: &mut MatrixFact<T>,
    value: T,
    provenance: FactProvenance,
) -> Result<()> {
    match fact {
        MatrixFact::Absent => {
            *fact = MatrixFact::Known { value, provenance };
            Ok(())
        }
        MatrixFact::Known {
            value: existing, ..
        } if *existing == value => Ok(()),
        _ => Err(Error::InvalidArguments),
    }
}

/// New context-aware partition; legacy required_matrix_facts is unchanged.
pub fn required_matrix_operating_facts(
    context: &EffectiveMatrixRequirements,
    input: &EngineeringMatrixInput,
) -> Result<Vec<RequiredMatrixFact>> {
    let bound = bind_matrix_requirements_input(context, input)?;
    let mut operating = Vec::new();
    for fact in required_matrix_facts(&bound)? {
        match matrix_fact_role(&fact.path, &bound)? {
            MatrixFactRole::OperatingEvidence => operating.push(fact),
            MatrixFactRole::DeclaredRequirement => {
                if !context
                    .values
                    .keys()
                    .any(|path| path.fact_path() == fact.path)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            MatrixFactRole::Unresolved => return Err(Error::InvalidArguments),
        }
    }
    Ok(operating)
}

/// Provisional composition with explicit declaration context. This deliberately
/// preserves pending operating verification and cannot mint a legacy verified token.
pub fn compose_declared_requirements_matrix(
    context: &EffectiveMatrixRequirements,
    task_id: String,
    task_revision: String,
    input: &EngineeringMatrixInput,
) -> Result<crate::EngineeringMatrixComposition> {
    let input = bind_matrix_requirements_input(context, input)?;
    let reported = crate::OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        task_id,
        task_revision,
        input,
    )?;
    Ok(crate::compose_owner_reported_engineering_matrix(&reported))
}
