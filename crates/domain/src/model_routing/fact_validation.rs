use crate::{Error, Result};
use sha2::Sha256;

use super::{
    ModelRouteFact, ModelRouteFactProvenance, ModelRouteWorkContext, number, part, valid_ref,
};

pub(super) fn available_work_fact<T>(fact: &ModelRouteFact<T>) -> bool {
    matches!(
        fact,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. }
                | ModelRouteFactProvenance::ConfirmedWorkRequirement { .. }
                | ModelRouteFactProvenance::OperatingEvidence { .. },
            ..
        }
    )
}

pub(super) fn available_numeric_fact<T>(fact: &ModelRouteFact<T>) -> bool {
    matches!(
        fact,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. }
                | ModelRouteFactProvenance::OperatingEvidence { .. },
            ..
        }
    )
}

pub(super) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

impl<T> ModelRouteFact<T> {
    pub(super) fn provenance(&self) -> Option<&ModelRouteFactProvenance> {
        match self {
            Self::Known { provenance, .. } => Some(provenance),
            Self::Unknown => None,
        }
    }
}

pub(super) fn validate_host_fact<T>(fact: &ModelRouteFact<T>) -> Result<()> {
    match fact {
        ModelRouteFact::Unknown => Ok(()),
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Host { evidence_ref },
            ..
        } if valid_ref(evidence_ref) => Ok(()),
        _ => Err(Error::InvalidArguments),
    }
}

pub(super) fn validate_work_fact<T>(
    fact: &ModelRouteFact<T>,
    work: &ModelRouteWorkContext,
) -> Result<()> {
    validate_non_host_fact(fact, work, true)
}

pub(super) fn validate_operating_fact<T>(
    fact: &ModelRouteFact<T>,
    work: &ModelRouteWorkContext,
) -> Result<()> {
    validate_non_host_fact(fact, work, false)
}

fn validate_non_host_fact<T>(
    fact: &ModelRouteFact<T>,
    work: &ModelRouteWorkContext,
    declaration_allowed: bool,
) -> Result<()> {
    let link = &work.selection_link;
    let valid = match fact {
        ModelRouteFact::Unknown => true,
        ModelRouteFact::Known {
            provenance:
                ModelRouteFactProvenance::Caller {
                    source_ref,
                    work_node_id,
                    work_node_revision,
                },
            ..
        } => {
            valid_ref(source_ref)
                && *work_node_id == link.mapped_work_node_id
                && *work_node_revision == link.mapped_work_node_revision
        }
        ModelRouteFact::Known {
            provenance:
                ModelRouteFactProvenance::ConfirmedWorkRequirement {
                    frozen_snapshot_id,
                    requirements_semantic_digest,
                    source_ref,
                    work_node_id,
                    work_node_revision,
                },
            ..
        } => {
            declaration_allowed
                && valid_ref(source_ref)
                && *work_node_id == link.mapped_work_node_id
                && *work_node_revision == link.mapped_work_node_revision
                && work.context_authority.as_ref().is_some_and(|authority| {
                    *frozen_snapshot_id == authority.frozen_snapshot_id
                        && requirements_semantic_digest == &authority.requirements_semantic_digest
                })
        }
        ModelRouteFact::Known {
            provenance:
                ModelRouteFactProvenance::OperatingEvidence {
                    source_ref,
                    content_digest,
                    observed_at_epoch_ms,
                    expires_at_epoch_ms,
                    work_node_id,
                    work_node_revision,
                },
            ..
        } => {
            valid_ref(source_ref)
                && valid_sha256(content_digest)
                && *observed_at_epoch_ms >= 0
                && *expires_at_epoch_ms > *observed_at_epoch_ms
                && *work_node_id == link.mapped_work_node_id
                && *work_node_revision == link.mapped_work_node_revision
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidArguments)
    }
}

pub(super) fn hash_fact<T>(
    hash: &mut Sha256,
    fact: &ModelRouteFact<T>,
    value_hash: impl FnOnce(&mut Sha256, &T),
) {
    match fact {
        ModelRouteFact::Unknown => part(hash, "unknown"),
        ModelRouteFact::Known { value, provenance } => {
            part(hash, "known");
            match provenance {
                ModelRouteFactProvenance::Caller {
                    source_ref,
                    work_node_id,
                    work_node_revision,
                } => {
                    part(hash, "caller");
                    part(hash, source_ref);
                    part(hash, &work_node_id.to_string());
                    number(hash, *work_node_revision as u64);
                }
                ModelRouteFactProvenance::Host { evidence_ref } => {
                    part(hash, "host");
                    part(hash, evidence_ref);
                }
                ModelRouteFactProvenance::ConfirmedWorkRequirement {
                    frozen_snapshot_id,
                    requirements_semantic_digest,
                    source_ref,
                    work_node_id,
                    work_node_revision,
                } => {
                    part(hash, "confirmed_requirement");
                    part(hash, &frozen_snapshot_id.to_string());
                    part(hash, requirements_semantic_digest);
                    part(hash, source_ref);
                    part(hash, &work_node_id.to_string());
                    number(hash, *work_node_revision as u64);
                }
                ModelRouteFactProvenance::OperatingEvidence {
                    source_ref,
                    content_digest,
                    observed_at_epoch_ms,
                    expires_at_epoch_ms,
                    work_node_id,
                    work_node_revision,
                } => {
                    part(hash, "operating_evidence");
                    part(hash, source_ref);
                    part(hash, content_digest);
                    part(hash, &observed_at_epoch_ms.to_string());
                    part(hash, &expires_at_epoch_ms.to_string());
                    part(hash, &work_node_id.to_string());
                    number(hash, *work_node_revision as u64);
                }
            }
            value_hash(hash, value);
        }
    }
}
