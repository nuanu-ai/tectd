use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use resource::fixture;

fn test_page(manifest_id: Uuid) -> ManifestResourcePage {
    ManifestResourcePage {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION,
        manifest_id,
        manifest_digest: "a".repeat(64),
        resource_count: 2,
        start_ordinal: 0,
        start_byte_offset: 0,
        next_ordinal: 0,
        next_byte_offset: 0,
        complete: false,
        delivered_bytes: 0,
        resources: Vec::new(),
        fragment: None,
        next_cursor: None,
    }
}

#[test]
fn packing_complete_resources_and_stable_cursor() {
    let (mut pin, verified) = fixture();
    let resource = assemble(&pin, verified, None).unwrap();
    let bytes = serde_json::to_vec(&resource).unwrap();
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = sha256(&bytes);
    let (tenant, workspace, principal, manifest) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let mut page = test_page(manifest);
    let mut replay = page.clone();
    assert_eq!(
        pack_resource(
            &mut page,
            pin.clone(),
            resource.clone(),
            &bytes,
            0,
            0,
            2,
            20_000,
            tenant,
            workspace,
            principal
        ),
        Ok(PackOutcome::Complete)
    );
    assert_eq!(
        pack_resource(
            &mut replay,
            pin.clone(),
            resource.clone(),
            &bytes,
            0,
            0,
            2,
            20_000,
            tenant,
            workspace,
            principal
        ),
        Ok(PackOutcome::Complete)
    );
    assert_eq!(page, replay);
    let mut second_pin = pin.clone();
    second_pin.ordinal = 1;
    assert_eq!(
        pack_resource(
            &mut page,
            second_pin.clone(),
            resource,
            &bytes,
            1,
            0,
            2,
            20_000,
            tenant,
            workspace,
            principal
        ),
        Ok(PackOutcome::Complete)
    );
    assert_eq!(page.resources.len(), 2);
    assert_eq!(page.resources[0].pin, pin);
    assert_eq!(page.resources[1].pin, second_pin);
    assert_eq!(page.resources[0].pin_digest, pin_digest(&pin).unwrap());
    assert_eq!(page.next_cursor, None);
    assert!(serialized_size(&page).unwrap() <= 20_000);
}

#[test]
fn oversize_utf8_resource_fragments_reassemble_exact_json_bytes() {
    let (mut pin, mut verified) = fixture();
    verified
        .input
        .planned
        .document
        .as_mut()
        .unwrap()
        .canonical_text = "🍃漢字".repeat(20_000);
    let resource = assemble(&pin, verified, None).unwrap();
    let bytes = serde_json::to_vec(&resource).unwrap();
    let digest = sha256(&bytes);
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = digest.clone();
    let (tenant, workspace, principal, manifest) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let mut assembled = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let mut page = test_page(manifest);
        page.resource_count = 1;
        assert_eq!(
            pack_resource(
                &mut page,
                pin.clone(),
                resource.clone(),
                &bytes,
                0,
                offset,
                1,
                4096,
                tenant,
                workspace,
                principal
            ),
            Ok(PackOutcome::Fragment)
        );
        assert!(serialized_size(&page).unwrap() <= 4096);
        let fragment = page.fragment.unwrap();
        assert_eq!(fragment.pin, pin);
        assert_eq!(fragment.pin_digest, pin_digest(&pin).unwrap());
        assert_eq!(fragment.byte_offset, offset);
        assert_eq!(fragment.total_bytes, bytes.len());
        assert_eq!(fragment.sha256, digest);
        let chunk = URL_SAFE_NO_PAD.decode(fragment.data).unwrap();
        assert!(!chunk.is_empty());
        assembled.extend_from_slice(&chunk);
        offset += chunk.len();
    }
    assert_eq!(assembled, bytes);
}

#[test]
fn pin_bearing_fragment_stays_within_one_megabyte_envelope() {
    let (mut pin, verified) = fixture();
    let mut resource = assemble(&pin, verified, None).unwrap();
    // Exercise framing independently of the document ingress size guard.
    resource.canonical_text = "large".repeat(300_000);
    let bytes = serde_json::to_vec(&resource).unwrap();
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = sha256(&bytes);
    let (tenant, workspace, principal, manifest) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let mut page = test_page(manifest);
    page.resource_count = 1;
    assert_eq!(
        pack_resource(
            &mut page,
            pin.clone(),
            resource,
            &bytes,
            0,
            0,
            1,
            MAX_PAGE_BYTES,
            tenant,
            workspace,
            principal
        ),
        Ok(PackOutcome::Fragment)
    );
    assert_eq!(page.fragment.as_ref().unwrap().pin, pin);
    assert_eq!(
        page.fragment.as_ref().unwrap().pin_digest,
        pin_digest(&pin).unwrap()
    );
    assert!(serialized_size(&page).unwrap() <= MAX_PAGE_BYTES);
    assert!(page.next_cursor.is_some());
    assert!(!page.complete);
}

#[test]
fn tampered_cursor_and_tiny_budget_are_rejected() {
    let (tenant, workspace, principal, manifest) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let original = PageCursor::new(
        tenant,
        workspace,
        principal,
        manifest,
        &"a".repeat(64),
        0,
        4,
    )
    .encode();
    assert!(
        PageCursor::decode(
            &original,
            tenant,
            workspace,
            principal,
            manifest,
            &"a".repeat(64)
        )
        .is_ok()
    );
    let mut chars: Vec<char> = original.chars().collect();
    chars[20] = if chars[20] == 'A' { 'B' } else { 'A' };
    let tampered: String = chars.into_iter().collect();
    assert_eq!(
        PageCursor::decode(
            &tampered,
            tenant,
            workspace,
            principal,
            manifest,
            &"a".repeat(64)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    assert_eq!(
        PageCursor::decode(
            &original,
            tenant,
            workspace,
            Uuid::new_v4(),
            manifest,
            &"a".repeat(64)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    assert_eq!(
        PageCursor::decode(
            &original,
            tenant,
            Uuid::new_v4(),
            principal,
            manifest,
            &"a".repeat(64)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    assert_eq!(
        PageCursor::decode(
            &original,
            tenant,
            workspace,
            principal,
            Uuid::new_v4(),
            &"a".repeat(64)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    assert_eq!(
        PageCursor::decode(
            &original,
            tenant,
            workspace,
            principal,
            manifest,
            &"b".repeat(64)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    let (mut pin, verified) = fixture();
    let resource = assemble(&pin, verified, None).unwrap();
    let bytes = serde_json::to_vec(&resource).unwrap();
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = sha256(&bytes);
    let mut page = test_page(manifest);
    assert_eq!(
        pack_resource(
            &mut page,
            pin.clone(),
            resource,
            &bytes,
            0,
            0,
            1,
            100,
            tenant,
            workspace,
            principal
        ),
        Err(Error::RequestTooLarge)
    );
    assert!(!page_has_progress(&page));
}

#[test]
fn cursor_is_a_position_hint_with_canonical_bounds() {
    let (tenant, workspace, principal, manifest) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let digest = "a".repeat(64);
    // A caller can deliberately skip to a later row and recompute the public checksum.
    // Authorization and the manifest commitment must therefore run on that read.
    let later = PageCursor::new(tenant, workspace, principal, manifest, &digest, 7, 0);
    let decoded = PageCursor::decode(
        &later.encode(),
        tenant,
        workspace,
        principal,
        manifest,
        &digest,
    )
    .unwrap();
    assert_eq!(validate_position(8, &decoded, None), Ok(()));
    assert_eq!(
        validate_position(7, &decoded, None),
        Err(Error::InvalidArguments)
    );
    let overflow = PageCursor::new(tenant, workspace, principal, manifest, &digest, i64::MAX, 0);
    assert_eq!(
        validate_position(8, &overflow, None),
        Err(Error::InvalidArguments)
    );
    let past_resource = PageCursor::new(tenant, workspace, principal, manifest, &digest, 7, 100);
    assert_eq!(
        validate_position(8, &past_resource, Some(100)),
        Err(Error::InvalidArguments)
    );
    assert_eq!(validate_position(8, &past_resource, Some(101)), Ok(()));
    let empty = PageCursor::new(tenant, workspace, principal, manifest, &digest, 0, 0);
    assert_eq!(validate_position(0, &empty, None), Ok(()));
    let mut empty_page = test_page(manifest);
    empty_page.resource_count = 0;
    empty_page.complete = true;
    empty_page.delivered_bytes = serialized_size(&empty_page).unwrap();
    while empty_page.delivered_bytes != serialized_size(&empty_page).unwrap() {
        empty_page.delivered_bytes = serialized_size(&empty_page).unwrap();
    }
    assert!(empty_page.resources.is_empty());
    assert!(empty_page.next_cursor.is_none());
    assert_eq!(
        empty_page.delivered_bytes,
        serialized_size(&empty_page).unwrap()
    );
    assert!(empty_page.delivered_bytes <= MAX_PAGE_BYTES);
}

#[tokio::test]
async fn page_reader_requires_repeatable_snapshot() {
    let Ok(url) = std::env::var("TECT_TEST_ADMIN_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let mut read_committed = pool.begin().await.unwrap();
    assert_eq!(
        require_consistent_snapshot(&mut read_committed).await,
        Err(Error::InternalInvariant)
    );
    read_committed.rollback().await.unwrap();

    for level in ["REPEATABLE READ", "SERIALIZABLE"] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(&format!("SET TRANSACTION ISOLATION LEVEL {level}"))
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(require_consistent_snapshot(&mut tx).await, Ok(()));
        tx.rollback().await.unwrap();
    }
    let mut owned = begin_page_snapshot(&pool).await.unwrap();
    assert_eq!(require_consistent_snapshot(&mut owned).await, Ok(()));
    let read_only: String = sqlx::query_scalar("SELECT current_setting('transaction_read_only')")
        .fetch_one(&mut *owned)
        .await
        .unwrap();
    assert_eq!(read_only, "on");
    owned.rollback().await.unwrap();
}

mod resource;
