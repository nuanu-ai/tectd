use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;

const SOURCE_AUTHORED_ID: &str = "source-authored-v1";
const SOURCE_AUTHORED_VERSION: &str = "1";
const SOURCE_AUTHORED_POLICY: &str = include_str!("source_authored_v1.policy.txt");

pub struct PgScopeAuthoredManifestSupplier {
    store: crate::PgStore,
    authority: Arc<dyn ScopeAuthorityObserver>,
}

impl PgScopeAuthoredManifestSupplier {
    pub fn new(store: crate::PgStore, authority: Arc<dyn ScopeAuthorityObserver>) -> Self {
        Self { store, authority }
    }
}

fn source_authored_identity() -> ScopeConstructorIdentity {
    ScopeConstructorIdentity {
        id: SOURCE_AUTHORED_ID.into(),
        version: SOURCE_AUTHORED_VERSION.into(),
        digest: format!("{:x}", Sha256::digest(SOURCE_AUTHORED_POLICY.as_bytes())),
    }
}

fn authored_seed(
    tenant: Uuid,
    workspace: Uuid,
    source: &FrozenScopeSource,
    constructor: &ScopeConstructorIdentity,
    key: &str,
) -> Result<[u8; 32]> {
    let payload = serde_json::to_vec(&(
        tenant,
        workspace,
        source.candidate_set_id,
        source.snapshot_id,
        &source.digest,
        &constructor.digest,
        key,
    ))
    .map_err(storage_error)?;
    let mut hash = Sha256::new();
    hash.update(b"tect.scope-authored-entity-seed/source-authored-v1\0");
    hash.update(payload);
    Ok(hash.finalize().into())
}

fn atomic_coverage(
    declared: &[Uuid],
    obligations: &[SourceObligation],
) -> Result<Vec<ObligationCoverage>> {
    let expected = obligations
        .iter()
        .map(|value| Uuid::parse_str(&value.source_input_id).map_err(|_| Error::InvalidSource))
        .collect::<Result<BTreeSet<_>>>()?;
    let actual = declared.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected || declared.len() != actual.len() {
        return Err(Error::InvalidSource);
    }
    Ok(obligations
        .iter()
        .map(|value| ObligationCoverage {
            obligation_id: value.id.clone(),
            condition_ids: Vec::new(),
            exception_ids: Vec::new(),
        })
        .collect())
}

#[async_trait]
impl ScopeManifestSupplier for PgScopeAuthoredManifestSupplier {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some((SOURCE_AUTHORED_ID, SOURCE_AUTHORED_VERSION))
    }

    async fn supply(&self, _: &ScopeAuthorityObservation) -> Result<ScopeConstructorManifest> {
        Err(Error::InputPending)
    }

    async fn supply_authored(
        &self,
        request: &ScopeAuthoredManifestRequest,
    ) -> Result<ScopeConstructorManifest> {
        request.authored_scope_set.validate()?;
        let observation = &request.observation;
        let source = &observation.source;
        source.validate(&Sha256ScopeDigest)?;
        if request.tenant_id.is_nil()
            || observation.workspace_id.is_nil()
            || observation.actor_id.is_nil()
            || observation.session_id.is_nil()
            || observation.candidate_set_id != source.candidate_set_id
            || request.authored_scope_set.expected_candidate_set_revision
                != source.candidate_set_revision
        {
            return Err(Error::InputConflict);
        }
        let authority_request = ScopeAuthorityRequest {
            tenant_id: request.tenant_id,
            workspace_id: observation.workspace_id,
            actor_id: observation.actor_id,
            session_id: observation.session_id,
            candidate_set_id: observation.candidate_set_id,
        };
        let fresh = self.authority.observe(&authority_request).await?;
        if fresh != ScopeAuthorityOutcome::Authorized(observation.clone()) {
            return Err(Error::StaleRevision);
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
        let authorized: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agent_sessions s JOIN memberships m \
             ON (m.tenant_id,m.workspace_id)=(s.tenant_id,s.workspace_id) \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 \
               AND m.principal_id=$4 AND public.tect_dk_session_principal(s.id)=$4)",
        )
        .bind(request.tenant_id)
        .bind(observation.workspace_id)
        .bind(observation.session_id)
        .bind(observation.actor_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        if !authorized {
            return Err(Error::Forbidden);
        }
        let stored = crate::scope_candidates::load(
            &mut tx,
            request.tenant_id,
            observation.workspace_id,
            source.candidate_set_id,
        )
        .await?
        .ok_or(Error::NotFound)?;
        let set = &stored.context.candidate_set;
        let snapshot = &stored.context.snapshot;
        let current_program = crate::programs::program(
            &mut tx,
            request.tenant_id,
            observation.workspace_id,
            source.program_id,
            false,
        )
        .await?
        .ok_or(Error::NotFound)?;
        if set.revision != source.candidate_set_revision
            || set.current_snapshot_id != source.snapshot_id
            || set.input_cursor != source.input_cursor
            || set.latest_input != source.planning_latest_input
            || set.program_id != source.program_id
            || current_program.revision != source.program_revision
            || current_program.latest_input != source.program_latest_input
            || snapshot.program_revision != source.program_revision
            || snapshot.program_latest_input != source.program_latest_input
            || snapshot.planning_latest_input != source.planning_latest_input
            || snapshot.selected_sources_digest != source.selected_sources_digest
            || snapshot.method.revision != source.method_revision
            || snapshot.method.digest != source.method_digest
            || snapshot.registry_revision != source.registry_revision
            || snapshot.registry_digest != source.registry_digest
        {
            return Err(Error::StaleRevision);
        }
        let fragments = load_persisted_fragments(
            &mut tx,
            request.tenant_id,
            observation.workspace_id,
            source.candidate_set_id,
            source.snapshot_id,
        )
        .await?;
        let (inputs, obligations) = source_inputs_and_obligations(fragments, source.snapshot_id)?;
        if inputs != source.inputs || obligations != observation.obligations {
            return Err(Error::StaleRevision);
        }
        let allowed_source_ids = source
            .inputs
            .iter()
            .map(|input| Uuid::parse_str(&input.id).map_err(|_| Error::InvalidSource))
            .collect::<Result<BTreeSet<_>>>()?;

        let constructor = source_authored_identity();
        let mut alternatives = Vec::with_capacity(request.authored_scope_set.alternatives.len());
        for alternative in &request.authored_scope_set.alternatives {
            if alternative.draft.boundary != set.boundary {
                return Err(Error::InvalidArguments);
            }
            let coverage = atomic_coverage(
                &alternative.covered_source_ref_ids,
                &observation.obligations,
            )?;
            let seed = authored_seed(
                request.tenant_id,
                observation.workspace_id,
                source,
                &constructor,
                &alternative.key,
            )?;
            let material = crate::scope_candidates::resolve::resolve_authored(
                &mut tx,
                &crate::scope_candidates::resolve::ResolveContext {
                    tenant_id: request.tenant_id,
                    workspace_id: observation.workspace_id,
                    candidate_set_id: source.candidate_set_id,
                    snapshot_id: source.snapshot_id,
                    latest_input: set.latest_input,
                },
                &alternative.draft,
                stored.draft.as_ref(),
                &seed,
                &allowed_source_ids,
            )
            .await?;
            alternatives.push(SourceAuthoredScopeAlternative {
                key: alternative.key.clone(),
                kind: alternative.kind,
                material,
                coverage,
            });
        }
        let manifest = build_source_authored_scope_manifest(
            &Sha256ScopeDigest,
            BuildSourceAuthoredScopeManifest {
                constructor,
                source: source.clone(),
                obligations: observation.obligations.clone(),
                alternatives,
                baseline_key: request.authored_scope_set.baseline_key.clone(),
            },
        )?;
        tx.commit().await.map_err(storage_error)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod authored_supplier_tests {
    use super::*;

    #[test]
    fn constructor_identity_is_bound_to_versioned_policy_bytes() {
        let identity = source_authored_identity();
        assert_eq!(identity.id, SOURCE_AUTHORED_ID);
        assert_eq!(identity.version, SOURCE_AUTHORED_VERSION);
        assert_eq!(identity.digest, format!("{:x}", Sha256::digest(SOURCE_AUTHORED_POLICY.as_bytes())));
        identity.validate().unwrap();
    }

    #[test]
    fn seed_changes_with_alternative_or_frozen_source() {
        let candidate = Uuid::from_u128(3);
        let snapshot = Uuid::from_u128(4);
        let mut source = FrozenScopeSource {
            candidate_set_id: candidate,
            candidate_set_revision: 2,
            snapshot_id: snapshot,
            input_cursor: 1,
            program_id: Uuid::from_u128(5),
            program_revision: 2,
            program_latest_input: 1,
            planning_latest_input: 1,
            selected_sources_digest: "a".repeat(64),
            method_revision: "1".into(),
            method_digest: "b".repeat(64),
            registry_revision: "1".into(),
            registry_digest: "c".repeat(64),
            inputs: vec![],
            digest: "d".repeat(64),
        };
        let identity = source_authored_identity();
        let tenant = Uuid::from_u128(1);
        let workspace = Uuid::from_u128(2);
        let first = authored_seed(tenant, workspace, &source, &identity, "first").unwrap();
        assert_eq!(first, authored_seed(tenant, workspace, &source, &identity, "first").unwrap());
        assert_ne!(first, authored_seed(tenant, workspace, &source, &identity, "second").unwrap());
        source.digest = "e".repeat(64);
        assert_ne!(first, authored_seed(tenant, workspace, &source, &identity, "first").unwrap());
    }

    #[test]
    fn coverage_requires_exact_atomic_ref_set() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        let obligations = [first, second]
            .into_iter()
            .map(|id| SourceObligation {
                id: id.to_string(),
                source_input_id: id.to_string(),
                statement_digest: "a".repeat(64),
                conditions: vec![],
                exceptions: vec![],
            })
            .collect::<Vec<_>>();
        let coverage = atomic_coverage(&[first, second], &obligations).unwrap();
        assert_eq!(coverage.len(), 2);
        assert!(coverage.iter().all(|row| row.condition_ids.is_empty() && row.exception_ids.is_empty()));
        assert_eq!(atomic_coverage(&[first], &obligations), Err(Error::InvalidSource));
        assert_eq!(atomic_coverage(&[first, first], &obligations), Err(Error::InvalidSource));
        assert_eq!(atomic_coverage(&[first, Uuid::from_u128(9)], &obligations), Err(Error::InvalidSource));
    }
}
