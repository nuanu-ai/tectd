use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tect_application::{
    MatrixPlanningEffectStore, MatrixRequirementsContextStore, MatrixRequirementsLocator,
};
use tect_domain::{
    AntiBloatObligationOrigin, AntiBloatProtectedObligation, MATRIX_REQUIREMENTS_SCHEMA,
    RequirementsAnchor, anti_bloat_protected_obligations_digest, resolve_matrix_requirements,
};

fn content_digest(value: &impl serde::Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(digest(&bytes))
}

fn add_obligation(
    obligations: &mut BTreeMap<String, AntiBloatProtectedObligation>,
    obligation: AntiBloatProtectedObligation,
) -> Result<()> {
    match obligations.get(&obligation.id) {
        Some(existing) if existing == &obligation => Ok(()),
        Some(_) => Err(Error::InputConflict),
        None => {
            obligations.insert(obligation.id.clone(), obligation);
            Ok(())
        }
    }
}

async fn add_declarations(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    locator: &MatrixRequirementsLocator,
    scope_candidate_id: Option<Uuid>,
    obligations: &mut BTreeMap<String, AntiBloatProtectedObligation>,
) -> Result<()> {
    let principal = uow.principal_id()?;
    let lineage = uow
        .matrix_requirements_lineage(workspace, principal, locator, false)
        .await?;
    let revisions = uow
        .matrix_requirements_revisions(workspace, &lineage)
        .await?;
    let effective = resolve_matrix_requirements(&lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)?;
    for (path, resolved) in effective.values() {
        let anchor = serde_json::to_string(&resolved.source.anchor).map_err(storage_error)?;
        let item = AntiBloatProtectedObligation {
            id: format!("matrix-declaration:{anchor}:{}", path.fact_path()),
            content_digest: content_digest(&resolved.value)?,
            origin: AntiBloatObligationOrigin::MatrixDeclaration,
            scope_candidate_id: match resolved.source.anchor {
                RequirementsAnchor::Program { .. } => None,
                _ => scope_candidate_id,
            },
        };
        add_obligation(obligations, item)?;
    }
    Ok(())
}

pub(crate) async fn protected_obligations(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    manifest: &ScopeConstructorManifest,
    saved: &ResolvedCandidateDraft,
    verifier_readback: bool,
) -> Result<Vec<AntiBloatProtectedObligation>> {
    let mut obligations = BTreeMap::new();
    for source in &manifest.obligations {
        add_obligation(
            &mut obligations,
            AntiBloatProtectedObligation {
                id: format!("scope-ref:{}", source.id),
                content_digest: source.statement_digest.clone(),
                origin: AntiBloatObligationOrigin::ScopeSource,
                scope_candidate_id: None,
            },
        )?;
    }
    add_declarations(
        uow,
        workspace,
        &MatrixRequirementsLocator::Program {
            program_id: manifest.source.program_id,
        },
        None,
        &mut obligations,
    )
    .await?;
    let tenant = uow.tenant_id()?;
    let descendants: Vec<(Uuid, Uuid, i64, Option<Uuid>)> = sqlx::query_as(
        "SELECT id,source_candidate_id,source_candidate_set_revision,slice_candidate_set_id \
         FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND source_candidate_set_id=$3 \
         ORDER BY id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest.source.candidate_set_id)
    .fetch_all(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let candidate_ids = saved
        .candidates
        .iter()
        .map(|item| item.id)
        .collect::<BTreeSet<_>>();
    for (scope_id, candidate_id, source_revision, slice_set) in descendants {
        if !candidate_ids.contains(&candidate_id)
            || source_revision > manifest.source.candidate_set_revision + 1
        {
            return Err(Error::InputConflict);
        }
        add_obligation(
            &mut obligations,
            AntiBloatProtectedObligation {
                id: format!("native-scope:{scope_id}"),
                content_digest: content_digest(&(
                    scope_id,
                    candidate_id,
                    source_revision,
                    slice_set,
                ))?,
                origin: AntiBloatObligationOrigin::NativeScope,
                scope_candidate_id: Some(candidate_id),
            },
        )?;
        add_declarations(
            uow,
            workspace,
            &MatrixRequirementsLocator::Scope {
                program_id: manifest.source.program_id,
                scope_id,
            },
            Some(candidate_id),
            &mut obligations,
        )
        .await?;
        if let Some(slice_set) = slice_set {
            add_selected_effects(
                uow,
                workspace,
                scope_id,
                candidate_id,
                slice_set,
                &mut obligations,
                verifier_readback,
            )
            .await?;
        }
    }
    Ok(obligations.into_values().collect())
}

mod selected_effects;
use selected_effects::add_selected_effects;

pub(super) async fn provider_profile_matches(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    profile: &str,
) -> Result<bool> {
    let tenant = uow.tenant_id()?;
    // SELECT-only: preserves existing runtime grants/RLS and read-only recovery.
    let selected: Option<String> = sqlx::query_scalar("SELECT provider_profile_ref FROM public.advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 AND mode='optional'")
        .bind(tenant).bind(workspace).fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?.flatten();
    Ok(selected.as_deref() == Some(profile))
}

pub(super) async fn advisory_mode(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
) -> Result<WorkspaceAdvisoryMode> {
    let tenant = uow.tenant_id()?;
    let mode: Option<String> = sqlx::query_scalar(
        "SELECT mode FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    match mode.as_deref() {
        None | Some("disabled") => Ok(WorkspaceAdvisoryMode::Disabled),
        Some("optional") => Ok(WorkspaceAdvisoryMode::Optional),
        _ => Err(Error::InternalInvariant),
    }
}

pub(super) async fn authoritative_input(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    expected_revision: i64,
) -> Result<Option<AntiBloatInput>> {
    let tenant = uow.tenant_id()?;
    let row: Option<SelectedBindingRow> = sqlx::query_as(
        "SELECT m.aggregate_payload AS manifest_payload,dr.payload AS draft_payload, \
                b.obligation_links,b.non_goal_source_obligation_ids,b.mandatory_policy_obligation_ids, \
                b.dependency_digest,b.source_digest,b.provenance,s.revision AS set_revision, \
                s.current_snapshot_id,b.selected_draft_revision,b.selected_material_digest, \
                b.selected_alternative_id,b.selected_caller_link_id,b.selected_caller_request_id, \
                c.caller_request_id,r.result_revision AS receipt_revision, \
                d.selected_alternative_id AS disposition_alternative_id \
         FROM scope_anti_bloat_bindings b \
         JOIN advisory_scope_manifest m ON (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id)= \
             (b.tenant_id,b.workspace_id,b.opportunity_id,b.candidate_set_id) \
         JOIN scope_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)= \
             (b.tenant_id,b.workspace_id,b.candidate_set_id) \
         JOIN scope_candidate_drafts dr ON (dr.tenant_id,dr.workspace_id,dr.candidate_set_id,dr.set_revision)= \
             (b.tenant_id,b.workspace_id,b.candidate_set_id,b.selected_draft_revision) \
         JOIN advisory_scope_caller_link c ON (c.tenant_id,c.workspace_id,c.link_id)= \
             (b.tenant_id,b.workspace_id,b.selected_caller_link_id) \
         JOIN advisory_scope_disposition d ON (d.tenant_id,d.workspace_id,d.disposition_id)= \
             (c.tenant_id,c.workspace_id,c.disposition_id) \
         JOIN scope_candidate_receipts r ON (r.tenant_id,r.workspace_id,r.candidate_set_id,r.operation,r.request_id)= \
             (c.tenant_id,c.workspace_id,c.candidate_set_id,'save_draft',c.caller_request_id) \
         WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
           AND b.candidate_set_revision=$4 AND b.selected_draft_revision IS NOT NULL \
           AND c.caller_operation='save_draft'",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(expected_revision)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.set_revision != expected_revision
        || row.selected_draft_revision != expected_revision
        || row.caller_request_id != row.selected_caller_request_id
        || row.receipt_revision != expected_revision
        || row.disposition_alternative_id.as_deref() != Some(row.selected_alternative_id.as_str())
    {
        return Ok(None);
    }
    let manifest: ScopeConstructorManifest =
        serde_json::from_value(row.manifest_payload).map_err(storage_error)?;
    let selected_id = tect_domain::ScopeAlternativeId(row.selected_alternative_id);
    let selected = manifest
        .eligible(&selected_id)
        .ok_or(Error::InputConflict)?;
    let saved: ResolvedCandidateDraft =
        serde_json::from_value(row.draft_payload.ok_or(Error::InputConflict)?)
            .map_err(storage_error)?;
    if manifest.source.candidate_set_id != candidate_set_id
        || manifest.source.candidate_set_revision >= expected_revision
        || manifest.source.snapshot_id != row.current_snapshot_id.ok_or(Error::InputConflict)?
        || manifest.source.digest != row.source_digest
        || saved != selected.material
        || selected.material_digest != row.selected_material_digest
    {
        return Err(Error::InputConflict);
    }
    let expected_non_goal = crate::scope_advisory::trusted_non_goal_source_obligation_ids(
        uow.transaction()?,
        tenant,
        workspace_id,
        &manifest,
        saved.boundary,
    )
    .await?;
    let (expected_links, expected_dependency, base_provenance) =
        crate::scope_advisory::authored_graph_binding_for(
            &manifest,
            &selected_id,
            expected_revision,
            &expected_non_goal,
        )?;
    let expected_provenance = format!(
        "{base_provenance}:selected={}:caller={}:receipt={}",
        selected_id.0, row.selected_caller_link_id, row.selected_caller_request_id
    );
    if row.obligation_links != serde_json::to_value(expected_links).map_err(storage_error)?
        || row.non_goal_source_obligation_ids
            != serde_json::to_value(&expected_non_goal).map_err(storage_error)?
        || row.dependency_digest != expected_dependency
        || row.provenance != expected_provenance
        || row.mandatory_policy_obligation_ids != serde_json::json!([])
    {
        return Err(Error::InputConflict);
    }
    let protected_obligations =
        protected_obligations(uow, workspace_id, &manifest, &saved, false).await?;
    let protected_obligations_digest =
        anti_bloat_protected_obligations_digest(&Sha256ScopeDigest, &protected_obligations)?;
    let input = AntiBloatInput {
        selected_id,
        selected_revision: expected_revision,
        manifest,
        graph_provenance: row.provenance,
        dependency_digest: row.dependency_digest,
        obligation_links: serde_json::from_value::<Vec<AntiBloatObligationLink>>(
            row.obligation_links,
        )
        .map_err(storage_error)?,
        non_goal_source_obligation_ids: expected_non_goal,
        mandatory_policy_obligation_ids: serde_json::from_value(
            row.mandatory_policy_obligation_ids,
        )
        .map_err(storage_error)?,
        protected_obligations,
        protected_obligations_digest,
    };
    review_anti_bloat(&Sha256ScopeDigest, &input)?;
    Ok(Some(input))
}
