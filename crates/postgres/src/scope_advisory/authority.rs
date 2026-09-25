pub struct PgScopeAuthorityObserver {
    store: crate::PgStore,
    guidance: std::sync::Arc<dyn tect_application::CandidateGuidance>,
}

impl PgScopeAuthorityObserver {
    pub fn new(
        store: crate::PgStore,
        guidance: std::sync::Arc<dyn tect_application::CandidateGuidance>,
    ) -> Self {
        Self { store, guidance }
    }
}

#[derive(sqlx::FromRow)]
struct SourceAuthorityRow {
    candidate_set_revision: i64,
    snapshot_id: Uuid,
    input_cursor: i64,
    candidate_latest_input: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_payload_erased: bool,
    snapshot_program_revision: i64,
    program_latest_input: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

fn invalid_source(request: &ScopeAuthorityRequest) -> ScopeAuthorityOutcome {
    ScopeAuthorityOutcome::AuthorizedInvalid(ScopeAuthorizedInvalidObservation {
        workspace_id: request.workspace_id,
        actor_id: request.actor_id,
        session_id: request.session_id,
        candidate_set_id: request.candidate_set_id,
    })
}

struct GuidanceFingerprint<'a> {
    selected_sources_digest: &'a str,
    method_revision: &'a str,
    method_digest: &'a str,
    registry_revision: &'a str,
    registry_digest: &'a str,
}

impl<'a> From<&'a CandidateSnapshotMaterial> for GuidanceFingerprint<'a> {
    fn from(value: &'a CandidateSnapshotMaterial) -> Self {
        Self {
            selected_sources_digest: &value.selected_sources_digest,
            method_revision: &value.method.revision,
            method_digest: &value.method.digest,
            registry_revision: &value.registry_revision,
            registry_digest: &value.registry_digest,
        }
    }
}

fn require_current_source(
    row: &SourceAuthorityRow,
    current: &GuidanceFingerprint<'_>,
) -> Result<()> {
    if row.program_revision != row.snapshot_program_revision
        || row.program_current_latest != row.program_latest_input
        || row.candidate_latest_input != row.planning_latest_input
        || row.input_cursor != row.candidate_latest_input
        || row.selected_sources_digest != current.selected_sources_digest
        || row.method_revision != current.method_revision
        || row.method_digest != current.method_digest
        || row.registry_revision != current.registry_revision
        || row.registry_digest != current.registry_digest
    {
        return Err(Error::StaleRevision);
    }
    Ok(())
}

#[async_trait]
impl ScopeAuthorityObserver for PgScopeAuthorityObserver {
    async fn observe(&self, request: &ScopeAuthorityRequest) -> Result<ScopeAuthorityOutcome> {
        if request.tenant_id.is_nil()
            || request.workspace_id.is_nil()
            || request.actor_id.is_nil()
            || request.session_id.is_nil()
            || request.candidate_set_id.is_nil()
        {
            return Err(Error::InvalidArguments);
        }
        let mut tx = self.store.pool().begin().await.map_err(storage_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(request.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;

        // Recheck the session and actor after crossing out of the application's
        // authenticated transaction. The scoped tenant setting activates RLS.
        let host_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT s.host_id FROM agent_sessions s JOIN memberships m \
             ON (m.tenant_id,m.workspace_id)=(s.tenant_id,s.workspace_id) \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 \
               AND m.principal_id=$4 AND public.tect_dk_session_principal(s.id)=$4",
        )
        .bind(request.tenant_id)
        .bind(request.workspace_id)
        .bind(request.session_id)
        .bind(request.actor_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        let Some(host_id) = host_id else {
            return Err(Error::Forbidden);
        };

        let row: Option<SourceAuthorityRow> = sqlx::query_as(
            "SELECT c.revision AS candidate_set_revision,s.id AS snapshot_id,c.input_cursor,\
                    c.latest_input AS candidate_latest_input,c.program_id,\
                    p.revision AS program_revision,p.latest_input AS program_current_latest,\
                    p.payload_erased AS program_payload_erased,\
                    s.program_revision AS snapshot_program_revision,s.program_latest_input,\
                    s.planning_latest_input,s.selected_sources_digest,s.method_revision,\
                    s.method_digest,s.registry_revision,s.registry_digest \
             FROM scope_candidate_sets c JOIN programs p \
               ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
             JOIN scope_candidate_snapshots s \
               ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)=\
                  (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
             WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3",
        )
        .bind(request.tenant_id)
        .bind(request.workspace_id)
        .bind(request.candidate_set_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        crate::planning_knowledge::require_owned_payload_identity(
            &mut tx,
            request.tenant_id,
            request.workspace_id,
            &["programs"],
            Some(row.program_id),
        )
        .await?;
        if row.program_payload_erased {
            return Ok(invalid_source(request));
        }
        let program = crate::programs::program(
            &mut tx,
            request.tenant_id,
            request.workspace_id,
            row.program_id,
            false,
        )
        .await?
        .ok_or(Error::NotFound)?;
        let selected_worktrees = crate::sources::selected_worktrees(
            &mut tx,
            request.tenant_id,
            request.workspace_id,
            host_id,
            request.session_id,
        )
        .await?;
        let current = self.guidance.snapshot(program, selected_worktrees)?;
        require_current_source(&row, &GuidanceFingerprint::from(&current))?;

        let fragments = load_persisted_fragments(
            &mut tx,
            request.tenant_id,
            request.workspace_id,
            request.candidate_set_id,
            row.snapshot_id,
        )
        .await?;
        let (inputs, obligations) = match source_inputs_and_obligations(fragments, row.snapshot_id)
        {
            Ok(value) => value,
            Err(Error::InvalidSource) => return Ok(invalid_source(request)),
            Err(error) => return Err(error),
        };
        let mut source = FrozenScopeSource {
            candidate_set_id: request.candidate_set_id,
            candidate_set_revision: row.candidate_set_revision,
            snapshot_id: row.snapshot_id,
            input_cursor: row.input_cursor,
            program_id: row.program_id,
            program_revision: row.program_revision,
            program_latest_input: row.program_latest_input,
            planning_latest_input: row.planning_latest_input,
            selected_sources_digest: row.selected_sources_digest,
            method_revision: row.method_revision,
            method_digest: row.method_digest,
            registry_revision: row.registry_revision,
            registry_digest: row.registry_digest,
            inputs,
            digest: String::new(),
        };
        source.digest = source.canonical_digest(&Sha256ScopeDigest)?;
        if source.validate(&Sha256ScopeDigest).is_err() {
            return Ok(invalid_source(request));
        }
        tx.commit().await.map_err(storage_error)?;
        Ok(ScopeAuthorityOutcome::Authorized(Box::new(
            ScopeAuthorityObservation {
                workspace_id: request.workspace_id,
                actor_id: request.actor_id,
                session_id: request.session_id,
                candidate_set_id: request.candidate_set_id,
                source,
                obligations,
            },
        )))
    }
}

#[cfg(test)]
mod authority_tests {
    use super::*;

    #[test]
    fn persisted_refs_become_sorted_atomic_obligations() {
        let snapshot = Uuid::from_u128(7);
        let digest = "a".repeat(64);
        let (inputs, obligations) = source_inputs_and_obligations(
            vec![
                PersistedSourceFragment {
                    id: Uuid::from_u128(3),
                    kind: "planning_input".into(),
                    body_digest: digest.clone(),
                    body: "second".into(),
                },
                PersistedSourceFragment {
                    id: Uuid::from_u128(1),
                    kind: "planning_input".into(),
                    body_digest: digest.clone(),
                    body: "first".into(),
                },
                PersistedSourceFragment {
                    id: Uuid::from_u128(2),
                    kind: "planning_input".into(),
                    body_digest: digest.clone(),
                    body: "   ".into(),
                },
            ],
            snapshot,
        )
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].id, Uuid::from_u128(1).to_string());
        assert_eq!(inputs[1].id, Uuid::from_u128(3).to_string());
        assert_eq!(inputs[0].version, snapshot.to_string());
        assert_eq!(inputs[0].digest, digest);
        assert_eq!(inputs[0].applicability, SourceApplicability::Applicable);
        assert_eq!(obligations.len(), 2);
        assert_eq!(obligations[0].source_input_id, inputs[0].id);
        assert_eq!(obligations[0].statement_digest, inputs[0].digest);
        assert!(
            obligations
                .iter()
                .all(|value| value.conditions.is_empty() && value.exceptions.is_empty())
        );
        assert_eq!(
            source_inputs_and_obligations(Vec::new(), snapshot),
            Err(Error::InvalidSource)
        );
    }

    #[test]
    fn source_freshness_rejects_each_moved_cursor() {
        let mut row = SourceAuthorityRow {
            candidate_set_revision: 3,
            snapshot_id: Uuid::from_u128(1),
            input_cursor: 2,
            candidate_latest_input: 2,
            program_id: Uuid::from_u128(2),
            program_revision: 4,
            program_current_latest: 2,
            program_payload_erased: false,
            snapshot_program_revision: 4,
            program_latest_input: 2,
            planning_latest_input: 2,
            selected_sources_digest: String::new(),
            method_revision: String::new(),
            method_digest: String::new(),
            registry_revision: String::new(),
            registry_digest: String::new(),
        };
        let current = GuidanceFingerprint {
            selected_sources_digest: "",
            method_revision: "",
            method_digest: "",
            registry_revision: "",
            registry_digest: "",
        };
        assert_eq!(require_current_source(&row, &current), Ok(()));
        row.program_revision += 1;
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        row.program_revision -= 1;
        row.program_current_latest += 1;
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        row.program_current_latest -= 1;
        row.candidate_latest_input += 1;
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        row.candidate_latest_input -= 1;
        row.input_cursor -= 1;
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
    }

    #[test]
    fn source_freshness_rejects_changed_selected_sources_method_and_registry() {
        let row = SourceAuthorityRow {
            candidate_set_revision: 3,
            snapshot_id: Uuid::from_u128(1),
            input_cursor: 2,
            candidate_latest_input: 2,
            program_id: Uuid::from_u128(2),
            program_revision: 4,
            program_current_latest: 2,
            program_payload_erased: false,
            snapshot_program_revision: 4,
            program_latest_input: 2,
            planning_latest_input: 2,
            selected_sources_digest: "sources".into(),
            method_revision: "4".into(),
            method_digest: "method".into(),
            registry_revision: "3".into(),
            registry_digest: "registry".into(),
        };
        let mut current = GuidanceFingerprint {
            selected_sources_digest: "sources",
            method_revision: "4",
            method_digest: "method",
            registry_revision: "3",
            registry_digest: "registry",
        };
        assert_eq!(require_current_source(&row, &current), Ok(()));
        current.selected_sources_digest = "changed";
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        current.selected_sources_digest = "sources";
        current.method_revision = "5";
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        current.method_revision = "4";
        current.method_digest = "changed";
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        current.method_digest = "method";
        current.registry_revision = "4";
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
        current.registry_revision = "3";
        current.registry_digest = "changed";
        assert_eq!(
            require_current_source(&row, &current),
            Err(Error::StaleRevision)
        );
    }
}
