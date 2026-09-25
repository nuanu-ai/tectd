//! A verification plan is the full required-phase projection of one pinned
//! pipeline definition. It grants no authority to attest or advance a phase.

use crate::{
    Error, PipelineArtifactRequirement, PipelineDefinitionSnapshot, PipelineKind,
    PipelineOutputConstraint, PipelineValidatorContract, PipelineVerdictRoute, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const PIPELINE_VERIFICATION_PLAN_SCHEMA: &str = "tect.pipeline-verification-plan/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineVerificationObligation {
    pub phase_id: String,
    pub required_fields: Vec<String>,
    pub required_artifacts: Vec<PipelineArtifactRequirement>,
    pub validator_contracts: Vec<PipelineValidatorContract>,
    pub output_constraints: Vec<PipelineOutputConstraint>,
    pub allowed_verdicts: Vec<String>,
    pub verdict_routes: Vec<PipelineVerdictRoute>,
    pub disposition_required: bool,
    pub required_dispositions: Vec<String>,
    pub fresh_reviewer_input: bool,
    pub output_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineVerificationPlan {
    pub schema: String,
    pub id: String,
    pub digest: String,
    pub source_kind: PipelineKind,
    pub source_definition_version: String,
    pub source_definition_digest: String,
    pub obligations: Vec<PipelineVerificationObligation>,
}

impl PipelineVerificationPlan {
    pub fn from_definition(definition: &PipelineDefinitionSnapshot) -> Result<Self> {
        definition.validate()?;
        let obligations = definition
            .phases
            .iter()
            .filter(|phase| phase.required)
            .map(|phase| PipelineVerificationObligation {
                phase_id: phase.id.clone(),
                required_fields: phase.required_fields.clone(),
                required_artifacts: phase.required_artifacts.clone(),
                validator_contracts: phase.validator_contracts.clone(),
                output_constraints: phase.output_constraints.clone(),
                allowed_verdicts: phase.allowed_verdicts.clone(),
                verdict_routes: phase.verdict_routes.clone(),
                disposition_required: phase.disposition_required,
                required_dispositions: phase.required_dispositions.clone(),
                fresh_reviewer_input: phase.fresh_reviewer_input,
                output_contract: phase.output_contract.clone(),
            })
            .collect();
        let mut plan = Self {
            schema: PIPELINE_VERIFICATION_PLAN_SCHEMA.into(),
            id: String::new(),
            digest: String::new(),
            source_kind: definition.kind,
            source_definition_version: definition.version.clone(),
            source_definition_digest: definition.digest.clone(),
            obligations,
        };
        plan.digest = plan.content_digest()?;
        plan.id = format!("verification-plan:{}", plan.digest);
        Ok(plan)
    }

    pub fn validate(&self) -> Result<()> {
        let distinct = self
            .obligations
            .iter()
            .map(|obligation| &obligation.phase_id)
            .collect::<BTreeSet<_>>();
        if self.schema != PIPELINE_VERIFICATION_PLAN_SCHEMA
            || self.source_definition_version.trim().is_empty()
            || self.source_definition_digest.trim().is_empty()
            || self.obligations.is_empty()
            || distinct.len() != self.obligations.len()
            || self
                .obligations
                .iter()
                .any(|obligation| obligation.phase_id.trim().is_empty())
            || self.digest != self.content_digest()?
            || self.id != format!("verification-plan:{}", self.digest)
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    pub fn validate_against_definition(
        &self,
        definition: &PipelineDefinitionSnapshot,
    ) -> Result<()> {
        self.validate()?;
        if *self != Self::from_definition(definition)? {
            return Err(Error::StaleContext);
        }
        Ok(())
    }

    pub(crate) fn content_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct Content<'a> {
            schema: &'a str,
            source_kind: PipelineKind,
            source_definition_version: &'a str,
            source_definition_digest: &'a str,
            obligations: &'a [PipelineVerificationObligation],
        }
        let content = Content {
            schema: &self.schema,
            source_kind: self.source_kind,
            source_definition_version: &self.source_definition_version,
            source_definition_digest: &self.source_definition_digest,
            obligations: &self.obligations,
        };
        let bytes = serde_json::to_vec(&content).map_err(|_| Error::InvalidArguments)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}
