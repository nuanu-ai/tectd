//! Internal reader for an exact pinned row of a compact pipeline manifest.
//! Callers keep one repeatable-read transaction around authorization and page assembly.
use super::*;
use serde::Serialize;

mod cursor;
mod packing;
mod pinned;
#[cfg(test)]
mod tests;

use cursor::PageCursor;
use packing::{PackOutcome, pack_resource};
pub(crate) use pinned::read_pinned_resource;
use pinned::verify_empty_manifest;
pub(super) use pinned::{assemble, load_manifest_commitment};
#[cfg(test)]
use pinned::{reconstruct, reconstruct_legacy};

/// Deliberately smaller than the native host's 8 MiB frame. This bounds the
/// final JSON response, rather than just the sum of resource payloads.
const MAX_PAGE_BYTES: usize = 1_048_576;
const MIN_PAGE_BYTES: usize = 768;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ManifestResourcePage {
    pub contract_version: &'static str,
    pub manifest_id: Uuid,
    pub manifest_digest: String,
    pub resource_count: i64,
    pub start_ordinal: i64,
    pub start_byte_offset: usize,
    pub next_ordinal: i64,
    pub next_byte_offset: usize,
    pub complete: bool,
    pub delivered_bytes: usize,
    pub resources: Vec<PinnedPageResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment: Option<ResourceByteFragment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PinnedPageResource {
    pub pin: PagedPipelineKnowledgeResourcePin,
    pub pin_digest: String,
    pub resource: PipelineKnowledgeResource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ResourceByteFragment {
    pub pin: PagedPipelineKnowledgeResourcePin,
    pub pin_digest: String,
    pub byte_offset: usize,
    pub total_bytes: usize,
    pub sha256: String,
    pub encoding: &'static str,
    pub data: String,
}

fn pin_digest(pin: &PagedPipelineKnowledgeResourcePin) -> Result<String> {
    Ok(sha256(&serde_json::to_vec(pin).map_err(storage_error)?))
}
/// Own the consistent snapshot for authorization and every resource assembled
/// into this page. A caller cannot accidentally authorize in another snapshot.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn read_manifest_resource_page(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
    cursor: Option<&str>,
    requested_bytes: usize,
) -> Result<ManifestResourcePage> {
    let budget = requested_bytes.min(MAX_PAGE_BYTES);
    if budget < MIN_PAGE_BYTES {
        return Err(Error::RequestTooLarge);
    }
    let position = match cursor {
        Some(value) => PageCursor::decode(
            value,
            tenant,
            workspace,
            principal,
            manifest_id,
            manifest_digest,
        )?,
        None => PageCursor::new(
            tenant,
            workspace,
            principal,
            manifest_id,
            manifest_digest,
            0,
            0,
        ),
    };
    require_consistent_snapshot(tx).await?;
    if delivery::authorize_manifest(tx, tenant, workspace, manifest_id, principal)
        .await?
        .as_deref()
        != Some(PAGED_KNOWLEDGE_CONTRACT_VERSION)
    {
        return Err(Error::NotFound);
    }
    let count: Option<i64> = sqlx::query_scalar("SELECT resource_count FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND contract_version='dk-2-paged' AND NOT payload_erased")
        .bind(tenant).bind(workspace).bind(manifest_id).bind(manifest_digest)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let count = count.ok_or(Error::KnowledgePayloadErased)?;
    validate_position(count, &position, None)?;
    // The pinned reader checks the whole manifest commitment and every pinned
    // child's current ACL/erasure before returning any payload.
    let mut page = ManifestResourcePage {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION,
        manifest_id,
        manifest_digest: manifest_digest.into(),
        resource_count: count,
        start_ordinal: position.ordinal,
        start_byte_offset: position.byte_offset,
        next_ordinal: position.ordinal,
        next_byte_offset: position.byte_offset,
        complete: count == 0,
        delivered_bytes: 0,
        resources: Vec::new(),
        fragment: None,
        next_cursor: None,
    };
    let mut ordinal = position.ordinal;
    let mut offset = position.byte_offset;
    while ordinal < count {
        let (pin, resource, bytes) = read_pinned_resource(
            tx,
            tenant,
            workspace,
            principal,
            manifest_id,
            manifest_digest,
            ordinal,
        )
        .await?;
        if ordinal == position.ordinal {
            validate_position(count, &position, Some(bytes.len()))?;
        }
        match pack_resource(
            &mut page, pin, resource, &bytes, ordinal, offset, count, budget, tenant, workspace,
            principal,
        )? {
            PackOutcome::Complete => {
                ordinal += 1;
                offset = 0;
                continue;
            }
            PackOutcome::Fragment | PackOutcome::Stop => break,
        }
    }
    if count == 0 {
        // Even an empty manifest is a committed snapshot, not a missing row.
        // Its digest and absence of child rows are checked by the same reader.
        verify_empty_manifest(
            tx,
            tenant,
            workspace,
            principal,
            manifest_id,
            manifest_digest,
        )
        .await?;
    }
    page.delivered_bytes = serialized_size(&page)?;
    while serialized_size(&page)? != page.delivered_bytes {
        page.delivered_bytes = serialized_size(&page)?;
    }
    if (count != 0 && !page_has_progress(&page)) || serialized_size(&page)? > budget {
        return Err(Error::InternalInvariant);
    }
    Ok(page)
}

fn page_has_progress(page: &ManifestResourcePage) -> bool {
    !page.resources.is_empty() || page.fragment.is_some()
}

fn validate_position(
    count: i64,
    position: &PageCursor,
    resource_bytes: Option<usize>,
) -> Result<()> {
    if count < 0
        || position.ordinal < 0
        || position.ordinal > count
        || (position.ordinal == count && (count != 0 || position.byte_offset != 0))
        || (position.ordinal < count
            && position.byte_offset > 0
            && resource_bytes.is_some_and(|length| position.byte_offset >= length))
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn serialized_size(page: &ManifestResourcePage) -> Result<usize> {
    Ok(serde_json::to_vec(page).map_err(storage_error)?.len())
}

async fn begin_page_snapshot(pool: &sqlx::PgPool) -> Result<Transaction<'_, Postgres>> {
    let mut tx = pool.begin().await.map_err(storage_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    require_consistent_snapshot(&mut tx).await?;
    Ok(tx)
}

async fn require_consistent_snapshot(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let isolation: String = sqlx::query_scalar("SELECT current_setting('transaction_isolation')")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !matches!(isolation.as_str(), "repeatable read" | "serializable") {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}
