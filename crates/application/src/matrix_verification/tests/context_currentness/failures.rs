use super::*;

fn valid_fixture() -> (ContextStore, MatrixTaskRequirementsBinding) {
    let first = revision(1, EngineeringMode::Mvp);
    let effective = resolve_matrix_requirements(
        &[anchor()],
        std::slice::from_ref(&first),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let snapshot_id = Uuid::from_u128(2);
    let frozen = FrozenMatrixRequirementsContext {
        id: snapshot_id,
        anchor: anchor(),
        effective: effective.clone(),
        payload_sha256: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&effective).unwrap())
        ),
    };
    let binding = MatrixTaskRequirementsBinding {
        locator: MatrixRequirementsLocator::Program {
            program_id: Uuid::from_u128(1),
        },
        snapshot_id,
        semantic_digest: effective.semantic_digest().into(),
        authority_schema: MATRIX_REQUIREMENTS_SCHEMA.into(),
    };
    (
        ContextStore {
            frozen,
            revisions: vec![first],
            stale_work_revision: false,
            locks: Vec::new(),
            freeze_saw_lock: false,
        },
        binding,
    )
}

#[tokio::test]
async fn missing_snapshot_is_snapshot_missing() {
    let (mut store, mut binding) = valid_fixture();
    binding.snapshot_id = Uuid::from_u128(3);
    assert!(matches!(
        lock_and_load_bound_matrix_context(&mut store, Uuid::new_v4(), Uuid::new_v4(), &binding)
            .await,
        Err(BoundContextFailure::SnapshotMissing),
    ));
}

#[tokio::test]
async fn corrupted_frozen_payload_is_binding_mismatch() {
    let (mut store, binding) = valid_fixture();
    store.frozen.payload_sha256 = "0".repeat(64);
    assert!(matches!(
        lock_and_load_bound_matrix_context(&mut store, Uuid::new_v4(), Uuid::new_v4(), &binding)
            .await,
        Err(BoundContextFailure::BindingMismatch),
    ));
}

#[tokio::test]
async fn unsupported_authority_schema_is_authority_schema_unsupported() {
    let (mut store, mut binding) = valid_fixture();
    binding.authority_schema = "unsupported-schema".into();
    assert!(matches!(
        lock_and_load_bound_matrix_context(&mut store, Uuid::new_v4(), Uuid::new_v4(), &binding)
            .await,
        Err(BoundContextFailure::AuthoritySchemaUnsupported),
    ));
}
