//! Native host configuration and bounded local transports.

mod api;
mod context;
mod frame;
mod git;
mod mcp;
mod program_output;
mod program_tools;
mod responses;
mod scope_candidate_dispatch;
mod scope_candidate_output;
mod scope_candidate_tools;
mod scope_guidance;
mod setup_dispatch;
mod setup_files;
mod setup_output;
mod setup_recovery;
mod setup_tools;
mod slice_dispatch;
mod slice_guidance;
mod slice_pipeline_catalog;
mod slice_tools;
mod tools;
mod transport;
mod workspace_output;

pub use context::{HostContext, host_context_from_env};
pub use git::GitSourceInspector;
pub use mcp::run_stdio;
#[doc(hidden)]
pub use scope_guidance::CandidateEncoding;
pub use setup_files::LocalSetupFiles;
pub use transport::{call, call_tool, serve};

pub type Result<T> = tect_domain::Result<T>;
