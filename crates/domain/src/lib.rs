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

mod setup;
mod setup_file;
mod setup_page;
pub use setup::{NewSetupInput, SaveSetup, Setup, SetupStatus, SetupStep, validate_setup_input};
pub use setup_file::{
    FileObservation, FilePublication, PublicationOutcome, SetupDirectory, SetupFileStatus,
    setup_path_is_granted, validate_setup_path,
};
pub use setup_page::{
    AppliedSetup, SetupContext, SetupDiscovery, SetupInput, SetupPage, SetupSummary,
};
