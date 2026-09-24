use crate::advisory_tools::{AdvisoryInvocation, MatrixCardDetail};
use crate::{Result, responses};
use tect_application::WorkspaceService;
use tect_domain::RequestContext;

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: AdvisoryInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<serde_json::Value> {
    let value = match invocation {
        AdvisoryInvocation::MatrixCard {
            task_id,
            expected_task_revision,
            card_id,
            detail,
        } => {
            let composition = service
                .compose_matrix_cards(context, task_id, expected_task_revision)
                .await?;
            serde_json::to_value(matrix_card_response(
                composition,
                card_id.as_deref(),
                detail,
            )?)
        }
        AdvisoryInvocation::VerifySelectedSave(request) => {
            let observation = service.verify_selected_save(context, &request).await?;
            serde_json::to_value(serde_json::json!({
                "observation": observation,
                "establishes_independent_approval": false,
                "establishes_current_acceptance": false,
            }))
        }
        AdvisoryInvocation::ScopeRequest(request) => {
            let outcome = service.run_scope_advisory(context, &request).await?;
            serde_json::to_value(serde_json::json!({
                "request_id": request.request_id,
                "opportunity_id": outcome.opportunity.id,
                "candidate_set_id": request.candidate_set_id,
                "state": outcome.opportunity.state,
                "reason": outcome.opportunity.primary_reason,
                "provider_called": outcome.opportunity.provider_called,
                "advice_id": outcome.advice.as_ref().map(|advice| &advice.id),
            }))
        }
        AdvisoryInvocation::ScopeDisposition {
            opportunity_id,
            candidate_set_id,
            request,
        } => serde_json::to_value(
            service
                .decide_scope_advisory(context, opportunity_id, candidate_set_id, request)
                .await?,
        ),
        AdvisoryInvocation::Config => serde_json::to_value(service.advisory_config(context).await?),
        AdvisoryInvocation::Configure(request) => {
            serde_json::to_value(service.configure_advisory(context, &request).await?)
        }
        AdvisoryInvocation::WorkspaceAudit(query) => {
            serde_json::to_value(service.advisory_audit(context, &query).await?)
        }
        AdvisoryInvocation::ScopeAudit { scope_id, query } => serde_json::to_value(
            service
                .scope_advisory_audit(context, scope_id, &query)
                .await?,
        ),
        AdvisoryInvocation::ScopeGet {
            scope_id,
            opportunity_id,
        } => serde_json::to_value(
            service
                .scope_advisory_get(context, scope_id, opportunity_id)
                .await?,
        ),
        AdvisoryInvocation::CandidateAudit {
            candidate_set_id,
            query,
        } => serde_json::to_value(
            service
                .candidate_advisory_audit(context, candidate_set_id, &query)
                .await?,
        ),
        AdvisoryInvocation::CandidateGet {
            candidate_set_id,
            opportunity_id,
        } => serde_json::to_value(
            service
                .candidate_advisory_get(context, candidate_set_id, opportunity_id)
                .await?,
        ),
    }
    .map_err(tect_domain::Error::invalid_arguments_from)?;
    let value = responses::with_actions(value, Vec::new(), None);
    if responses::encoded_len(&value)? > capacity {
        return Err(tect_domain::Error::RequestTooLarge);
    }
    Ok(value)
}

fn matrix_card_response(
    composition: tect_domain::EngineeringMatrixComposition,
    card_id: Option<&str>,
    detail: MatrixCardDetail,
) -> Result<serde_json::Value> {
    let selected = card_id
        .map(|id| {
            composition
                .mandatory_cards
                .iter()
                .find(|card| card.id == id)
                .ok_or(tect_domain::Error::NotFound)
        })
        .transpose()?;
    let cards: Vec<_> = composition
        .mandatory_cards
        .iter()
        .map(|card| serde_json::json!({"id": card.id, "summary": card.summary}))
        .collect();
    let selected_card = if detail == MatrixCardDetail::Full {
        let card = selected.ok_or(tect_domain::Error::InvalidArguments)?;
        Some(serde_json::json!({"id":card.id,"summary":card.summary,"body":card.body}))
    } else {
        None
    };
    Ok(serde_json::json!({
        "catalogue_version": composition.catalogue_version,
        "task_id": composition.task_id,
        "task_revision": composition.task_revision,
        "mandatory_cards": cards,
        "unresolved_evidence": composition.unresolved_evidence,
        "selected_card": selected_card,
    }))
}

#[cfg(test)]
mod matrix_card_tests {
    use super::*;
    use tect_domain::{
        EngineeringMatrixComposition, MandatoryMatrixCard, MatrixEvidenceState,
        UnresolvedMatrixEvidence,
    };

    fn composition() -> EngineeringMatrixComposition {
        EngineeringMatrixComposition {
            catalogue_version: "EM02-INITIAL@0.1",
            task_id: uuid::Uuid::new_v4().to_string(),
            task_revision: "3".into(),
            mandatory_cards: vec![MandatoryMatrixCard {
                id: "EM02-SCOPE@0.1",
                catalogue_version: "EM02-INITIAL@0.1",
                summary: "Scope summary",
                body: "Complete scope body",
            }],
            unresolved_evidence: vec![UnresolvedMatrixEvidence {
                field: "mode".into(),
                state: MatrixEvidenceState::Absent,
                provenance: None,
            }],
        }
    }

    #[test]
    fn summary_and_full_preserve_version_ids_evidence_and_body_boundary() {
        let summary = matrix_card_response(composition(), None, MatrixCardDetail::Summary).unwrap();
        assert_eq!(summary["catalogue_version"], "EM02-INITIAL@0.1");
        assert_eq!(summary["task_revision"], "3");
        assert_eq!(summary["mandatory_cards"][0]["id"], "EM02-SCOPE@0.1");
        assert_eq!(summary["mandatory_cards"][0]["summary"], "Scope summary");
        assert!(summary["mandatory_cards"][0].get("body").is_none());
        assert_eq!(summary["unresolved_evidence"][0]["field"], "mode");
        assert!(summary["selected_card"].is_null());
        let full = matrix_card_response(
            composition(),
            Some("EM02-SCOPE@0.1"),
            MatrixCardDetail::Full,
        )
        .unwrap();
        assert_eq!(full["selected_card"]["body"], "Complete scope body");
        assert_eq!(summary["mandatory_cards"], full["mandatory_cards"]);
        assert!(
            crate::responses::encoded_len(&crate::responses::with_actions(full, Vec::new(), None))
                .unwrap()
                < crate::frame::MAX_FRAME_BYTES
        );
    }

    #[test]
    fn absent_conditional_card_is_not_returned_as_selected() {
        assert!(
            matrix_card_response(
                composition(),
                Some("EM02-HOTFIX@0.1"),
                MatrixCardDetail::Full
            )
            .is_err()
        );
    }
}
