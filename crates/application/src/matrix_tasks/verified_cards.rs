use super::*;
use tect_domain::{
    ContextEngineeringMatrixComposition, ContextMatrixResolutionStatus, MandatoryMatrixCard,
};

pub const VERIFIED_MATRIX_CARDS_SCHEMA: &str = "tect.engineering-matrix-verified-cards/1";

/// The digest is mandatory: a caller cannot accidentally read a different
/// verification than the one it inspected before requesting full card text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetVerifiedMatrixCards {
    pub task_id: Uuid,
    pub expected_task_revision: i64,
    pub operating_verification_digest: String,
    pub card_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMatrixCardSummary {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMatrixCards {
    pub schema: &'static str,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub input_digest: String,
    pub catalogue_version: String,
    pub mandatory_cards: Vec<VerifiedMatrixCardSummary>,
    pub selected_card: Option<MandatoryMatrixCard>,
    pub resolution_status: ContextMatrixResolutionStatus,
    pub frozen_snapshot_id: Uuid,
    pub authority_schema: String,
    pub requirements_semantic_digest: String,
    pub operating_verification_digest: String,
    pub policy_version: String,
}

impl WorkspaceService {
    /// V2-only, authenticated read. Legacy owner-report cards are never a
    /// fallback for a missing, stale, or unvalidated V2 operating record.
    pub async fn get_verified_matrix_cards(
        &self,
        context: &RequestContext,
        request: &GetVerifiedMatrixCards,
    ) -> Result<VerifiedMatrixCards> {
        validate_read_request(request)?;
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let source = tx
            .matrix_task_source(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let binding = require_current_bound_source(&source, request)?;
        let context_store = tx
            .matrix_requirements_context_store()
            .ok_or(Error::StorageUnavailable)?;
        let effective = crate::matrix_verification::load_bound_matrix_context(
            context_store,
            workspace.id,
            identity.principal_id,
            binding,
        )
        .await
        .map_err(|_| Error::StaleRevision)?;
        let (composition, record) = binding::compose_bound_revision_with_verification(
            tx.context_matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            &source.revision,
            binding.snapshot_id,
            &effective,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await?
        .ok_or(Error::Forbidden)?;
        if record.digest != request.operating_verification_digest
            || composition.operating_verification_digest() != request.operating_verification_digest
        {
            return Err(Error::StaleRevision);
        }
        let result =
            project_verified_cards(&source, &composition, &record.policy_version, request)?;
        tx.commit().await?;
        Ok(result)
    }
}

fn validate_read_request(request: &GetVerifiedMatrixCards) -> Result<()> {
    if request.task_id.is_nil()
        || request.expected_task_revision < 1
        || request.operating_verification_digest.len() != 64
        || !request
            .operating_verification_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || request
            .card_id
            .as_deref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn require_current_bound_source<'a>(
    source: &'a MatrixTaskSource,
    request: &GetVerifiedMatrixCards,
) -> Result<&'a MatrixTaskRequirementsBinding> {
    if source.revision.task_id != request.task_id
        || source.revision.revision != request.expected_task_revision
    {
        return Err(Error::StaleRevision);
    }
    source.requirements_binding.as_ref().ok_or(Error::Forbidden)
}

fn project_verified_cards(
    source: &MatrixTaskSource,
    verified: &ContextEngineeringMatrixComposition,
    policy_version: &str,
    request: &GetVerifiedMatrixCards,
) -> Result<VerifiedMatrixCards> {
    let binding = require_current_bound_source(source, request)?;
    if verified.status()
        != ContextMatrixResolutionStatus::ConfirmedRequirementsValidatedOperatingEvidence
        || !verified.is_resolved()
        || verified.frozen_snapshot_id() != binding.snapshot_id.to_string()
        || verified.authority_schema() != binding.authority_schema
        || verified.requirements_semantic_digest() != binding.semantic_digest
        || verified.operating_verification_digest() != request.operating_verification_digest
        || verified.composition().task_id != request.task_id.to_string()
        || verified.composition().task_revision != request.expected_task_revision.to_string()
    {
        return Err(Error::StaleRevision);
    }
    let cards = &verified.composition().mandatory_cards;
    let selected_card = select_mandatory_card(cards, request.card_id.as_deref())?;
    Ok(VerifiedMatrixCards {
        schema: VERIFIED_MATRIX_CARDS_SCHEMA,
        task_id: source.revision.task_id,
        task_revision: source.revision.revision,
        input_digest: source.revision.input_digest.clone(),
        catalogue_version: verified.composition().catalogue_version.to_owned(),
        mandatory_cards: cards
            .iter()
            .map(|card| VerifiedMatrixCardSummary {
                id: card.id.to_owned(),
                summary: card.summary.to_owned(),
            })
            .collect(),
        selected_card,
        resolution_status: verified.status(),
        frozen_snapshot_id: binding.snapshot_id,
        authority_schema: binding.authority_schema.clone(),
        requirements_semantic_digest: binding.semantic_digest.clone(),
        operating_verification_digest: request.operating_verification_digest.clone(),
        policy_version: policy_version.to_owned(),
    })
}

fn select_mandatory_card(
    cards: &[MandatoryMatrixCard],
    card_id: Option<&str>,
) -> Result<Option<MandatoryMatrixCard>> {
    card_id
        .map(|id| {
            cards
                .iter()
                .find(|card| card.id == id)
                .cloned()
                .ok_or(Error::NotFound)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{EngineeringMatrixInput, MatrixFact, OperatingEnvelope, OperationalFacts};

    fn source(bound: bool) -> MatrixTaskSource {
        let task_id = Uuid::new_v4();
        let input = EngineeringMatrixInput {
            mode: MatrixFact::Absent,
            envelope: OperatingEnvelope {
                scale: MatrixFact::Absent,
                operational_facts: OperationalFacts::Absent,
            },
            criticality: MatrixFact::Absent,
            intent: MatrixFact::Absent,
            urgency: MatrixFact::Absent,
            promised_behavior: MatrixFact::Absent,
            promised_proof: MatrixFact::Absent,
            affected_guarantees: MatrixFact::Absent,
            actual_exposure: MatrixFact::Absent,
            demand_commitment: MatrixFact::Absent,
            latency_commitment: MatrixFact::Absent,
            urgent_repair: MatrixFact::Absent,
        };
        MatrixTaskSource {
            revision: MatrixTaskRevision {
                task_id,
                revision: 1,
                request_id: Uuid::new_v4(),
                input,
                input_digest: "a".repeat(64),
                choice_set: None,
                choice_set_digest: None,
                recorded_by_principal_id: Uuid::new_v4(),
                recorded_by_session_id: Uuid::new_v4(),
            },
            requirements_binding: bound.then(|| MatrixTaskRequirementsBinding {
                locator: MatrixRequirementsLocator::Program {
                    program_id: Uuid::new_v4(),
                },
                snapshot_id: Uuid::new_v4(),
                semantic_digest: "b".repeat(64),
                authority_schema: tect_domain::MATRIX_REQUIREMENTS_SCHEMA.into(),
            }),
        }
    }

    fn request(source: &MatrixTaskSource) -> GetVerifiedMatrixCards {
        GetVerifiedMatrixCards {
            task_id: source.revision.task_id,
            expected_task_revision: 1,
            operating_verification_digest: "c".repeat(64),
            card_id: None,
        }
    }

    #[test]
    fn legacy_source_never_falls_back_to_v1_cards() {
        let legacy = source(false);
        assert_eq!(
            require_current_bound_source(&legacy, &request(&legacy)),
            Err(Error::Forbidden)
        );
        let bound = source(true);
        assert!(require_current_bound_source(&bound, &request(&bound)).is_ok());
    }

    #[test]
    fn wrong_task_revision_or_digest_fails_before_read() {
        let bound = source(true);
        let mut read = request(&bound);
        read.expected_task_revision = 2;
        assert_eq!(
            require_current_bound_source(&bound, &read),
            Err(Error::StaleRevision)
        );
        read.expected_task_revision = 1;
        read.task_id = Uuid::new_v4();
        assert_eq!(
            require_current_bound_source(&bound, &read),
            Err(Error::StaleRevision)
        );
        read.task_id = bound.revision.task_id;
        read.operating_verification_digest.clear();
        assert_eq!(validate_read_request(&read), Err(Error::InvalidArguments));
    }

    #[test]
    fn only_mandatory_card_body_can_be_selected() {
        let cards = [MandatoryMatrixCard {
            id: "EM02-SCOPE@0.1",
            catalogue_version: "EM02-INITIAL@0.1",
            summary: "scope",
            body: "full scope body",
        }];
        assert_eq!(select_mandatory_card(&cards, None).unwrap(), None);
        assert_eq!(
            select_mandatory_card(&cards, Some("EM02-SCOPE@0.1"))
                .unwrap()
                .unwrap()
                .body,
            "full scope body"
        );
        assert_eq!(
            select_mandatory_card(&cards, Some("EM02-CAPACITY@0.1")),
            Err(Error::NotFound)
        );
    }
}
