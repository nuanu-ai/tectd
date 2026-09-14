use crate::knowledge_lifecycle::rdf;
use crate::storage_error;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use tect_domain::*;
use uuid::Uuid;

fn json<T: Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn byte_digest(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn blocking(purpose: KnowledgeBindingPurpose) -> bool {
    purpose != KnowledgeBindingPurpose::Reference
}

fn purpose_rank(purpose: KnowledgeBindingPurpose) -> u8 {
    match purpose {
        KnowledgeBindingPurpose::Required => 0,
        KnowledgeBindingPurpose::Procedure => 1,
        KnowledgeBindingPurpose::ProofBasis => 2,
        KnowledgeBindingPurpose::Reference => 3,
    }
}

fn purpose_name(purpose: KnowledgeBindingPurpose) -> &'static str {
    match purpose {
        KnowledgeBindingPurpose::Required => "required",
        KnowledgeBindingPurpose::Reference => "reference",
        KnowledgeBindingPurpose::Procedure => "procedure",
        KnowledgeBindingPurpose::ProofBasis => "proof_basis",
    }
}

async fn require_manifest_access(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest: Uuid,
) -> Result<()> {
    let protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM planning_knowledge_manifests m \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 \
           AND (m.payload_erased OR pg_catalog.jsonb_array_length(COALESCE(m.selected,'[]'::jsonb))>0 \
             OR pg_catalog.jsonb_array_length(COALESCE(m.unresolved_needs,'[]'::jsonb))>0))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if protected {
        crate::durable_knowledge::require_identity_ready(tx).await?;
    }
    let restricted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM planning_knowledge_manifests m \
         CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)) item \
         JOIN knowledge_unit_heads h ON h.tenant_id=m.tenant_id AND h.workspace_id=m.workspace_id \
           AND h.unit_id=(item->>'unit_id')::uuid \
         JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id \
           AND r.unit_id=h.unit_id AND r.revision=(item->>'unit_revision')::bigint \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 \
           AND (h.access_scope='owners_only' OR r.access_scope='owners_only'))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if restricted {
        let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
            .bind(principal)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
        if !owner {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectorMatch {
    Applicable,
    NotApplicable,
    NeedsContext,
}

fn selector(expected: &[String], actual: Option<&[String]>) -> SelectorMatch {
    if expected.is_empty() {
        SelectorMatch::Applicable
    } else if let Some(actual) = actual {
        if expected.iter().any(|v| actual.contains(v)) {
            SelectorMatch::Applicable
        } else {
            SelectorMatch::NotApplicable
        }
    } else {
        SelectorMatch::NeedsContext
    }
}

fn applicable(
    brief: &PlanningBrief,
    context: &PlanningTaskContext,
) -> std::result::Result<bool, ()> {
    let dimensions = [
        selector(&brief.selectors.target_iris, context.target_iris.as_deref()),
        selector(
            &brief.selectors.environment_iris,
            context.environment_iris.as_deref(),
        ),
        selector(
            &brief.selectors.action_classes,
            context.action_classes.as_deref(),
        ),
    ];
    if dimensions.contains(&SelectorMatch::NotApplicable) {
        Ok(false)
    } else if dimensions.contains(&SelectorMatch::NeedsContext) {
        Err(())
    } else {
        Ok(true)
    }
}

fn stage_name(stage: PlanningStage) -> &'static str {
    stage.as_str()
}

pub(crate) async fn require_owned_payload_identity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    relations: &[&str],
    row: Option<Uuid>,
) -> Result<()> {
    let protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_owned_copies c \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name=ANY($3) \
           AND ($4::uuid IS NULL OR c.row_id=$4) AND NOT c.redacted)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(relations)
    .bind(row)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if protected {
        crate::durable_knowledge::require_identity_ready(tx).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
mod capture;
mod lineage;
mod status;

pub(crate) use capture::capture;
pub(crate) use lineage::{
    consumption_status, program_refresh_replay, register_consumption, register_manifest_lineage,
    register_receipt_copy, save_program_refresh_receipt,
};
pub(crate) use status::{require, status, status_for_manifest};

#[cfg(test)]
mod tests {
    use super::*;

    fn brief() -> PlanningBrief {
        PlanningBrief {
            local_id: "fixture".into(),
            stage: PlanningStage::Program,
            instruction: "fixture instruction".into(),
            conditions: Vec::new(),
            exceptions: Vec::new(),
            purpose: "fixture purpose".into(),
            selectors: PlanningBriefSelectors {
                target_iris: vec!["urn:fixture:r1".into(), "urn:fixture:r2".into()],
                environment_iris: vec!["urn:fixture:prod".into()],
                action_classes: vec!["deploy".into()],
            },
        }
    }

    #[test]
    fn selector_is_or_within_and_and_across_dimensions() {
        let context = PlanningTaskContext {
            target_iris: Some(vec!["urn:fixture:r2".into(), "urn:fixture:r3".into()]),
            environment_iris: Some(vec!["urn:fixture:prod".into()]),
            action_classes: Some(vec!["deploy".into(), "verify".into()]),
        };
        assert_eq!(applicable(&brief(), &context), Ok(true));
    }

    #[test]
    fn known_mismatch_wins_over_unknown_dimension() {
        let context = PlanningTaskContext {
            target_iris: None,
            environment_iris: Some(vec!["urn:fixture:staging".into()]),
            action_classes: Some(vec!["deploy".into()]),
        };
        assert_eq!(applicable(&brief(), &context), Ok(false));
    }

    #[test]
    fn unknown_constrained_dimension_needs_context() {
        let context = PlanningTaskContext {
            target_iris: None,
            environment_iris: Some(vec!["urn:fixture:prod".into()]),
            action_classes: Some(vec!["deploy".into()]),
        };
        assert_eq!(applicable(&brief(), &context), Err(()));
    }
}
