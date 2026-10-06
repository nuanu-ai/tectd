#![allow(dead_code)]
// Dedicated finite cleanup integration binary; no tests injected into other suites.
include!("recovery_support/mod.rs");
#[path = "recovery_support/cleanup_tests.rs"]
mod cleanup_tests;
