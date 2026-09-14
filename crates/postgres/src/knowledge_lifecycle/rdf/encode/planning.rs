use super::*;

pub(super) fn encode_planning_briefs(
    builder: &mut Builder,
    revision: &str,
    document: &KnowledgeDocumentDraft,
) -> Result<()> {
    let nodes = structured_list(
        builder,
        revision,
        "planningBriefs",
        revision,
        document.planning_briefs.len(),
        "PlanningBrief",
    )?;
    for (node, brief) in nodes.iter().zip(&document.planning_briefs) {
        builder.text(node, &field("localId"), &brief.local_id)?;
        builder.iri(
            node,
            &field("stage"),
            &enum_iri("planning-stage", brief.stage)?,
        )?;
        builder.text(node, &field("instruction"), &brief.instruction)?;
        builder.text(node, &field("purpose"), &brief.purpose)?;
        list_text(builder, node, "conditions", node, &brief.conditions)?;
        list_text(builder, node, "exceptions", node, &brief.exceptions)?;
        list_iris(
            builder,
            node,
            "targetSelectors",
            node,
            &brief.selectors.target_iris,
        )?;
        list_iris(
            builder,
            node,
            "environmentSelectors",
            node,
            &brief.selectors.environment_iris,
        )?;
        list_text(
            builder,
            node,
            "actionClassSelectors",
            node,
            &brief.selectors.action_classes,
        )?;
    }
    Ok(())
}
