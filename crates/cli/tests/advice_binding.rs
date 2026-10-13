//! Jev's advice binds: what a draft save keeps of it, and how a ready review is held
//! to it, on `scope.candidates.save` and `slice.candidates.save` through a real daemon
//! and MCP bridge against PostgreSQL. Jev is a loopback fake; nothing leaves the machine.
#[path = "advice_binding/support.rs"]
#[allow(dead_code)]
mod binding;
#[path = "advice_sizing/drafts.rs"]
#[allow(dead_code)]
mod drafts;
#[path = "advice_binding/kept.rs"]
mod kept;
#[path = "advice_binding/migration.rs"]
mod migration;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_sizing/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "mode_override/upgrade.rs"]
#[allow(dead_code)]
mod upgrade;
#[path = "mode_override/walk.rs"]
#[allow(dead_code)]
mod walk;
