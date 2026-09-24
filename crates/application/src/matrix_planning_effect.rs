use crate::{
    MatrixPlanningEffectAttestation, MatrixPlanningEffectSnapshot, MatrixPlanningEffectVerdict,
    TransactionMode, WorkspaceService,
};
use tect_domain::{Error, MatrixPlanningEffectMaterial, PrincipalRole, RequestContext, Result};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningEffectRead {
    pub material: MatrixPlanningEffectMaterial,
    pub effect_digest: String,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyMatrixPlanningEffect {
    pub request_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_request_id: Uuid,
    pub expected_result_revision: i64,
    pub expected_effect_digest: String,
    pub verdict: MatrixPlanningEffectVerdict,
    pub summary: String,
}

impl VerifyMatrixPlanningEffect {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.candidate_set_id.is_nil()
            || self.caller_request_id.is_nil()
            || self.expected_result_revision < 1
            || self.expected_effect_digest.len() != 64
            || !self
                .expected_effect_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.summary.is_empty()
            || self.summary.len() > 4096
            || self.summary.trim() != self.summary
            || self.summary.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl WorkspaceService {
    /// Exact selected choice and persisted mapped node bodies for a separate
    /// verifier. The digest comes from saved content, never from caller text.
    pub async fn get_matrix_planning_effect(
        &self,
        context: &RequestContext,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
    ) -> Result<MatrixPlanningEffectRead> {
        if candidate_set_id.is_nil() || caller_request_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let snapshot = tx
            .matrix_planning_effect_store()
            .ok_or(Error::StorageUnavailable)?
            .matrix_planning_effect_snapshot(
                workspace.id,
                candidate_set_id,
                caller_request_id,
                false,
            )
            .await?
            .ok_or(Error::NotFound)?;
        let material = snapshot.material(workspace.id)?;
        let effect_digest = material.canonical_digest()?;
        tx.commit().await?;
        Ok(MatrixPlanningEffectRead {
            material,
            effect_digest,
            verifier_principal_id: identity.principal_id,
            verifier_session_id: session.id,
        })
    }

    /// Appends an independent attestation; it never changes planning readiness.
    pub async fn verify_matrix_planning_effect(
        &self,
        context: &RequestContext,
        request: &VerifyMatrixPlanningEffect,
    ) -> Result<MatrixPlanningEffectAttestation> {
        request.validate()?;
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let store = tx
            .matrix_planning_effect_store()
            .ok_or(Error::StorageUnavailable)?;
        if let Some(existing) = store
            .matrix_planning_effect_attestation_by_request(workspace.id, request.request_id)
            .await?
        {
            if existing.matches_request(workspace.id, identity.principal_id, session.id, request) {
                tx.commit().await?;
                return Ok(existing);
            }
            return Err(Error::InputConflict);
        }
        let snapshot = store
            .matrix_planning_effect_snapshot(
                workspace.id,
                request.candidate_set_id,
                request.caller_request_id,
                true,
            )
            .await?
            .ok_or(Error::NotFound)?;
        let attestation = checked_attestation(
            workspace.id,
            identity.principal_id,
            session.id,
            request,
            &snapshot,
        )?;
        store
            .append_matrix_planning_effect_attestation(workspace.id, &attestation)
            .await?;
        tx.commit().await?;
        Ok(attestation)
    }
}

impl MatrixPlanningEffectAttestation {
    fn matches_request(
        &self,
        workspace_id: Uuid,
        verifier_principal_id: Uuid,
        verifier_session_id: Uuid,
        request: &VerifyMatrixPlanningEffect,
    ) -> bool {
        self.workspace_id == workspace_id
            && self.request_id == request.request_id
            && self.candidate_set_id == request.candidate_set_id
            && self.caller_request_id == request.caller_request_id
            && self.expected_result_revision == request.expected_result_revision
            && self.effect_digest == request.expected_effect_digest
            && self.verifier_principal_id == verifier_principal_id
            && self.verifier_session_id == verifier_session_id
            && self.verdict == request.verdict
            && self.summary == request.summary
    }
}

fn checked_attestation(
    workspace_id: Uuid,
    verifier_principal_id: Uuid,
    verifier_session_id: Uuid,
    request: &VerifyMatrixPlanningEffect,
    snapshot: &MatrixPlanningEffectSnapshot,
) -> Result<MatrixPlanningEffectAttestation> {
    let material = snapshot.material(workspace_id)?;
    if material.candidate_set_id != request.candidate_set_id
        || material.caller_request_id != request.caller_request_id
    {
        return Err(Error::StaleContext);
    }
    if material.result_revision != request.expected_result_revision {
        return Err(Error::StaleRevision);
    }
    if material.canonical_digest()? != request.expected_effect_digest {
        return Err(Error::InputConflict);
    }
    if verifier_principal_id == material.caller_principal_id
        || verifier_principal_id == material.matrix_owner_principal_id
    {
        return Err(Error::Forbidden);
    }
    Ok(MatrixPlanningEffectAttestation {
        request_id: request.request_id,
        workspace_id,
        candidate_set_id: request.candidate_set_id,
        caller_request_id: request.caller_request_id,
        expected_result_revision: request.expected_result_revision,
        effect_digest: request.expected_effect_digest.clone(),
        verifier_principal_id,
        verifier_session_id,
        verdict: request.verdict,
        summary: request.summary.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MatrixPlanningMappedNode, MatrixPlanningSelectionLink};
    use tect_domain::{EngineeringCandidate, MatrixPlanningSelection, SliceCandidateNode};

    fn setup() -> (
        Uuid,
        MatrixPlanningEffectSnapshot,
        VerifyMatrixPlanningEffect,
    ) {
        let workspace_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let selected_choice = EngineeringCandidate {
            candidate_id: "option-a".into(),
            title: "Option A".into(),
            approach: "Apply A".into(),
            assumption_fact_ids: vec!["scale".into()],
        };
        let link = MatrixPlanningSelectionLink {
            selection: MatrixPlanningSelection {
                task_id: Uuid::new_v4(),
                task_revision: 2,
                disposition_id: Uuid::new_v4(),
                selected_choice_id: selected_choice.candidate_id.clone(),
                expected_input_digest: "a".repeat(64),
                expected_choice_set_digest: "b".repeat(64),
                expected_verification_digest: "c".repeat(64),
                mapped_draft_node_indices: vec![0],
            },
            evaluation_digest: "d".repeat(64),
            catalogue_version: "EM@1".into(),
            caller_principal_id: Uuid::new_v4(),
            caller_session_id: Uuid::new_v4(),
            scope_id: Uuid::new_v4(),
            candidate_set_id: Uuid::new_v4(),
            caller_request_id: Uuid::new_v4(),
            result_revision: 3,
            mapped_nodes: vec![MatrixPlanningMappedNode {
                draft_index: 0,
                node_id,
                node_revision: 1,
            }],
        };
        let snapshot = MatrixPlanningEffectSnapshot {
            link,
            receipt_present: true,
            selected_choice,
            matrix_owner_principal_id: Uuid::new_v4(),
            saved_nodes: vec![SliceCandidateNode::Decision {
                id: node_id,
                revision: 1,
                title: "Storage".into(),
                question: "Use A?".into(),
                resolution_criteria: vec!["Proof".into()],
                dependencies: vec![],
                source_result_ids: vec![],
            }],
            current_result_revision: 3,
            is_current: true,
        };
        let request = VerifyMatrixPlanningEffect {
            request_id: Uuid::new_v4(),
            candidate_set_id: snapshot.link.candidate_set_id,
            caller_request_id: snapshot.link.caller_request_id,
            expected_result_revision: 3,
            expected_effect_digest: snapshot.effect_digest(workspace_id).unwrap(),
            verdict: MatrixPlanningEffectVerdict::Matches,
            summary: "The saved node implements option A.".into(),
        };
        (workspace_id, snapshot, request)
    }

    #[test]
    fn independent_attestation_requires_exact_saved_content_and_actor() {
        let (workspace, mut snapshot, request) = setup();
        let verifier = Uuid::new_v4();
        let session = Uuid::new_v4();
        let record =
            checked_attestation(workspace, verifier, session, &request, &snapshot).unwrap();
        assert!(record.matches_request(workspace, verifier, session, &request));
        assert!(!record.matches_request(workspace, verifier, Uuid::new_v4(), &request));
        assert_eq!(
            checked_attestation(
                workspace,
                snapshot.link.caller_principal_id,
                session,
                &request,
                &snapshot
            ),
            Err(Error::Forbidden)
        );
        assert_eq!(
            checked_attestation(
                workspace,
                snapshot.matrix_owner_principal_id,
                session,
                &request,
                &snapshot
            ),
            Err(Error::Forbidden)
        );
        if let SliceCandidateNode::Decision { question, .. } = &mut snapshot.saved_nodes[0] {
            question.push_str(" Changed.");
        }
        assert_eq!(
            checked_attestation(workspace, verifier, session, &request, &snapshot),
            Err(Error::InputConflict)
        );
    }

    #[test]
    fn legacy_erased_or_stale_mapping_cannot_be_attested() {
        let (workspace, mut snapshot, request) = setup();
        let verifier = Uuid::new_v4();
        let session = Uuid::new_v4();
        snapshot.receipt_present = false;
        assert_eq!(
            checked_attestation(workspace, verifier, session, &request, &snapshot),
            Err(Error::StaleContext)
        );
        snapshot.receipt_present = true;
        snapshot.link.mapped_nodes.clear();
        assert_eq!(
            checked_attestation(workspace, verifier, session, &request, &snapshot),
            Err(Error::StaleContext)
        );
        snapshot.link.mapped_nodes.push(MatrixPlanningMappedNode {
            draft_index: 0,
            node_id: snapshot.saved_nodes[0].id(),
            node_revision: snapshot.saved_nodes[0].revision(),
        });
        snapshot.current_result_revision += 1;
        assert_eq!(
            checked_attestation(workspace, verifier, session, &request, &snapshot),
            Err(Error::StaleContext)
        );
    }
}
