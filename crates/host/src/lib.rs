//! Native host configuration and bounded local transports.

mod context;
mod frame;
mod mcp;
mod transport;

pub use context::context_from_env;
pub use mcp::run_stdio;
pub use transport::{call, serve};

pub type Result<T> = tect_domain::Result<T>;
