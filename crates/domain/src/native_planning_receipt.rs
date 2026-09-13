use crate::{
    RecordSliceCandidateInput, RefreshSliceCandidateSet, ReviewSliceCandidateSet,
    SaveSliceCandidateDraft,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativePlanningReceiptRequest {
    SaveDraft(SaveSliceCandidateDraft),
    Review(ReviewSliceCandidateSet),
    RecordInput(RecordSliceCandidateInput),
    Refresh(RefreshSliceCandidateSet),
}

impl NativePlanningReceiptRequest {
    pub fn candidate_set_id(&self) -> Uuid {
        match self {
            Self::SaveDraft(value) => value.candidate_set_id,
            Self::Review(value) => value.candidate_set_id,
            Self::RecordInput(value) => value.candidate_set_id,
            Self::Refresh(value) => value.candidate_set_id,
        }
    }

    pub fn request_id(&self) -> Uuid {
        match self {
            Self::SaveDraft(value) => value.request_id,
            Self::Review(value) => value.request_id,
            Self::RecordInput(value) => value.request_id,
            Self::Refresh(value) => value.request_id,
        }
    }

    pub fn operation(&self) -> &'static str {
        match self {
            Self::SaveDraft(_) => "save_slice_draft",
            Self::Review(_) => "review_slice_set",
            Self::RecordInput(_) => "record_slice_input",
            Self::Refresh(_) => "refresh_slice_set",
        }
    }
}
