// Owned synthetic Matrix transport fixture. No real credentials or Owner signature.
use ring::signature::{Ed25519KeyPair,KeyPair};
use tokio::net::TcpListener;
fn signed_test_budget(
    workspace: Uuid,
    owner: Uuid,
) -> (AdvisoryBudgetPolicy, crate::BudgetOwnerKeys) {
    // Same test-only signing mechanics as Scope's private fixture. No real Owner key.
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let (from, until, id) = (now - 60_000, now + 3_600_000, Uuid::new_v4());
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 1024,
        output_tokens: 1024,
        request_utf8_bytes: 45_000 as i64,
        elapsed_monotonic_ms: 10_000,
        retry_dispatches: 1,
    };
    let digest = AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest.clone(),
        from,
        until,
        ceilings,
        owner,
        "0".repeat(128),
    )
    .unwrap();
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    let signature = pair.sign(&unsigned.approval_signing_message(workspace).unwrap());
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        digest,
        from,
        until,
        ceilings,
        owner,
        hex(signature.as_ref()),
    )
    .unwrap();
    let keys = crate::BudgetOwnerKeys::from_json(
        &json!([{
            "workspace_id": workspace, "owner_id": owner,
            "public_key_hex": hex(pair.public_key().as_ref()),
        }])
        .to_string(),
    )
    .unwrap();
    (signed, keys)
}


