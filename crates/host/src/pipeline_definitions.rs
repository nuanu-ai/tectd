use sha2::{Digest, Sha256};
use tect_application::{PipelineDefinitionProvider, PipelineRecommendationDefinitionProvider};
use tect_domain::{Error, PipelineDefinitionSnapshot, PipelineKind, Result};

pub(crate) struct StaticPipelineDefinitions;

/// Supplies only definitions pinned to the current published Slice catalogue.
/// A stale catalogue cannot silently inherit definitions from this host build.
pub struct StaticPipelineRecommendationDefinitions;

impl PipelineRecommendationDefinitionProvider for StaticPipelineRecommendationDefinitions {
    fn definition(
        &self,
        catalogue_revision: &str,
        kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>> {
        if catalogue_revision != crate::slice_pipeline_catalog::CATALOG_REVISION
            || !PipelineKind::CURRENT_SLICE_RUN_KINDS.contains(&kind)
        {
            return Ok(None);
        }
        let definition = if kind == PipelineKind::LightweightTddDevelopment {
            lightweight_v07()?
        } else {
            StaticPipelineDefinitions.definition(kind)?
        };
        Ok((definition.kind == kind).then_some(definition))
    }
}

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

    fn definition_for(
        &self,
        kind: PipelineKind,
        requested_version: Option<&str>,
    ) -> Result<PipelineDefinitionSnapshot> {
        if kind == PipelineKind::LightweightTddDevelopment {
            match requested_version {
                Some("0.7.0-native.k1k5") => return lightweight_v070(),
                Some("0.7.1-native.k1k5") => return lightweight_v07(),
                _ => {}
            }
        }
        let definition = self.definition(kind)?;
        if requested_version.is_some_and(|version| version != definition.version) {
            return Err(Error::InvalidArguments);
        }
        Ok(definition)
    }
}

/// Loads the immutable Lightweight TDD v0.7 contract without changing the
/// v0.6 provider selected by existing runs.  Callers creating a new revision
/// may opt into this definition explicitly; archived runs continue to use the
/// definition snapshot persisted at run creation.
pub(crate) fn lightweight_v07() -> Result<PipelineDefinitionSnapshot> {
    load(
        include_str!("../pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json"),
        PipelineKind::LightweightTddDevelopment,
    )
}

fn lightweight_v070() -> Result<PipelineDefinitionSnapshot> {
    load(
        include_str!("../pipeline-definitions/lightweight-tdd-0.7.0-native.k1k5.json"),
        PipelineKind::LightweightTddDevelopment,
    )
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

pub(crate) fn delivery_modes_v07(
    kind: PipelineKind,
) -> Option<(
    tect_domain::PipelineDeliveryMode,
    Vec<tect_domain::PipelineDeliveryMode>,
)> {
    (kind == PipelineKind::LightweightTddDevelopment)
        .then(|| lightweight_v07().ok())
        .flatten()
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
    if hex(&Sha256::digest(&bytes)) != expected_digest {
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
    if !definition.version.starts_with("0.7") {
        definition
            .validate()
            .map_err(|_| Error::InvalidConfiguration)?;
    }
    Ok(definition)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod engineering_review_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod recommendation_tests {
    use super::*;

    #[test]
    fn recommendation_definitions_are_pinned_to_current_catalogue() {
        let provider = StaticPipelineRecommendationDefinitions;
        for kind in PipelineKind::CURRENT_SLICE_RUN_KINDS {
            let definition = provider
                .definition(crate::slice_pipeline_catalog::CATALOG_REVISION, kind)
                .unwrap()
                .expect("current Slice kind has a pinned definition");
            assert_eq!(definition.kind, kind);
            assert!(!definition.digest.is_empty());
            assert!(provider.definition("stale", kind).unwrap().is_none());
        }
        assert!(
            provider
                .definition(
                    crate::slice_pipeline_catalog::CATALOG_REVISION,
                    PipelineKind::PromoteToDurableKnowledge,
                )
                .unwrap()
                .is_none()
        );
    }
}
