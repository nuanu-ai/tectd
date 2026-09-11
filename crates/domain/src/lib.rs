//! Pure identities, values and invariants. No environment, transport or persistence.
mod error;
mod identity;
mod program;
mod program_page;
mod state;

pub use program::{
    NewProgramInput, Program, ProgramStatus, ProgramStep, SaveProgram, TextPatch,
    validate_program_input,
};
pub use program_page::{ProgramCursor, ProgramInput, ProgramList, ProgramPage, ProgramSummary};

pub use error::{Error, Result};
pub use identity::{
    HostAuth, HostIdentity, RequestContext, validate_native_id, validate_workspace_key,
};
pub use state::{
    Created, EventKind, Session, StateStatus, Workspace, WorkspaceState, WorktreeSummary,
};

mod sources;
pub use sources::{
    MAX_SOURCE_PATH_BYTES, MAX_WORKTREES, RegisteredSource, SourceLocation, SourcePage,
    validate_selection,
};
