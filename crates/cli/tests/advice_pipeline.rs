//! The `pipeline` advice point on `slice.candidates.save`, a second point on that
//! command after `sizing`, through a real daemon and MCP bridge against PostgreSQL.
//! Jev is a loopback fake; nothing leaves the machine.
#[path = "advice_pipeline/answers.rs"]
mod answers;
#[path = "advice_sizing/drafts.rs"]
#[allow(dead_code)]
mod drafts;
#[path = "advice_pipeline/first_draft.rs"]
mod first_draft;
#[path = "advice_pipeline/limits.rs"]
mod limits;
#[path = "advice_pipeline/live.rs"]
mod live;
#[path = "advice_pipeline/support.rs"]
#[allow(dead_code)]
mod pipeline;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_pipeline/second_draft.rs"]
mod second_draft;
#[path = "advice_sizing/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "advice_pipeline/unavailable.rs"]
mod unavailable;
#[path = "mode_override/walk.rs"]
#[allow(dead_code)]
mod walk;
