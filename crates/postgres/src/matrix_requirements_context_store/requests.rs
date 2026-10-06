use super::*;
use tect_application::{ConfirmMatrixRequirementsContext, ProposeMatrixRequirementsContext};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "level", rename_all = "snake_case", deny_unknown_fields)]
enum LocatorJson {
    Program {
        program_id: Uuid,
    },
    Scope {
        program_id: Uuid,
        scope_id: Uuid,
    },
    Slice {
        program_id: Uuid,
        scope_id: Uuid,
        candidate_set_id: Uuid,
        work_candidate_id: Uuid,
        expected_work_revision: i64,
    },
    OpenedSlice {
        slice_id: Uuid,
    },
}
impl From<&MatrixRequirementsLocator> for LocatorJson {
    fn from(value: &MatrixRequirementsLocator) -> Self {
        match *value {
            MatrixRequirementsLocator::Program { program_id } => Self::Program { program_id },
            MatrixRequirementsLocator::Scope {
                program_id,
                scope_id,
            } => Self::Scope {
                program_id,
                scope_id,
            },
            MatrixRequirementsLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            MatrixRequirementsLocator::OpenedSlice { slice_id } => Self::OpenedSlice { slice_id },
        }
    }
}
impl From<LocatorJson> for MatrixRequirementsLocator {
    fn from(value: LocatorJson) -> Self {
        match value {
            LocatorJson::Program { program_id } => Self::Program { program_id },
            LocatorJson::Scope {
                program_id,
                scope_id,
            } => Self::Scope {
                program_id,
                scope_id,
            },
            LocatorJson::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            LocatorJson::OpenedSlice { slice_id } => Self::OpenedSlice { slice_id },
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalRequestJson {
    request_id: Uuid,
    locator: LocatorJson,
    expected_context_revision: u64,
    patches: Vec<RequirementDeclarationPatch>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmationRequestJson {
    request_id: Uuid,
    locator: LocatorJson,
    proposal_revision: u64,
    proposal_digest: String,
    owner_response_ref: String,
}
pub(super) fn proposal_request_json(request: &ProposeMatrixRequirementsContext) -> Result<Value> {
    json(&ProposalRequestJson {
        request_id: request.request_id,
        locator: (&request.locator).into(),
        expected_context_revision: request.expected_context_revision,
        patches: request.patches.clone(),
    })
}
pub(super) fn confirmation_request_json(
    request: &ConfirmMatrixRequirementsContext,
) -> Result<Value> {
    json(&ConfirmationRequestJson {
        request_id: request.request_id,
        locator: (&request.locator).into(),
        proposal_revision: request.proposal_revision,
        proposal_digest: request.proposal_digest.clone(),
        owner_response_ref: request.owner_response_ref.clone(),
    })
}
pub(super) fn decode_proposal_request(value: Value) -> Result<ProposeMatrixRequirementsContext> {
    let request: ProposalRequestJson =
        serde_json::from_value(value).map_err(|_| Error::InputConflict)?;
    signed(request.expected_context_revision)?;
    Ok(ProposeMatrixRequirementsContext {
        request_id: request.request_id,
        locator: request.locator.into(),
        expected_context_revision: request.expected_context_revision,
        patches: request.patches,
    })
}
pub(super) fn decode_confirmation_request(
    value: Value,
) -> Result<ConfirmMatrixRequirementsContext> {
    let request: ConfirmationRequestJson =
        serde_json::from_value(value).map_err(|_| Error::InputConflict)?;
    signed(request.proposal_revision)?;
    Ok(ConfirmMatrixRequirementsContext {
        request_id: request.request_id,
        locator: request.locator.into(),
        proposal_revision: request.proposal_revision,
        proposal_digest: request.proposal_digest,
        owner_response_ref: request.owner_response_ref,
    })
}
