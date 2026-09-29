use serde::{Deserialize, Serialize};
use tect_domain::KnowledgeAccessScope;

pub(super) const OPERATIONAL_SCHEMA_VERSION: u32 = 2;
const OPERATIONAL_MAX_ASSERTIONS: usize = 128;
const OPERATIONAL_MAX_SOURCE_REFS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OperationalEntityKind {
    TaxonomyConcept,
    KnowledgeResource,
    ProjectContext,
    Environment,
    DeploymentSurface,
    Host,
    AccessRoute,
    NetworkEndpoint,
    CredentialLocator,
    AccessProcedure,
}

/// A preferred taxonomy label or alias in a specific locale. The original
/// `label`/`locale`/`aliases` fields remain the default-locale spelling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalizedTaxonomyLabel {
    pub locale: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum OperationalEntityDraft {
    TaxonomyConcept {
        iri: String,
        label: String,
        locale: String,
        aliases: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        preferred_labels: Vec<LocalizedTaxonomyLabel>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        localized_aliases: Vec<LocalizedTaxonomyLabel>,
    },
    KnowledgeResource {
        iri: String,
    },
    ProjectContext {
        iri: String,
        label: String,
        custodian_ref: String,
    },
    Environment {
        iri: String,
        label: String,
        environment_kind: EnvironmentKind,
        project_context_iri: String,
    },
    DeploymentSurface {
        iri: String,
        label: String,
        environment_iri: String,
        surface_kind: String,
        declared_state: String,
        current_state: String,
    },
    Host {
        iri: String,
        label: String,
        identity_evidence_refs: Vec<u32>,
        provider_instance_id: Option<String>,
    },
    AccessRoute {
        iri: String,
        purpose: AccessPurpose,
        origin_iri: String,
        observed_at: String,
        review_due_at: String,
    },
    NetworkEndpoint {
        iri: String,
        endpoint_kind: EndpointKind,
        locator: String,
        observed_at: String,
        review_due_at: String,
    },
    CredentialLocator {
        iri: String,
        locator_kind: CredentialLocatorKind,
        locator: String,
        custodian_ref: String,
        observed_at: String,
        review_due_at: String,
    },
    AccessProcedure {
        iri: String,
        purpose: AccessPurpose,
        target_iri: String,
        target_kind: OperationalEntityKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EnvironmentKind {
    Production,
    Staging,
    Development,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AccessPurpose {
    WebIngress,
    HostSsh,
    PersonalQa,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EndpointKind {
    Ipv4,
    Ipv6,
    Dns,
    Mesh,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CredentialLocatorKind {
    VaultPath,
    LocalKeyRef,
    IdentityName,
    RequestRoute,
}

impl OperationalEntityDraft {
    pub(super) fn kind(&self) -> OperationalEntityKind {
        match self {
            Self::TaxonomyConcept { .. } => OperationalEntityKind::TaxonomyConcept,
            Self::KnowledgeResource { .. } => OperationalEntityKind::KnowledgeResource,
            Self::ProjectContext { .. } => OperationalEntityKind::ProjectContext,
            Self::Environment { .. } => OperationalEntityKind::Environment,
            Self::DeploymentSurface { .. } => OperationalEntityKind::DeploymentSurface,
            Self::Host { .. } => OperationalEntityKind::Host,
            Self::AccessRoute { .. } => OperationalEntityKind::AccessRoute,
            Self::NetworkEndpoint { .. } => OperationalEntityKind::NetworkEndpoint,
            Self::CredentialLocator { .. } => OperationalEntityKind::CredentialLocator,
            Self::AccessProcedure { .. } => OperationalEntityKind::AccessProcedure,
        }
    }

    pub(super) fn iri(&self) -> &str {
        match self {
            Self::TaxonomyConcept { iri, .. }
            | Self::KnowledgeResource { iri }
            | Self::ProjectContext { iri, .. }
            | Self::Environment { iri, .. }
            | Self::DeploymentSurface { iri, .. }
            | Self::Host { iri, .. }
            | Self::AccessRoute { iri, .. }
            | Self::NetworkEndpoint { iri, .. }
            | Self::CredentialLocator { iri, .. }
            | Self::AccessProcedure { iri, .. } => iri,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OperationalPredicate {
    HasEnvironment,
    HasDeploymentSurface,
    TargetsHost,
    RunsOnHost,
    HasEndpoint,
    ReachableThrough,
    UsesConnectorHost,
    AccessVia,
    AppliesToRoute,
    UsesCredentialLocator,
    RelatedToProjectContext,
    ReplacesHost,
    SupportedBy,
    BroaderConcept,
    ClassifiedAs,
    AppliesTo,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(super) struct OperationalPredicateRule {
    pub subject: &'static [OperationalEntityKind],
    pub object: &'static [OperationalEntityKind],
    pub inverse_label: &'static str,
    pub singular_current: bool,
    pub dynamic: bool,
}

impl OperationalPredicate {
    /// Canonical local name in the versioned operational RDF vocabulary.
    pub(super) const fn rdf_local_name(self) -> &'static str {
        match self {
            Self::HasEnvironment => "hasEnvironment",
            Self::HasDeploymentSurface => "hasDeploymentSurface",
            Self::TargetsHost => "targetsHost",
            Self::RunsOnHost => "runsOnHost",
            Self::HasEndpoint => "hasEndpoint",
            Self::ReachableThrough => "reachableThrough",
            Self::UsesConnectorHost => "usesConnectorHost",
            Self::AccessVia => "accessVia",
            Self::AppliesToRoute => "appliesToRoute",
            Self::UsesCredentialLocator => "usesCredentialLocator",
            Self::RelatedToProjectContext => "relatedToProjectContext",
            Self::ReplacesHost => "replacesHost",
            Self::SupportedBy => "supportedBy",
            Self::BroaderConcept => "broaderConcept",
            Self::ClassifiedAs => "classifiedAs",
            Self::AppliesTo => "appliesTo",
        }
    }

    pub(super) const fn rule(self) -> OperationalPredicateRule {
        use OperationalEntityKind as K;
        match self {
            Self::HasEnvironment => OperationalPredicateRule {
                subject: &[K::ProjectContext],
                object: &[K::Environment],
                inverse_label: "environment of",
                singular_current: false,
                dynamic: false,
            },
            Self::HasDeploymentSurface => OperationalPredicateRule {
                subject: &[K::Environment],
                object: &[K::DeploymentSurface],
                inverse_label: "surface of",
                singular_current: false,
                dynamic: false,
            },
            Self::TargetsHost => OperationalPredicateRule {
                subject: &[K::DeploymentSurface],
                object: &[K::Host],
                inverse_label: "targeted by",
                singular_current: false,
                dynamic: true,
            },
            Self::RunsOnHost => OperationalPredicateRule {
                subject: &[K::DeploymentSurface],
                object: &[K::Host],
                inverse_label: "runs surface",
                singular_current: false,
                dynamic: true,
            },
            Self::HasEndpoint => OperationalPredicateRule {
                subject: &[K::Host, K::DeploymentSurface],
                object: &[K::NetworkEndpoint],
                inverse_label: "endpoint of",
                singular_current: false,
                dynamic: true,
            },
            Self::ReachableThrough => OperationalPredicateRule {
                subject: &[K::Host],
                object: &[K::AccessRoute],
                inverse_label: "reaches",
                singular_current: false,
                dynamic: true,
            },
            Self::UsesConnectorHost => OperationalPredicateRule {
                subject: &[K::AccessRoute],
                object: &[K::Host],
                inverse_label: "connector for",
                singular_current: false,
                dynamic: true,
            },
            Self::AccessVia => OperationalPredicateRule {
                subject: &[K::Host],
                object: &[K::AccessProcedure],
                inverse_label: "accesses",
                singular_current: false,
                dynamic: true,
            },
            Self::AppliesToRoute => OperationalPredicateRule {
                subject: &[K::AccessProcedure],
                object: &[K::AccessRoute],
                inverse_label: "has procedure",
                singular_current: false,
                dynamic: true,
            },
            Self::UsesCredentialLocator => OperationalPredicateRule {
                subject: &[K::AccessProcedure],
                object: &[K::CredentialLocator],
                inverse_label: "used by procedure",
                singular_current: false,
                dynamic: true,
            },
            Self::RelatedToProjectContext => OperationalPredicateRule {
                subject: &[K::Host, K::AccessRoute, K::AccessProcedure],
                object: &[K::ProjectContext],
                inverse_label: "related resource",
                singular_current: false,
                dynamic: false,
            },
            Self::ReplacesHost => OperationalPredicateRule {
                subject: &[K::Host],
                object: &[K::Host],
                inverse_label: "replaced by",
                singular_current: false,
                dynamic: false,
            },
            Self::SupportedBy => OperationalPredicateRule {
                subject: &[
                    K::ProjectContext,
                    K::Environment,
                    K::DeploymentSurface,
                    K::Host,
                    K::AccessRoute,
                    K::NetworkEndpoint,
                    K::CredentialLocator,
                    K::AccessProcedure,
                ],
                object: &[K::AccessProcedure],
                inverse_label: "supports",
                singular_current: false,
                dynamic: false,
            },
            Self::BroaderConcept => OperationalPredicateRule {
                subject: &[K::TaxonomyConcept],
                object: &[K::TaxonomyConcept],
                inverse_label: "narrower concept",
                singular_current: false,
                dynamic: false,
            },
            Self::ClassifiedAs => OperationalPredicateRule {
                subject: &[
                    K::KnowledgeResource,
                    K::ProjectContext,
                    K::Environment,
                    K::DeploymentSurface,
                    K::Host,
                    K::AccessRoute,
                    K::NetworkEndpoint,
                    K::CredentialLocator,
                    K::AccessProcedure,
                ],
                object: &[K::TaxonomyConcept],
                inverse_label: "classified resource",
                singular_current: false,
                dynamic: false,
            },
            Self::AppliesTo => OperationalPredicateRule {
                subject: &[K::KnowledgeResource],
                object: &[
                    K::ProjectContext,
                    K::Host,
                    K::Environment,
                    K::DeploymentSurface,
                ],
                inverse_label: "applicable knowledge",
                singular_current: false,
                dynamic: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OperationalAssertionState {
    Accepted,
    Historical,
    Proposed,
    Retracted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OperationalSourceRef {
    pub source_index: u32,
    pub fragment_iri: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OperationalAssertionDraft {
    pub subject_iri: String,
    pub subject_kind: OperationalEntityKind,
    pub predicate: OperationalPredicate,
    pub object_iri: String,
    pub object_kind: OperationalEntityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_purpose: Option<AccessPurpose>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procedure_purpose: Option<AccessPurpose>,
    pub state: OperationalAssertionState,
    pub source_refs: Vec<OperationalSourceRef>,
    pub reviewer_ref: String,
    pub review_receipt: String,
    pub authority_basis: String,
    pub access_scope: KnowledgeAccessScope,
    pub asserted_at: String,
    pub observed_at: Option<String>,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub review_due_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationalReferencesDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) entity: Option<OperationalEntityDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) entity_access_scope: Option<KnowledgeAccessScope>,
    pub(super) entity_source_refs: Vec<OperationalSourceRef>,
    pub(super) assertions: Vec<OperationalAssertionDraft>,
}

impl OperationalReferencesDraft {
    pub(super) fn validate_shape(&self, source_count: usize) -> bool {
        let valid_sources = |refs: &[OperationalSourceRef]| {
            refs.len() <= OPERATIONAL_MAX_SOURCE_REFS
                && refs.iter().all(|reference| {
                    (reference.source_index as usize) < source_count
                        && !reference.fragment_iri.trim().is_empty()
                        && reference.sha256.len() == 64
                        && reference
                            .sha256
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit())
                })
        };
        if self.entity.is_none() && self.assertions.is_empty()
            || self.assertions.len() > OPERATIONAL_MAX_ASSERTIONS
            || self.entity.is_some() == self.entity_source_refs.is_empty()
            || !valid_sources(&self.entity_source_refs)
        {
            return false;
        }
        self.assertions.iter().all(|assertion| {
            let rule = assertion.predicate.rule();
            !assertion.source_refs.is_empty()
                && valid_sources(&assertion.source_refs)
                && rule.subject.contains(&assertion.subject_kind)
                && rule.object.contains(&assertion.object_kind)
                && self.entity.as_ref().is_none_or(|entity| {
                    entity.iri() == assertion.subject_iri && entity.kind() == assertion.subject_kind
                })
        })
    }
}
