use super::*;

pub(super) async fn resolve(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    principal: Uuid,
    locator: &MatrixRequirementsLocator,
    for_write: bool,
) -> Result<Vec<RequirementsAnchor>> {
    if uow.principal_id()? != principal || for_write && (!uow.is_read_write() || !uow.is_owner()) {
        return Err(Error::Forbidden);
    }
    let tenant = uow.tenant_id()?;
    let member: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3)")
        .bind(tenant).bind(workspace).bind(principal).fetch_one(&mut **uow.transaction()?).await.map_err(storage_error)?;
    if !member {
        return Err(Error::Forbidden);
    }
    let mut opened = None;
    let direct = match locator {
        MatrixRequirementsLocator::OpenedSlice { slice_id } => {
            let row: Option<(Uuid,Uuid,i64,Uuid,Value)> = sqlx::query_as("SELECT scope_id,candidate_id,candidate_revision,opening_snapshot_id,origin_payload FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                .bind(tenant).bind(workspace).bind(slice_id).fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?;
            let (scope, work, revision, snapshot, payload) = row.ok_or(Error::NotFound)?;
            let origin: OpenSlice =
                serde_json::from_value(payload).map_err(|_| Error::InputConflict)?;
            if origin.scope_id != scope
                || origin.candidate_id != work
                || origin.candidate_revision != revision
                || origin.candidate_snapshot_id != snapshot
            {
                return Err(Error::InputConflict);
            }
            let sets: Vec<Uuid> = sqlx::query_scalar("SELECT p.candidate_set_id FROM slice_planning_snapshots p JOIN slice_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)=(p.tenant_id,p.workspace_id,p.candidate_set_id) WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.id=$3 AND s.scope_id=$4")
                .bind(tenant).bind(workspace).bind(snapshot).bind(scope).fetch_all(&mut **uow.transaction()?).await.map_err(storage_error)?;
            if sets != vec![origin.candidate_set_id] {
                return Err(Error::InputConflict);
            }
            let program: Uuid=sqlx::query_scalar("SELECT c.program_id FROM native_scopes n JOIN scope_candidate_sets c ON (c.tenant_id,c.workspace_id,c.id)=(n.tenant_id,n.workspace_id,n.source_candidate_set_id) WHERE n.tenant_id=$1 AND n.workspace_id=$2 AND n.id=$3")
                .bind(tenant).bind(workspace).bind(scope).fetch_one(&mut **uow.transaction()?).await.map_err(storage_error)?;
            opened = Some(origin.candidate_set_revision);
            MatrixRequirementsLocator::Slice {
                program_id: program,
                scope_id: scope,
                candidate_set_id: origin.candidate_set_id,
                work_candidate_id: work,
                expected_work_revision: revision,
            }
        }
        other => other.clone(),
    };
    let (program, scope, work) = match direct {
        MatrixRequirementsLocator::Program { program_id } => (program_id, None, None),
        MatrixRequirementsLocator::Scope {
            program_id,
            scope_id,
        } => (program_id, Some(scope_id), None),
        MatrixRequirementsLocator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        } => (
            program_id,
            Some(scope_id),
            Some((candidate_set_id, work_candidate_id, expected_work_revision)),
        ),
        MatrixRequirementsLocator::OpenedSlice { .. } => return Err(Error::InternalInvariant),
    };
    let program_sql = format!(
        "SELECT id FROM programs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{}",
        if for_write { " FOR UPDATE" } else { "" }
    );
    let exists: Option<Uuid> = sqlx::query_scalar(&program_sql)
        .bind(tenant)
        .bind(workspace)
        .bind(program)
        .fetch_optional(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
    if exists.is_none() {
        return Err(Error::NotFound);
    }
    let mut lineage = vec![RequirementsAnchor::Program {
        program_id: program,
    }];
    if let Some(scope) = scope {
        let scope_sql = format!(
            "SELECT c.program_id FROM native_scopes n JOIN scope_candidate_sets c ON (c.tenant_id,c.workspace_id,c.id)=(n.tenant_id,n.workspace_id,n.source_candidate_set_id) WHERE n.tenant_id=$1 AND n.workspace_id=$2 AND n.id=$3{}",
            if for_write { " FOR UPDATE OF n" } else { "" }
        );
        let actual: Option<Uuid> = sqlx::query_scalar(&scope_sql)
            .bind(tenant)
            .bind(workspace)
            .bind(scope)
            .fetch_optional(&mut **uow.transaction()?)
            .await
            .map_err(storage_error)?;
        if actual != Some(program) {
            return Err(Error::InputConflict);
        }
        lineage.push(RequirementsAnchor::Scope {
            program_id: program,
            scope_id: scope,
        });
        if let Some((set, work, revision)) = work {
            let set_sql = format!(
                "SELECT scope_id FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{}",
                if for_write { " FOR UPDATE" } else { "" }
            );
            let actual: Option<Uuid> = sqlx::query_scalar(&set_sql)
                .bind(tenant)
                .bind(workspace)
                .bind(set)
                .fetch_optional(&mut **uow.transaction()?)
                .await
                .map_err(storage_error)?;
            if actual != Some(scope) {
                return Err(Error::InputConflict);
            }
            let row: Option<(Option<Value>,bool)> = sqlx::query_as("SELECT payload,payload_erased FROM slice_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND ($4::bigint IS NULL OR set_revision<=$4) ORDER BY set_revision DESC LIMIT 1")
                .bind(tenant).bind(workspace).bind(set).bind(opened).fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?;
            let (payload, erased) = row.ok_or(Error::NotFound)?;
            if erased {
                return Err(Error::KnowledgePayloadErased);
            }
            let draft: ResolvedSliceCandidateDraft =
                serde_json::from_value(payload.ok_or(Error::InternalInvariant)?)
                    .map_err(|_| Error::InputConflict)?;
            let nodes: Vec<_> = draft
                .nodes
                .iter()
                .filter(|node| node.id() == work)
                .collect();
            if nodes.len() != 1 || !matches!(nodes[0], SliceCandidateNode::Work { .. }) {
                return Err(Error::InputConflict);
            }
            if nodes[0].revision() != revision {
                return Err(Error::StaleRevision);
            }
            lineage.push(RequirementsAnchor::Slice {
                program_id: program,
                scope_id: scope,
                candidate_set_id: set,
                work_candidate_id: work,
            });
        }
    }
    Ok(lineage)
}
