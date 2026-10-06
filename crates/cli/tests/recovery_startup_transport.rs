#![allow(dead_code)]
// Dedicated controlled B1 fixtures; not injected into product integration suites.
include!("recovery_support/mod.rs");
#[path = "recovery_support/startup_tests.rs"]
mod startup_tests;
#[path = "recovery_support/wire_tests.rs"]
mod wire_tests;
