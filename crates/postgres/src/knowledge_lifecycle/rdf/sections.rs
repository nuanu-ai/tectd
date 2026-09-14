use super::encode::{enum_iri, field, list_iris, list_text, list_u32, structured_list};
use super::model::{Builder, RDF_TYPE, V2};
use tect_domain::*;

pub(super) fn encode(
    builder: &mut Builder,
    revision: &str,
    document: &KnowledgeDocumentDraft,
) -> Result<()> {
    if let Some(section) = &document.sections.constraint {
        let node = section_node(builder, revision, "constraint", "ConstraintSection")?;
        builder.iri(
            &node,
            &field("modality"),
            &enum_iri("modality", section.modality)?,
        )?;
        builder.text(&node, &field("action"), &section.action)?;
        builder.iri(&node, &field("target"), &section.target_iri)?;
    }
    for profile in &document.profiles {
        match profile {
            KnowledgeProfileId::General => {
                if let Some(section) = &document.sections.general {
                    general(builder, revision, section)?;
                }
            }
            KnowledgeProfileId::Runbook => runbook(
                builder,
                revision,
                document
                    .sections
                    .runbook
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
            KnowledgeProfileId::Protocol => protocol(
                builder,
                revision,
                document
                    .sections
                    .protocol
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
            KnowledgeProfileId::Devops => devops(
                builder,
                revision,
                document
                    .sections
                    .devops
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
            KnowledgeProfileId::Operations => operations(
                builder,
                revision,
                document
                    .sections
                    .operations
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
            KnowledgeProfileId::ProductResearch => product_research(
                builder,
                revision,
                document
                    .sections
                    .product_research
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
            KnowledgeProfileId::Security => security(
                builder,
                revision,
                document
                    .sections
                    .security
                    .as_ref()
                    .ok_or(Error::InvalidArguments)?,
            )?,
        }
    }
    Ok(())
}

fn section_node(builder: &mut Builder, revision: &str, name: &str, class: &str) -> Result<String> {
    let node = format!("{revision}:section:{name}");
    builder.iri(revision, &field("profileSection"), &node)?;
    let named = match name {
        "product_research" => "productResearchSection",
        _ => match name {
            "constraint" => "constraintSection",
            "general" => "generalSection",
            "runbook" => "runbookSection",
            "protocol" => "protocolSection",
            "devops" => "devopsSection",
            "operations" => "operationsSection",
            "security" => "securitySection",
            _ => return Err(Error::InternalInvariant),
        },
    };
    builder.iri(revision, &field(named), &node)?;
    builder.iri(&node, RDF_TYPE, &format!("{V2}{class}"))?;
    builder.iri(&node, &field("revision"), revision)?;
    builder.iri(&node, &field("profile"), &format!("{V2}profile:{name}"))?;
    Ok(node)
}

fn general(builder: &mut Builder, revision: &str, value: &KnowledgeGeneralSection) -> Result<()> {
    let node = section_node(builder, revision, "general", "GeneralSection")?;
    builder.text(&node, &field("statement"), &value.statement)?;
    builder.text(&node, &field("evidenceScope"), &value.evidence_scope)?;
    builder.text(&node, &field("rationale"), &value.rationale)?;
    list_text(builder, &node, "assumptions", &node, &value.assumptions)?;
    list_text(builder, &node, "alternatives", &node, &value.alternatives)?;
    list_text(
        builder,
        &node,
        "negativeLimits",
        &node,
        &value.negative_limits,
    )?;
    list_text(
        builder,
        &node,
        "unknownLimits",
        &node,
        &value.unknown_limits,
    )
}

fn runbook(builder: &mut Builder, revision: &str, value: &KnowledgeRunbookSection) -> Result<()> {
    let node = section_node(builder, revision, "runbook", "RunbookSection")?;
    builder.text(&node, &field("purposeAndFit"), &value.purpose_and_fit)?;
    builder.text(
        &node,
        &field("requiredAuthority"),
        &value.required_authority,
    )?;
    builder.text(
        &node,
        &field("failureAndRecovery"),
        &value.failure_and_recovery,
    )?;
    builder.iri(
        &node,
        &field("proofStatus"),
        &enum_iri("proof-status", value.proof_status)?,
    )?;
    list_iris(
        builder,
        &node,
        "targetEnvironments",
        &node,
        &value.target_environment_iris,
    )?;
    list_text(builder, &node, "prerequisites", &node, &value.prerequisites)?;
    list_u32(
        builder,
        &node,
        "proofEvidenceRefs",
        &node,
        &value.proof_evidence_refs,
    )?;
    list_iris(
        builder,
        &node,
        "dependencies",
        &node,
        &value.dependency_iris,
    )?;
    let parameters = structured_list(
        builder,
        &node,
        "parameters",
        &node,
        value.parameters.len(),
        "Parameter",
    )?;
    for (entry, parameter) in parameters.iter().zip(&value.parameters) {
        builder.text(entry, &field("name"), &parameter.name)?;
        builder.text(entry, &field("description"), &parameter.description)?;
        builder.boolean(entry, &field("required"), parameter.required)?;
    }
    let steps = structured_list(
        builder,
        &node,
        "steps",
        &node,
        value.steps.len(),
        "RunbookStep",
    )?;
    for (entry, step) in steps.iter().zip(&value.steps) {
        builder.integer(entry, &field("declaredOrdinal"), step.ordinal as i64)?;
        builder.text(entry, &field("action"), &step.action)?;
        builder.text(entry, &field("expectedResult"), &step.expected_result)?;
        builder.text(entry, &field("verification"), &step.verification)?;
    }
    Ok(())
}

fn protocol(builder: &mut Builder, revision: &str, value: &KnowledgeProtocolSection) -> Result<()> {
    let node = section_node(builder, revision, "protocol", "ProtocolSection")?;
    builder.iri(&node, &field("specification"), &value.specification_uri)?;
    builder.text(
        &node,
        &field("specificationVersion"),
        &value.specification_version,
    )?;
    builder.text(
        &node,
        &field("observationBounds"),
        &value.observation_bounds,
    )?;
    list_text(
        builder,
        &node,
        "providerScope",
        &node,
        &value.provider_scope,
    )?;
    list_text(builder, &node, "networkScope", &node, &value.network_scope)?;
    list_text(builder, &node, "capabilities", &node, &value.capabilities)?;
    list_text(
        builder,
        &node,
        "compatibilityConstraints",
        &node,
        &value.compatibility_constraints,
    )?;
    list_text(
        builder,
        &node,
        "negativeStatesAndQuirks",
        &node,
        &value.negative_states_and_quirks,
    )?;
    let assertions = structured_list(
        builder,
        &node,
        "assertions",
        &node,
        value.assertions.len(),
        "ProtocolAssertion",
    )?;
    for (entry, assertion) in assertions.iter().zip(&value.assertions) {
        builder.text(entry, &field("statement"), &assertion.statement)?;
        builder.boolean(entry, &field("observed"), assertion.observed)?;
        list_u32(
            builder,
            entry,
            "evidenceRefs",
            entry,
            &assertion.evidence_refs,
        )?;
    }
    Ok(())
}

fn devops(builder: &mut Builder, revision: &str, value: &KnowledgeDevopsSection) -> Result<()> {
    let node = section_node(builder, revision, "devops", "DevopsSection")?;
    builder.text(
        &node,
        &field("configurationCustody"),
        &value.configuration_custody,
    )?;
    list_iris(builder, &node, "assets", &node, &value.asset_iris)?;
    list_iris(
        builder,
        &node,
        "environments",
        &node,
        &value.environment_iris,
    )?;
    list_text(
        builder,
        &node,
        "topologyLinks",
        &node,
        &value.topology_links,
    )?;
    list_text(builder, &node, "ownership", &node, &value.ownership)?;
    list_text(
        builder,
        &node,
        "configurationRefs",
        &node,
        &value.configuration_refs,
    )?;
    list_text(
        builder,
        &node,
        "deploymentSurfaces",
        &node,
        &value.deployment_surfaces,
    )?;
    let observations = structured_list(
        builder,
        &node,
        "observations",
        &node,
        value.observations.len(),
        "DatedObservation",
    )?;
    for (entry, observation) in observations.iter().zip(&value.observations) {
        builder.datetime(entry, &field("observedAt"), &observation.observed_at)?;
        builder.text(entry, &field("status"), &observation.status)?;
        list_text(builder, entry, "limits", entry, &observation.limits)?;
        list_u32(
            builder,
            entry,
            "evidenceRefs",
            entry,
            &observation.evidence_refs,
        )?;
    }
    Ok(())
}

fn operations(
    builder: &mut Builder,
    revision: &str,
    value: &KnowledgeOperationsSection,
) -> Result<()> {
    let node = section_node(builder, revision, "operations", "OperationsSection")?;
    builder.text(&node, &field("operatingPurpose"), &value.operating_purpose)?;
    builder.text(&node, &field("cadence"), &value.cadence)?;
    list_text(builder, &node, "handoffs", &node, &value.handoffs)?;
    list_text(builder, &node, "escalation", &node, &value.escalation)?;
    list_text(
        builder,
        &node,
        "statusSemantics",
        &node,
        &value.status_semantics,
    )?;
    list_text(
        builder,
        &node,
        "signalSources",
        &node,
        &value.signal_sources,
    )?;
    list_text(builder, &node, "exceptions", &node, &value.exceptions)?;
    list_text(
        builder,
        &node,
        "ownershipGaps",
        &node,
        &value.ownership_gaps,
    )?;
    let roles = structured_list(
        builder,
        &node,
        "roles",
        &node,
        value.roles.len(),
        "RoleResponsibility",
    )?;
    for (entry, role) in roles.iter().zip(&value.roles) {
        builder.text(entry, &field("role"), &role.role)?;
        list_text(
            builder,
            entry,
            "responsibilities",
            entry,
            &role.responsibilities,
        )?;
    }
    let metrics = structured_list(
        builder,
        &node,
        "metrics",
        &node,
        value.metrics.len(),
        "MetricDefinition",
    )?;
    for (entry, metric) in metrics.iter().zip(&value.metrics) {
        builder.text(entry, &field("name"), &metric.name)?;
        builder.text(entry, &field("meaning"), &metric.meaning)?;
        builder.text(entry, &field("objective"), &metric.objective)?;
        builder.text(entry, &field("signalSource"), &metric.signal_source)?;
    }
    Ok(())
}

fn product_research(
    builder: &mut Builder,
    revision: &str,
    value: &KnowledgeProductResearchSection,
) -> Result<()> {
    let node = section_node(
        builder,
        revision,
        "product_research",
        "ProductResearchSection",
    )?;
    builder.text(&node, &field("question"), &value.question)?;
    list_text(builder, &node, "assumptions", &node, &value.assumptions)?;
    list_text(builder, &node, "segments", &node, &value.segments)?;
    list_text(builder, &node, "alternatives", &node, &value.alternatives)?;
    list_text(
        builder,
        &node,
        "conclusionsAndDecisions",
        &node,
        &value.conclusions_and_decisions,
    )?;
    list_text(
        builder,
        &node,
        "observationLimits",
        &node,
        &value.observation_limits,
    )?;
    list_text(
        builder,
        &node,
        "negativeEvidence",
        &node,
        &value.negative_evidence,
    )?;
    let evidence = structured_list(
        builder,
        &node,
        "evidenceMap",
        &node,
        value.evidence_map.len(),
        "ResearchEvidence",
    )?;
    for (entry, item) in evidence.iter().zip(&value.evidence_map) {
        builder.text(entry, &field("claim"), &item.claim)?;
        builder.boolean(entry, &field("synthetic"), item.synthetic)?;
        list_u32(builder, entry, "sourceRefs", entry, &item.source_refs)?;
    }
    Ok(())
}

fn security(builder: &mut Builder, revision: &str, value: &KnowledgeSecuritySection) -> Result<()> {
    let node = section_node(builder, revision, "security", "SecuritySection")?;
    list_iris(builder, &node, "assets", &node, &value.asset_iris)?;
    list_text(
        builder,
        &node,
        "trustBoundaries",
        &node,
        &value.trust_boundaries,
    )?;
    list_text(builder, &node, "threats", &node, &value.threats)?;
    list_text(builder, &node, "controls", &node, &value.controls)?;
    list_u32(builder, &node, "evidenceRefs", &node, &value.evidence_refs)?;
    builder.text(
        &node,
        &field("verificationStatus"),
        &value.verification_status,
    )?;
    builder.iri(
        &node,
        &field("sensitivity"),
        &enum_iri("sensitivity", value.sensitivity)?,
    )?;
    builder.text(
        &node,
        &field("applicableAuthority"),
        &value.applicable_authority,
    )?;
    list_text(builder, &node, "exceptions", &node, &value.exceptions)?;
    builder.text(&node, &field("findingState"), &value.finding_state)?;
    list_u32(
        builder,
        &node,
        "remediationProofRefs",
        &node,
        &value.remediation_proof_refs,
    )
}
