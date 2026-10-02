use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct PublicationProofKey {
    pub unit_id: Uuid,
    pub revision: i64,
    pub event_id: Uuid,
    pub include_revision: bool,
}

#[derive(Clone)]
pub(crate) struct ExpectedPublicationMaterial {
    pub unit_iri: String,
    pub revision_iri: String,
    pub document_json: Option<serde_json::Value>,
}

/// Verified publication material for one lexical, workspace-locked operation.
/// The caller must retain the same transaction and lock for this scope's entire
/// lifetime. Bindings, authorization, maintenance and freshness are not cached.
pub(crate) struct PublicationProofScope {
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    verified: BTreeMap<PublicationProofKey, Arc<VerifiedPublicationEvent>>,
    expected: BTreeMap<PublicationProofKey, ExpectedPublicationMaterial>,
    eager_preload: bool,
}

impl PublicationProofScope {
    pub(crate) fn new(tenant: Uuid, workspace: Uuid, principal: Uuid, session: Uuid) -> Self {
        Self {
            tenant,
            workspace,
            principal,
            session,
            verified: BTreeMap::new(),
            expected: BTreeMap::new(),
            eager_preload: true,
        }
    }

    /// Only ordinary context reads use lazy first-resource proof ordering.
    /// Creating this scope itself acquires the existing workspace fence; it
    /// never creates knowledge state or moves inactive/absent identity gates.
    /// Keep the same transaction alive for the entire scope.
    pub(crate) async fn ordinary_context(
        tx: &mut Transaction<'_, Postgres>,
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
    ) -> Result<Option<Self>> {
        let ready: Option<bool> = sqlx::query_scalar(
            "SELECT capability_ready FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
        )
        .bind(tenant)
        .bind(workspace)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
        if ready != Some(true) {
            return Ok(None);
        }
        let mut scope = Self::new(tenant, workspace, principal, session);
        scope.eager_preload = false;
        Ok(Some(scope))
    }

    pub(crate) fn eager_preload(&self) -> bool {
        self.eager_preload
    }

    pub(crate) fn require_identity(
        &self,
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
    ) -> Result<()> {
        if (tenant, workspace, principal, session)
            != (self.tenant, self.workspace, self.principal, self.session)
        {
            return Err(Error::InternalInvariant);
        }
        Ok(())
    }

    /// Only a fully verified exact key can supply immutable expectations. The
    /// caller still compares fresh relational rows and current state itself.
    pub(crate) fn expected_material(
        &self,
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        key: PublicationProofKey,
    ) -> Result<&ExpectedPublicationMaterial> {
        self.require_identity(tenant, workspace, principal, session)?;
        if !self.verified.contains_key(&key) {
            return Err(Error::InternalInvariant);
        }
        self.expected.get(&key).ok_or(Error::InternalInvariant)
    }

    pub(crate) async fn preload(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        keys: &[PublicationProofKey],
    ) -> Result<()> {
        let keys = keys
            .iter()
            .copied()
            .filter(|key| !self.verified.contains_key(key))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(());
        }
        let event_ids = keys.iter().map(|key| key.event_id).collect::<BTreeSet<_>>();
        type EventBatchRow = (
            Uuid,
            Option<serde_json::Value>,
            Option<String>,
            Uuid,
            i64,
            String,
            Uuid,
            Uuid,
            bool,
        );
        let rows: Vec<EventBatchRow> = sqlx::query_as(
            "SELECT id,event_payload,rdf_digest,unit_id,unit_revision,operation,lifecycle_change_id,operation_id,payload_erased \
             FROM knowledge_publication_events WHERE tenant_id=$1 AND workspace_id=$2 AND id=ANY($3) AND contract_version='dk-2'",
        )
        .bind(self.tenant)
        .bind(self.workspace)
        .bind(event_ids.iter().copied().collect::<Vec<_>>())
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        let mut events = BTreeMap::new();
        for (id, payload, digest, unit, revision, operation, change, operation_id, erased) in rows {
            if !event_ids.contains(&id)
                || events
                    .insert(
                        id,
                        (
                            payload,
                            digest,
                            unit,
                            revision,
                            operation,
                            change,
                            operation_id,
                            erased,
                        ),
                    )
                    .is_some()
            {
                return Err(Error::InternalInvariant);
            }
        }
        if events.len() != event_ids.len() {
            return Err(Error::InternalInvariant);
        }
        let mut pending = Vec::with_capacity(keys.len());
        for key in &keys {
            let verified = verify_event_payload(
                self.tenant,
                self.workspace,
                key.unit_id,
                key.revision,
                key.event_id,
                events
                    .get(&key.event_id)
                    .ok_or(Error::InternalInvariant)?
                    .clone(),
            )?;
            let document = rdf::build(&verified.input)?;
            let request = rdf::NativeReadRequest {
                unit_id: key.unit_id,
                revision: key.revision,
                event_id: key.event_id,
                include_revision: key.include_revision,
            };
            pending.push((request, document, verified));
        }
        // The adapter rejects missing/unknown ordinals, mismatched echoed keys,
        // invalid empty sentinels and any typed RDF mismatch using validate_rows.
        let requests = pending
            .iter()
            .map(|(key, document, _)| (key, document))
            .collect::<Vec<_>>();
        let groups = rdf::native_rows_batch(tx, self.tenant, self.workspace, &requests).await?;
        if groups.len() != pending.len() {
            return Err(Error::InternalInvariant);
        }
        let change_ids = pending
            .iter()
            .map(|(_, _, value)| value.input.change_id)
            .collect::<BTreeSet<_>>();
        let rows: Vec<(Uuid, Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT change_id,publisher_receipt,erased_publisher_receipt FROM knowledge_change_runs \
             WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=ANY($3)",
        )
        .bind(self.tenant)
        .bind(self.workspace)
        .bind(change_ids.iter().copied().collect::<Vec<_>>())
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        let mut receipts = BTreeMap::new();
        for (change, full, erased) in rows {
            if !change_ids.contains(&change) || receipts.insert(change, (full, erased)).is_some() {
                return Err(Error::InternalInvariant);
            }
        }
        if receipts.len() != change_ids.len() {
            return Err(Error::InternalInvariant);
        }
        for (_, _, verified) in &pending {
            verify_receipt_row(
                self.tenant,
                self.workspace,
                verified,
                receipts
                    .get(&verified.input.change_id)
                    .ok_or(Error::InternalInvariant)?
                    .clone(),
            )?;
        }
        let mut expected = Vec::with_capacity(pending.len());
        for (_, document, verified) in &pending {
            expected.push(ExpectedPublicationMaterial {
                unit_iri: document.refs.unit.clone(),
                revision_iri: document.refs.revision.clone(),
                document_json: verified
                    .input
                    .planned
                    .document
                    .as_ref()
                    .map(json)
                    .transpose()?,
            });
        }
        // Publish both caches only after every requested proof and derived
        // expectation has passed. No mutable relational state is retained.
        for ((key, (_, _, verified)), material) in keys.into_iter().zip(pending).zip(expected) {
            self.verified.insert(key, Arc::new(verified));
            self.expected.insert(key, material);
        }
        Ok(())
    }

    pub(crate) async fn verify(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        key: PublicationProofKey,
    ) -> Result<Arc<VerifiedPublicationEvent>> {
        self.preload(tx, &[key]).await?;
        // Share only immutable verified publication material within this
        // workspace-locked operation; mutable authority remains freshly read.
        self.verified
            .get(&key)
            .map(Arc::clone)
            .ok_or(Error::InternalInvariant)
    }
}

#[cfg(test)]
mod tests;
