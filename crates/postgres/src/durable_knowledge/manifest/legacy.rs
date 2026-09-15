use super::*;

pub(super) fn semantic(
    selected: &[PipelineKnowledgeItem],
    unresolved: &[String],
) -> Result<String> {
    let material = selected
        .iter()
        .map(|value| {
            (
                &value.unit_id,
                value.revision,
                &value.rdf_digest,
                &value.source_sha256,
                &value.statement,
                value.modality,
                &value.action,
                &value.target_iri,
                &value.conditions,
                &value.exceptions,
            )
        })
        .collect::<Vec<_>>();
    digest(&(material, unresolved))
}
