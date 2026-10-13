//! The product-level override of one Scope candidate or one Slice work node, through
//! a real daemon and MCP bridge against PostgreSQL. The daemon has no Jev provider:
//! the override does not depend on Jev.
#[path = "advice_sizing/drafts.rs"]
#[allow(dead_code)]
mod drafts;
#[path = "mode_override/migration.rs"]
mod migration;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[allow(dead_code)]
mod recovery_support;
#[path = "mode_override/replies.rs"]
mod replies;
#[path = "mode_override/scope.rs"]
mod scope;
#[path = "mode_override/slice.rs"]
mod slice;
#[path = "advice_sizing/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "mode_override/upgrade.rs"]
#[allow(dead_code)]
mod upgrade;
#[path = "mode_override/walk.rs"]
mod walk;
