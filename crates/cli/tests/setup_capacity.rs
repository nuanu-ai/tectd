//! Real 8 MiB setup capacity, paging, and legacy Program compatibility proof.

mod recovery_support;

#[path = "setup_capacity/baseline.rs"]
mod baseline;
#[path = "setup_capacity/current.rs"]
mod current;
#[path = "setup_capacity/legacy.rs"]
mod legacy;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_capacity_preserves_exact_input_and_rolls_back_refusals() {
    current::run().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit pre-setup baseline compatibility acceptance"]
async fn legacy_program_capacity_remains_recoverable() {
    baseline::run().await;
}
