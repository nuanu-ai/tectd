use crate::{
    CandidateGuidance, NativePlanningGuidance, NativePlanningOutputGuard, TransactionMode,
    UnitOfWork, WorkspaceService,
};
use tect_domain::{
    Error, NativeScope, NativeSlice, OpenScope, OpenScopeOutcome, OpenSlice, OpenSliceOutcome,
    RecordSliceCandidateInput, RecordSliceResult, RecordSliceResultOutcome,
    RefreshSliceCandidateSet, Result, ReviewSliceCandidateSet, SaveSliceCandidateDraft,
    SliceCandidateContext, SliceCandidateContextQuery,
};

mod candidate;
mod lifecycle;
mod matrix_selection;
pub(crate) use matrix_selection::validate_selected_matrix_plan;
mod validation;

use validation::*;
