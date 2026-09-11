//! Native host configuration and bounded local transports.

mod context;
mod frame;
mod git;
mod mcp;
mod program_output;
mod program_tools;
mod responses;
mod tools;
mod transport;

pub use context::{HostContext, host_context_from_env};
pub use git::GitSourceInspector;
pub use mcp::run_stdio;
pub use transport::{call, call_tool, serve};

pub type Result<T> = tect_domain::Result<T>;
