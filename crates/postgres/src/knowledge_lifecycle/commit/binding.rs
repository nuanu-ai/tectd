use super::*;

type SliceDefinitionRow = (String, Option<String>, Option<String>, Option<String>);

pub(super) async fn insert_bindings(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    bindings: &[KnowledgeDocumentBinding],
    pins: &[KnowledgeResolvedBindingPin],
) -> Result<()> {
    for (index, binding) in bindings.iter().enumerate() {
        let pin = pins.iter().find(|pin| pin.binding_index == index as u32);
        validate_binding_target(tx, tenant, workspace, unit, revision, binding, pin).await?;
        let (kind, program, scope, slice, phase) = match &binding.target {
            KnowledgeBindingTarget::Workspace => ("workspace", None, None, None, None),
            KnowledgeBindingTarget::Program { program_id } => {
                ("program", Some(*program_id), None, None, None)
            }
            KnowledgeBindingTarget::Scope { scope_id } => {
                ("scope", None, Some(*scope_id), None, None)
            }
            KnowledgeBindingTarget::Slice { scope_id, slice_id } => {
                ("slice", None, Some(*scope_id), Some(*slice_id), None)
            }
            KnowledgeBindingTarget::SlicePhase {
                scope_id,
                slice_id,
                phase_id,
            } => (
                "slice_phase",
                None,
                Some(*scope_id),
                Some(*slice_id),
                Some(phase_id.clone()),
            ),
        };
        let (resolution, pinned) = match binding.version_resolution {
            KnowledgeBindingVersion::CurrentAccepted => ("current_accepted", None),
            KnowledgeBindingVersion::PinnedRevision { revision } => {
                ("pinned_revision", Some(revision))
            }
        };
        let (definition_kind, definition_version, definition_digest) = pin
            .map(|pin| {
                (
                    Some(pin.definition_kind.as_str()),
                    Some(pin.definition_version.as_str()),
                    Some(pin.definition_digest.as_str()),
                )
            })
            .unwrap_or((None, None, None));
        sqlx::query("INSERT INTO knowledge_bindings(tenant_id,workspace_id,unit_id,revision,binding_kind,program_id,scope_id,slice_id,phase_id,definition_kind,definition_version,definition_digest,purpose,version_resolution,pinned_revision,contract_version,active) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,'dk-2',true) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(unit).bind(revision).bind(kind).bind(program).bind(scope).bind(slice).bind(phase)
            .bind(definition_kind).bind(definition_version).bind(definition_digest)
            .bind(enum_text(&binding.purpose)?).bind(resolution).bind(pinned).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}

async fn validate_binding_target(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    binding: &KnowledgeDocumentBinding,
    pin: Option<&KnowledgeResolvedBindingPin>,
) -> Result<()> {
    let exists = match &binding.target {
        KnowledgeBindingTarget::Workspace => true,
        KnowledgeBindingTarget::Program{program_id} => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM programs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)").bind(tenant).bind(workspace).bind(program_id).fetch_one(&mut **tx).await.map_err(storage_error)?,
        KnowledgeBindingTarget::Scope{scope_id} => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)").bind(tenant).bind(workspace).bind(scope_id).fetch_one(&mut **tx).await.map_err(storage_error)?,
        KnowledgeBindingTarget::Slice{scope_id,slice_id} => sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND id=$4)").bind(tenant).bind(workspace).bind(scope_id).bind(slice_id).fetch_one(&mut **tx).await.map_err(storage_error)?,
        KnowledgeBindingTarget::SlicePhase{scope_id,slice_id,phase_id} => {
            let pin = pin.ok_or(Error::InvalidArguments)?;
            let row:Option<SliceDefinitionRow>=sqlx::query_as("SELECT s.pipeline,r.definition_kind,r.definition_version,r.definition_digest FROM native_slices s LEFT JOIN slice_pipeline_runs r ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id AND r.scope_id=s.scope_id AND r.slice_id=s.id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.scope_id=$3 AND s.id=$4")
                .bind(tenant).bind(workspace).bind(scope_id).bind(slice_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
            let Some((pipeline, run_kind, run_version, run_digest)) = row else { return Err(Error::InvalidArguments) };
            if pipeline != pin.definition_kind.as_str() || pin.phase_id != *phase_id {
                false
            } else if let (Some(kind),Some(version),Some(digest))=(run_kind,run_version,run_digest) {
                kind == pin.definition_kind.as_str()
                    && version == pin.definition_version
                    && digest == pin.definition_digest
            } else {
                true
            }
        },
    };
    if !exists {
        return Err(Error::InvalidArguments);
    }
    if let KnowledgeBindingVersion::PinnedRevision { revision: pinned } = binding.version_resolution
    {
        let eligible=pinned==revision || sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=$4 AND NOT payload_erased)").bind(tenant).bind(workspace).bind(unit).bind(pinned).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !eligible {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}
