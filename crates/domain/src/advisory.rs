//! Optional Jev advisory contracts and the durable Slice 0 audit records.
//!
//! These types deliberately describe an advisory opportunity separately from
//! a provider dispatch.  An opportunity is a workflow fact, including a
//! no-call decision; a dispatch is evidence of one authorized provider-send

mod audit;
mod config;
mod disposition;
mod lifecycle;
mod scope_manifest;
mod scope_source;
mod system_one;

pub use audit::*;
pub use config::*;
pub use disposition::*;
pub use lifecycle::*;
pub use scope_manifest::*;
pub use scope_source::*;
pub use system_one::*;

#[cfg(test)]
mod slice01_tests;
#[cfg(test)]
mod tests;
