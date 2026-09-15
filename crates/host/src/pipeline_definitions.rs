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
            PipelineKind::Research => {
                load(include_str!("../pipeline-definitions/research.json"), kind)
            }
            PipelineKind::DeepBrainstorming => load(
                include_str!("../pipeline-definitions/deep-brainstorming.json"),
                kind,
            ),
            PipelineKind::CustomProcedureCapture => load(
                include_str!("../pipeline-definitions/procedure-capture.json"),
                kind,
            ),
            PipelineKind::PromoteToDurableKnowledge => Err(Error::KnowledgeLifecycleRequired),
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
mod tests;
