//! Optional Jev advisory contracts and the durable Slice 0 audit records.
//!
//! These types deliberately describe an advisory opportunity separately from
//! a provider dispatch.  An opportunity is a workflow fact, including a
//! no-call decision; a dispatch is evidence of one authorized provider-send

mod anti_bloat;
mod audit;
mod authored_scope;
mod config;
mod disposition;
mod lifecycle;
mod pipeline_recommendation;
mod pipeline_recommendation_disposition;
mod scope_manifest;
mod scope_source;
mod selected_save_observation;
mod system_one;

pub use anti_bloat::*;
pub use audit::*;
pub use authored_scope::*;
pub use config::*;
pub use disposition::*;
pub use lifecycle::*;
pub use pipeline_recommendation::*;
pub use pipeline_recommendation_disposition::*;
pub use scope_manifest::*;
pub use scope_source::*;
pub use selected_save_observation::*;
pub use system_one::*;

#[cfg(test)]
mod slice01_tests;
#[cfg(test)]
mod tests;
