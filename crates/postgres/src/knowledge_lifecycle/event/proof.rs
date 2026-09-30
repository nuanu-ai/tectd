use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct PublicationProofKey {
    pub unit_id: Uuid,
    pub revision: i64,
    pub event_id: Uuid,
    pub include_revision: bool,
}

/// Verified publication material for one lexical, workspace-locked operation.
/// The caller must retain the same transaction and lock for this scope's entire
/// lifetime. Bindings, authorization, maintenance and freshness are not cached.
pub(crate) struct PublicationProofScope {
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    verified: BTreeMap<PublicationProofKey, VerifiedPublicationEvent>,
}

impl PublicationProofScope {
    pub(crate) fn new(tenant: Uuid, workspace: Uuid, principal: Uuid, session: Uuid) -> Self {
        Self {
            tenant,
            workspace,
            principal,
            session,
            verified: BTreeMap::new(),
        }
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
        // Publish cache entries only after every requested proof has passed.
        for (key, (_, _, verified)) in keys.into_iter().zip(pending) {
            self.verified.insert(key, verified);
        }
        Ok(())
    }

    pub(crate) async fn verify(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        key: PublicationProofKey,
    ) -> Result<VerifiedPublicationEvent> {
        self.preload(tx, &[key]).await?;
        self.verified
            .get(&key)
            .cloned()
            .ok_or(Error::InternalInvariant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> rdf::RdfPublicationInput {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../rdf/fixtures/general-constraint.json")).unwrap();
        rdf::RdfPublicationInput {
            tenant: Uuid::from_u128(1),
            workspace: Uuid::from_u128(2),
            change_id: Uuid::from_u128(3),
            event_id: Uuid::from_u128(4),
            content_revision: 1,
            planned: KnowledgePlannedOperation {
                operation_id: Uuid::from_u128(5),
                unit_id: Uuid::from_u128(6),
                client_label: "test".into(),
                operation: KnowledgeLifecycleOperation::Create,
                expected_revision: None,
                expected_lifecycle: None,
                document: Some(decode(fixture["document"].clone()).unwrap()),
                revalidation: None,
                successor: None,
                replacement_bindings: vec![],
                reason: "test".into(),
                authority_basis: "test".into(),
                dependency_operation_ids: vec![],
                binding_pins: vec![],
            },
            principal_id: Uuid::from_u128(7),
            session_id: Uuid::from_u128(8),
            resolved_sources: vec![],
            successor_unit: None,
            include_empty_planning_briefs: false,
        }
    }

    fn event_row(input: &rdf::RdfPublicationInput) -> PublicationEventRow {
        (
            Some(json(input).unwrap()),
            Some("rdf-digest".into()),
            input.planned.unit_id,
            input.content_revision,
            "create".into(),
            input.change_id,
            input.planned.operation_id,
            false,
        )
    }

    fn operation(input: &rdf::RdfPublicationInput) -> KnowledgeAppliedOperationReceipt {
        let unit = format!(
            "urn:tect:dk:unit:{}:{}:{}",
            input.tenant, input.workspace, input.planned.unit_id
        );
        KnowledgeAppliedOperationReceipt {
            operation_id: input.planned.operation_id,
            unit_id: input.planned.unit_id,
            operation: input.planned.operation,
            revision: Some(input.content_revision),
            event_id: input.event_id,
            revision_iri: Some(format!("{unit}:revision:{}", input.content_revision)),
            unit_iri: unit,
            event_iri: format!(
                "urn:tect:dk:event:{}:{}:{}",
                input.tenant, input.workspace, input.event_id
            ),
            rdf_digest: "rdf-digest".into(),
            rdf_digest_method: "rdfc-1.0-sha256".into(),
            rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
        }
    }

    fn full_receipt(
        input: &rdf::RdfPublicationInput,
        operation: KnowledgeAppliedOperationReceipt,
    ) -> PublisherReceiptRow {
        let mut receipt = KnowledgePublisherReceipt {
            id: Uuid::from_u128(9),
            request_id: Uuid::from_u128(10),
            change_id: input.change_id,
            run_id: Uuid::from_u128(11),
            sealed_command_digest: "sealed".into(),
            workspace_generation: 1,
            applied_operations: vec![operation],
            effects: vec![],
            digest: String::new(),
        };
        receipt.digest = digest(&receipt).unwrap();
        (Some(json(&receipt).unwrap()), None)
    }

    #[test]
    fn event_identity_and_erasure_checks_are_shared() {
        let input = input();
        let check = |row| {
            verify_event_payload(
                input.tenant,
                input.workspace,
                input.planned.unit_id,
                input.content_revision,
                input.event_id,
                row,
            )
            .map(|_| ())
        };
        assert_eq!(check(event_row(&input)), Ok(()));
        let mut erased = event_row(&input);
        erased.7 = true;
        assert_eq!(check(erased), Err(Error::KnowledgePayloadErased));
        let mut missing = event_row(&input);
        missing.0 = None;
        assert_eq!(check(missing), Err(Error::InternalInvariant));
        let mut operation = event_row(&input);
        operation.4 = "revalidate".into();
        assert_eq!(check(operation), Err(Error::InternalInvariant));
        for field in ["tenant", "workspace", "change_id", "event_id"] {
            let mut wrong = event_row(&input);
            wrong.0.as_mut().unwrap()[field] = json(&Uuid::from_u128(99)).unwrap();
            assert_eq!(check(wrong), Err(Error::InternalInvariant));
        }
        let mut wrong_revision = event_row(&input);
        wrong_revision.3 += 1;
        assert_eq!(check(wrong_revision), Err(Error::InternalInvariant));
        let mut wrong_unit = event_row(&input);
        wrong_unit.2 = Uuid::from_u128(99);
        assert_eq!(check(wrong_unit), Err(Error::InternalInvariant));
        let mut wrong_operation = event_row(&input);
        wrong_operation.6 = Uuid::from_u128(99);
        assert_eq!(check(wrong_operation), Err(Error::InternalInvariant));
    }

    #[test]
    fn full_and_retained_receipt_checks_are_shared() {
        let input = input();
        let verified = VerifiedPublicationEvent {
            input: input.clone(),
            rdf_digest: "rdf-digest".into(),
        };
        let check = |row| verify_receipt_row(input.tenant, input.workspace, &verified, row);
        assert_eq!(check(full_receipt(&input, operation(&input))), Ok(()));
        let mut tampered = full_receipt(&input, operation(&input));
        tampered.0.as_mut().unwrap()["digest"] = serde_json::json!("tampered");
        assert_eq!(check(tampered), Err(Error::InternalInvariant));
        let mut wrong = operation(&input);
        wrong.event_id = Uuid::from_u128(99);
        assert_eq!(
            check(full_receipt(&input, wrong)),
            Err(Error::InternalInvariant)
        );
        let full = full_receipt(&input, operation(&input));
        assert_eq!(
            check((full.0.clone(), full.0)),
            Err(Error::InternalInvariant)
        );
        let retained = KnowledgeErasedPublisherReceipt {
            id: Uuid::from_u128(9),
            request_id: Uuid::from_u128(10),
            change_id: input.change_id,
            run_id: Uuid::from_u128(11),
            completion: KnowledgeCompletionRequirement {
                canonical_result: true,
                exact_delivery: true,
                impact_recorded: true,
                search: KnowledgeSearchRequirement::NotRequired,
                erasure: KnowledgeErasureRequirement::NotRequired,
            },
            operations: vec![KnowledgeRetainedOperationReceipt::Intact(operation(&input))],
            effects: vec![],
        };
        assert_eq!(check((None, Some(json(&retained).unwrap()))), Ok(()));
        let mut wrong = retained;
        let KnowledgeRetainedOperationReceipt::Intact(operation) = &mut wrong.operations[0] else {
            unreachable!()
        };
        operation.rdf_digest_scope = KnowledgeRdfDigestScope::LifecycleEventPayload;
        assert_eq!(
            check((None, Some(json(&wrong).unwrap()))),
            Err(Error::InternalInvariant)
        );
    }

    #[test]
    fn scope_identity_and_revision_proof_mode_are_distinct() {
        let input = input();
        let mut scope = PublicationProofScope::new(
            input.tenant,
            input.workspace,
            input.principal_id,
            input.session_id,
        );
        assert_eq!(
            scope.require_identity(
                input.tenant,
                input.workspace,
                input.principal_id,
                input.session_id
            ),
            Ok(())
        );
        assert_eq!(
            scope.require_identity(
                input.tenant,
                input.workspace,
                input.principal_id,
                Uuid::from_u128(99)
            ),
            Err(Error::InternalInvariant)
        );
        let key = PublicationProofKey {
            unit_id: input.planned.unit_id,
            revision: 1,
            event_id: input.event_id,
            include_revision: false,
        };
        scope.verified.insert(
            key,
            VerifiedPublicationEvent {
                input,
                rdf_digest: "rdf-digest".into(),
            },
        );
        assert!(!scope.verified.contains_key(&PublicationProofKey {
            include_revision: true,
            ..key
        }));
    }
}
