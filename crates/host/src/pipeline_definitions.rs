use sha2::{Digest, Sha256};
use tect_application::PipelineDefinitionProvider;
use tect_domain::{Error, PipelineDefinitionSnapshot, PipelineKind, Result};

pub(crate) struct StaticPipelineDefinitions;

impl PipelineDefinitionProvider for StaticPipelineDefinitions {
    fn definition(&self, kind: PipelineKind) -> Result<PipelineDefinitionSnapshot> {
        match kind {
            PipelineKind::LightweightTddDevelopment => load(
                include_str!("../pipeline-definitions/lightweight-tdd.json"),
                kind,
            ),
            PipelineKind::FullDesignToExecution => load(
                include_str!("../pipeline-definitions/full-design-to-execution.json"),
                kind,
            ),
            PipelineKind::DebugRootCause => load(
                include_str!("../pipeline-definitions/debug-root-cause.json"),
                kind,
            ),
            PipelineKind::OperationalPreparation => load(
                include_str!("../pipeline-definitions/operational-preparation.json"),
                kind,
            ),
            PipelineKind::OperationalExecution => load(
                include_str!("../pipeline-definitions/operational-execution.json"),
                kind,
            ),
            PipelineKind::ResearchToDurableKnowledge => load(
                include_str!("../pipeline-definitions/research-to-durable-knowledge.json"),
                kind,
            ),
            PipelineKind::CustomProcedureCapture => load(
                include_str!("../pipeline-definitions/procedure-capture.json"),
                kind,
            ),
        }
    }
}

pub(crate) fn delivery_modes(
    kind: PipelineKind,
) -> Option<(
    tect_domain::PipelineDeliveryMode,
    Vec<tect_domain::PipelineDeliveryMode>,
)> {
    StaticPipelineDefinitions
        .definition(kind)
        .ok()
        .map(|definition| (definition.default_mode, definition.allowed_modes))
}

fn load(source: &str, expected: PipelineKind) -> Result<PipelineDefinitionSnapshot> {
    let definition: PipelineDefinitionSnapshot =
        serde_json::from_str(source).map_err(|_| Error::InvalidConfiguration)?;
    if definition.kind != expected {
        return Err(Error::InvalidConfiguration);
    }
    let expected_digest = definition.digest.clone();
    let mut material = definition.clone();
    material.digest.clear();
    let bytes = serde_json::to_vec(&material).map_err(|_| Error::InvalidConfiguration)?;
    if hex(&Sha256::digest(bytes)) != expected_digest {
        return Err(Error::InvalidConfiguration);
    }
    for body in
        std::iter::once(&definition.overview).chain(definition.phases.iter().flat_map(|phase| {
            phase
                .instructions
                .iter()
                .chain(&phase.skills)
                .chain(&phase.resources)
        }))
    {
        if hex(&Sha256::digest(body.body.as_bytes())) != body.digest {
            return Err(Error::InvalidConfiguration);
        }
    }
    definition
        .validate()
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(definition)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightweight_definition_has_exact_complete_bodies() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::LightweightTddDevelopment)
            .unwrap();
        assert_eq!(definition.phases.len(), 14);
        assert!(definition.phases.iter().all(|phase| {
            !phase.instructions.is_empty()
                && phase.instructions.iter().all(|body| body.body.len() > 1000)
        }));
        assert_eq!(definition.phases[3].skills.len(), 1);
        assert!(!definition.phases[7].skills.is_empty());
        assert_eq!(definition.phases[9].skills.len(), 1);
    }

    #[test]
    fn full_definition_is_phasewise_and_retains_resources_and_artifacts() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::FullDesignToExecution)
            .unwrap();
        assert_eq!(definition.phases.len(), 20);
        assert_eq!(definition.allowed_modes.len(), 1);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.resources.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert_eq!(
            definition
                .phases
                .iter()
                .map(|phase| phase.validator_contracts.len())
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn debug_definition_is_complete_and_allows_both_delivery_modes() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::DebugRootCause)
            .unwrap();
        assert_eq!(definition.phases.len(), 18);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn operational_preparation_is_complete_and_allows_both_delivery_modes() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::OperationalPreparation)
            .unwrap();
        assert_eq!(definition.phases.len(), 16);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn operational_execution_is_complete_and_phasewise_only() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::OperationalExecution)
            .unwrap();
        assert_eq!(definition.phases.len(), 18);
        assert_eq!(definition.allowed_modes.len(), 1);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(definition.phases.iter().any(|phase| phase.retry_policy
            == tect_domain::PipelinePhaseRetryPolicy::ReconciliationRequired));
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn research_definition_is_complete_and_defaults_to_phasewise() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::ResearchToDurableKnowledge)
            .unwrap();
        assert_eq!(definition.phases.len(), 22);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.allowed_backward_to.is_empty())
        );
    }

    #[test]
    fn procedure_capture_definition_is_complete_and_defaults_to_whole() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::CustomProcedureCapture)
            .unwrap();
        assert_eq!(definition.phases.len(), 17);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.skills.is_empty())
        );
    }
}
