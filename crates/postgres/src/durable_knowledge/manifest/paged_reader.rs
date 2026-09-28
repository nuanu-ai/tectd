//! Internal reader for an exact pinned row of a compact pipeline manifest.
//! Callers keep one repeatable-read transaction around authorization and page assembly.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::Row;

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

/// Public checksum is for accidental corruption only. Every use reauthorizes
/// the manifest and all children; callers may construct any valid position.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    version: u8,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: String,
    ordinal: i64,
    byte_offset: usize,
    checksum: String,
}

impl PageCursor {
    fn new(
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        manifest_id: Uuid,
        manifest_digest: &str,
        ordinal: i64,
        byte_offset: usize,
    ) -> Self {
        let mut value = Self {
            version: 1,
            tenant,
            workspace,
            principal,
            manifest_id,
            manifest_digest: manifest_digest.into(),
            ordinal,
            byte_offset,
            checksum: String::new(),
        };
        value.checksum = value.expected_checksum();
        value
    }

    fn expected_checksum(&self) -> String {
        sha256(
            &serde_json::to_vec(&(
                "dk-2-paged-cursor-v1",
                self.version,
                self.tenant,
                self.workspace,
                self.principal,
                self.manifest_id,
                &self.manifest_digest,
                self.ordinal,
                self.byte_offset,
            ))
            .expect("cursor tuple serializes"),
        )
    }

    fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("cursor serializes"))
    }

    fn decode(
        encoded: &str,
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        manifest_id: Uuid,
        manifest_digest: &str,
    ) -> Result<Self> {
        if encoded.len() > 2048 {
            return Err(Error::InvalidArguments);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::InvalidArguments)?;
        let value: Self = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidArguments)?;
        if value.version != 1
            || value.tenant != tenant
            || value.workspace != workspace
            || value.principal != principal
            || value.manifest_id != manifest_id
            || value.manifest_digest != manifest_digest
            || value.ordinal < 0
            || value.checksum != value.expected_checksum()
            || value.encode() != encoded
        {
            return Err(Error::InvalidArguments);
        }
        Ok(value)
    }
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

#[derive(Debug, PartialEq, Eq)]
enum PackOutcome {
    Complete,
    Fragment,
    Stop,
}

#[allow(clippy::too_many_arguments)]
fn pack_resource(
    page: &mut ManifestResourcePage,
    pin: PagedPipelineKnowledgeResourcePin,
    resource: PipelineKnowledgeResource,
    bytes: &[u8],
    ordinal: i64,
    offset: usize,
    count: i64,
    budget: usize,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
) -> Result<PackOutcome> {
    if pin.ordinal != ordinal
        || pin.resource_bytes != bytes.len() as i64
        || offset >= bytes.len() && offset != 0
    {
        return Err(Error::InvalidArguments);
    }
    let digest = pin_digest(&pin)?;
    let manifest_id = page.manifest_id;
    let manifest_digest = page.manifest_digest.clone();
    let next = |byte_offset: usize| {
        let (next_ordinal, next_offset) = if byte_offset == bytes.len() {
            (ordinal + 1, 0)
        } else {
            (ordinal, byte_offset)
        };
        (next_offset != 0 || next_ordinal < count).then(|| {
            PageCursor::new(
                tenant,
                workspace,
                principal,
                manifest_id,
                &manifest_digest,
                next_ordinal,
                next_offset,
            )
            .encode()
        })
    };
    if offset == 0 {
        let mut candidate = page.clone();
        candidate.resources.push(PinnedPageResource {
            pin: pin.clone(),
            pin_digest: digest.clone(),
            resource,
        });
        candidate.next_cursor = next(bytes.len());
        candidate.next_ordinal = ordinal + 1;
        candidate.next_byte_offset = 0;
        candidate.complete = candidate.next_cursor.is_none();
        candidate.delivered_bytes = MAX_PAGE_BYTES;
        if serialized_size(&candidate)? <= budget {
            *page = candidate;
            return Ok(PackOutcome::Complete);
        }
        if page_has_progress(page) {
            return Ok(PackOutcome::Stop);
        }
    }
    let mut low = 0usize;
    let mut high = bytes.len() - offset;
    while low < high {
        let n = low + (high - low).div_ceil(2);
        let next_offset = offset + n;
        let mut candidate = page.clone();
        candidate.fragment = Some(ResourceByteFragment {
            pin: pin.clone(),
            pin_digest: digest.clone(),
            byte_offset: offset,
            total_bytes: bytes.len(),
            sha256: pin.resource_digest.clone(),
            encoding: "base64_json_bytes",
            data: URL_SAFE_NO_PAD.encode(&bytes[offset..next_offset]),
        });
        candidate.next_cursor = next(next_offset);
        candidate.next_ordinal = if next_offset == bytes.len() {
            ordinal + 1
        } else {
            ordinal
        };
        candidate.next_byte_offset = if next_offset == bytes.len() {
            0
        } else {
            next_offset
        };
        candidate.complete = candidate.next_cursor.is_none();
        candidate.delivered_bytes = MAX_PAGE_BYTES;
        if serialized_size(&candidate)? <= budget {
            low = n;
        } else {
            high = n - 1;
        }
    }
    if low == 0 {
        return Err(Error::RequestTooLarge);
    }
    let next_offset = offset + low;
    let resource_sha256 = pin.resource_digest.clone();
    page.fragment = Some(ResourceByteFragment {
        pin,
        pin_digest: digest,
        byte_offset: offset,
        total_bytes: bytes.len(),
        sha256: resource_sha256,
        encoding: "base64_json_bytes",
        data: URL_SAFE_NO_PAD.encode(&bytes[offset..next_offset]),
    });
    page.next_cursor = next(next_offset);
    page.next_ordinal = if next_offset == bytes.len() {
        ordinal + 1
    } else {
        ordinal
    };
    page.next_byte_offset = if next_offset == bytes.len() {
        0
    } else {
        next_offset
    };
    page.complete = page.next_cursor.is_none();
    Ok(PackOutcome::Fragment)
}

/// The serialized bytes are the committed resource-json-v1 representation.
/// No current binding, revision, or validation event is used for content.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn read_pinned_resource(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
    ordinal: i64,
) -> Result<(
    PagedPipelineKnowledgeResourcePin,
    PipelineKnowledgeResource,
    Vec<u8>,
)> {
    require_consistent_snapshot(tx).await?;
    if ordinal < 0
        || delivery::authorize_manifest(tx, tenant, workspace, manifest_id, principal)
            .await?
            .as_deref()
            != Some(PAGED_KNOWLEDGE_CONTRACT_VERSION)
    {
        return Err(Error::NotFound);
    }
    let (manifest, pins) =
        load_manifest_commitment(tx, tenant, workspace, manifest_id, manifest_digest).await?;
    if ordinal >= manifest.resource_count {
        return Err(Error::InvalidArguments);
    }
    let pin = pins
        .into_iter()
        .nth(ordinal as usize)
        .ok_or(Error::InternalInvariant)?;
    pin.validate()?;
    let binding_matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND unit_id=$4)")
        .bind(tenant).bind(workspace).bind(pin.binding_id).bind(pin.unit_id)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !binding_matches
        || pin.binding_pin.binding_iri != format!("urn:tect:dk:binding:{}", pin.binding_id)
    {
        return Err(Error::InternalInvariant);
    }
    if pin.entry_kind == PagedKnowledgeEntryKind::Dk1Legacy {
        let revision =
            context::load_revision(tx, tenant, workspace, pin.unit_id, Some(pin.revision), true)
                .await?
                .ok_or(Error::InternalInvariant)?;
        let (resource, bytes) = reconstruct_legacy(&pin, revision)?;
        return Ok((pin, resource, bytes));
    }
    let event = pin.publication_event_id.ok_or(Error::InternalInvariant)?;
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx,
        tenant,
        workspace,
        pin.unit_id,
        pin.revision,
        event,
        true,
    )
    .await?;
    if verified.rdf_digest != pin.rdf_digest {
        return Err(Error::InternalInvariant);
    }
    let latest_validation = if let Some(id) = pin.validation_event_id {
        let sequence: Option<i64> = sqlx::query_scalar("SELECT (SELECT count(*) FROM knowledge_validation_events x WHERE x.tenant_id=v.tenant_id AND x.workspace_id=v.workspace_id AND x.unit_id=v.unit_id AND x.unit_revision=v.unit_revision AND NOT x.payload_erased AND (x.created_at,x.id)<=(v.created_at,v.id)) FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.id=$3 AND v.unit_id=$4 AND v.unit_revision=$5 AND NOT v.payload_erased")
            .bind(tenant).bind(workspace).bind(id).bind(pin.unit_id).bind(pin.revision)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let validation = generic::resource::validation_at(
            tx,
            tenant,
            workspace,
            pin.unit_id,
            pin.revision,
            id,
            sequence.ok_or(Error::KnowledgePayloadErased)?,
        )
        .await?;
        if pin.validation_event_digest.as_deref() != Some(validation.event_digest.as_str()) {
            return Err(Error::InternalInvariant);
        }
        Some(validation)
    } else {
        None
    };
    let (resource, bytes) = reconstruct(&pin, verified, latest_validation)?;
    Ok((pin, resource, bytes))
}

pub(super) async fn load_manifest_commitment(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
) -> Result<(
    PagedPipelineKnowledgeManifest,
    Vec<PagedPipelineKnowledgeResourcePin>,
)> {
    let header = sqlx::query(
        "SELECT digest,semantic_digest,selected,unresolved_needs,resource_semantic_digest,workspace_generation,run_id,run_revision,phase_id,definition_version,definition_digest,method_requirements,resource_inquiry,resource_projection_policy,resource_unresolved_needs,freshness_warnings,resource_count,total_resource_bytes,resource_digest_algorithm FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND contract_version='dk-2-paged' AND NOT payload_erased",
    ).bind(tenant).bind(workspace).bind(manifest_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::KnowledgePayloadErased)?;
    let manifest = PagedPipelineKnowledgeManifest {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION.into(),
        id: manifest_id,
        digest: header.try_get("digest").map_err(storage_error)?,
        semantic_digest: header
            .try_get("resource_semantic_digest")
            .map_err(storage_error)?,
        workspace_generation: header
            .try_get("workspace_generation")
            .map_err(storage_error)?,
        run_id: header.try_get("run_id").map_err(storage_error)?,
        run_revision: header.try_get("run_revision").map_err(storage_error)?,
        phase_id: header.try_get("phase_id").map_err(storage_error)?,
        definition_version: header
            .try_get("definition_version")
            .map_err(storage_error)?,
        definition_digest: header.try_get("definition_digest").map_err(storage_error)?,
        method_requirements: decode(
            header
                .try_get("method_requirements")
                .map_err(storage_error)?,
        )?,
        inquiry: header
            .try_get::<Option<serde_json::Value>, _>("resource_inquiry")
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        projection_policy: header
            .try_get::<Option<String>, _>("resource_projection_policy")
            .map_err(storage_error)?
            .map(|v| decode(serde_json::Value::String(v)))
            .transpose()?,
        unresolved_needs: decode(
            header
                .try_get("resource_unresolved_needs")
                .map_err(storage_error)?,
        )?,
        freshness_warnings: decode(
            header
                .try_get("freshness_warnings")
                .map_err(storage_error)?,
        )?,
        resource_count: header.try_get("resource_count").map_err(storage_error)?,
        total_resource_bytes: header
            .try_get("total_resource_bytes")
            .map_err(storage_error)?,
        resource_digest_algorithm: header
            .try_get("resource_digest_algorithm")
            .map_err(storage_error)?,
        page_route: "slice.pipeline.knowledge_page".into(),
    };
    if manifest.digest != manifest_digest {
        return Err(Error::InternalInvariant);
    }
    // Verify the entire ordered commitment before releasing one row. A deleted or
    // reordered child must never turn a page into an apparently complete response.
    let rows = sqlx::query("SELECT ordinal,entry_kind,unit_id,revision,publication_event_id,rdf_digest,binding_id,binding_pin,lifecycle,access_scope,validation_event_id,validation_event_digest,projection,resource_digest,resource_bytes FROM pipeline_knowledge_manifest_resources WHERE tenant_id=$1 AND workspace_id=$2 AND manifest_id=$3 ORDER BY ordinal")
        .bind(tenant).bind(workspace).bind(manifest_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut pins = Vec::with_capacity(rows.len());
    for row in &rows {
        let kind: String = row.try_get("entry_kind").map_err(storage_error)?;
        let pin = PagedPipelineKnowledgeResourcePin {
            ordinal: row.try_get("ordinal").map_err(storage_error)?,
            entry_kind: decode(serde_json::Value::String(kind))?,
            unit_id: row.try_get("unit_id").map_err(storage_error)?,
            revision: row.try_get("revision").map_err(storage_error)?,
            publication_event_id: row.try_get("publication_event_id").map_err(storage_error)?,
            rdf_digest: row.try_get("rdf_digest").map_err(storage_error)?,
            binding_id: row.try_get("binding_id").map_err(storage_error)?,
            binding_pin: decode(row.try_get("binding_pin").map_err(storage_error)?)?,
            lifecycle: decode(serde_json::Value::String(
                row.try_get("lifecycle").map_err(storage_error)?,
            ))?,
            access_scope: decode(serde_json::Value::String(
                row.try_get("access_scope").map_err(storage_error)?,
            ))?,
            validation_event_id: row.try_get("validation_event_id").map_err(storage_error)?,
            validation_event_digest: row
                .try_get("validation_event_digest")
                .map_err(storage_error)?,
            projection: decode(row.try_get("projection").map_err(storage_error)?)?,
            resource_digest: row.try_get("resource_digest").map_err(storage_error)?,
            resource_bytes: row.try_get("resource_bytes").map_err(storage_error)?,
        };
        pins.push(pin);
    }
    let legacy_selected: Vec<PipelineKnowledgeItem> =
        decode(header.try_get("selected").map_err(storage_error)?)?;
    let legacy_unresolved: Vec<String> =
        decode(header.try_get("unresolved_needs").map_err(storage_error)?)?;
    let legacy_semantic: String = header.try_get("semantic_digest").map_err(storage_error)?;
    if legacy::semantic(&legacy_selected, &legacy_unresolved)? != legacy_semantic
        || paged_manifest_digest(
            tenant,
            workspace,
            &manifest,
            &pins,
            &legacy_selected,
            &legacy_unresolved,
        )? != manifest.digest
    {
        return Err(Error::InternalInvariant);
    }
    Ok((manifest, pins))
}

async fn verify_empty_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
) -> Result<()> {
    require_consistent_snapshot(tx).await?;
    if delivery::authorize_manifest(tx, tenant, workspace, manifest_id, principal)
        .await?
        .as_deref()
        != Some(PAGED_KNOWLEDGE_CONTRACT_VERSION)
    {
        return Err(Error::NotFound);
    }
    let (manifest, pins) =
        load_manifest_commitment(tx, tenant, workspace, manifest_id, manifest_digest).await?;
    if manifest.resource_count != 0 || !pins.is_empty() {
        return Err(Error::InternalInvariant);
    }
    Ok(())
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

fn reconstruct(
    pin: &PagedPipelineKnowledgeResourcePin,
    verified: crate::knowledge_lifecycle::VerifiedPublicationEvent,
    latest_validation: Option<PipelineKnowledgeValidationPin>,
) -> Result<(PipelineKnowledgeResource, Vec<u8>)> {
    let resource = assemble(pin, verified, latest_validation)?;
    let bytes = checked_resource_bytes(pin, &resource)?;
    Ok((resource, bytes))
}

fn reconstruct_legacy(
    pin: &PagedPipelineKnowledgeResourcePin,
    revision: KnowledgeUnitRevision,
) -> Result<(PipelineKnowledgeResource, Vec<u8>)> {
    if pin.entry_kind != PagedKnowledgeEntryKind::Dk1Legacy
        || pin.publication_event_id.is_some()
        || pin.validation_event_id.is_some()
        || pin.validation_event_digest.is_some()
        || pin.projection.policy != PipelineKnowledgeProjectionPolicy::FullResources
        || !pin.projection.inquiry_briefs.is_empty()
        || revision.unit_id != pin.unit_id
        || revision.revision != pin.revision
        || revision.rdf_digest != pin.rdf_digest
    {
        return Err(Error::InternalInvariant);
    }
    let resource = generic::resource::legacy_from_revision(
        pin.lifecycle,
        revision,
        pin.access_scope,
        pin.binding_pin.clone(),
    );
    let bytes = checked_resource_bytes(pin, &resource)?;
    Ok((resource, bytes))
}

fn checked_resource_bytes(
    pin: &PagedPipelineKnowledgeResourcePin,
    resource: &PipelineKnowledgeResource,
) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&resource).map_err(storage_error)?;
    if bytes.len() as i64 != pin.resource_bytes || sha256(&bytes) != pin.resource_digest {
        return Err(Error::InternalInvariant);
    }
    Ok(bytes)
}

pub(super) fn assemble(
    pin: &PagedPipelineKnowledgeResourcePin,
    verified: crate::knowledge_lifecycle::VerifiedPublicationEvent,
    latest_validation: Option<PipelineKnowledgeValidationPin>,
) -> Result<PipelineKnowledgeResource> {
    if verified.rdf_digest != pin.rdf_digest
        || verified.input.planned.unit_id != pin.unit_id
        || verified.input.content_revision != pin.revision
        || Some(verified.input.event_id) != pin.publication_event_id
        || latest_validation.as_ref().map(|v| v.event_id) != pin.validation_event_id
        || latest_validation.as_ref().map(|v| v.event_digest.as_str())
            != pin.validation_event_digest.as_deref()
    {
        return Err(Error::InternalInvariant);
    }
    let document = verified
        .input
        .planned
        .document
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    let rdf = crate::knowledge_lifecycle::rdf::build(&verified.input)?;
    let (canonical_text, target_iris, conditions, exceptions, sections, inquiry_briefs) =
        if pin.projection.policy == PipelineKnowledgeProjectionPolicy::FullResources {
            (
                document.canonical_text.clone(),
                document.target_iris.clone(),
                document.conditions.clone(),
                document.exceptions.clone(),
                document.sections.clone(),
                None,
            )
        } else {
            if pin.projection.inquiry_briefs.is_empty() {
                return Err(Error::InternalInvariant);
            }
            let mut values = Vec::new();
            for brief_pin in &pin.projection.inquiry_briefs {
                let brief = document
                    .planning_briefs
                    .iter()
                    .find(|v| v.local_id == brief_pin.id)
                    .ok_or(Error::InternalInvariant)?;
                if digest(brief)? != brief_pin.digest {
                    return Err(Error::InternalInvariant);
                }
                values.push(brief.clone());
            }
            (
                inquiry::projected_text(&values),
                inquiry::projected_targets(&values),
                inquiry::projected_conditions(&values),
                inquiry::projected_exceptions(&values),
                KnowledgeProfileSections::default(),
                Some(values),
            )
        };
    let resource = PipelineKnowledgeResource {
        unit_id: pin.unit_id,
        revision: pin.revision,
        lifecycle: pin.lifecycle,
        access_scope: pin.access_scope,
        rdf_digest: pin.rdf_digest.clone(),
        unit_iri: rdf.refs.unit,
        revision_iri: rdf.refs.revision,
        title: document.title.clone(),
        canonical_text,
        knowledge_kind: document.knowledge_kind,
        epistemic_state: document.epistemic_state,
        target_iris,
        profiles: document.profiles.clone(),
        conditions,
        exceptions,
        sections,
        inquiry_briefs,
        source_pins: verified
            .input
            .resolved_sources
            .into_iter()
            .map(|v| PipelineKnowledgeSourcePin {
                source_iri: v.pin.source_iri,
                digest: v.pin.digest,
                evidence_kind: v.pin.evidence_kind,
                observed_at: v.pin.observed_at,
                evidence_scope: v.pin.evidence_scope,
                title: v.title,
                uri: v.uri,
            })
            .collect(),
        latest_validation,
        binding: pin.binding_pin.clone(),
        why_included: match pin.binding_pin.target {
            KnowledgeBindingTarget::Workspace => "workspace_binding",
            KnowledgeBindingTarget::Program { .. } => "program_binding",
            KnowledgeBindingTarget::Scope { .. } => "scope_binding",
            KnowledgeBindingTarget::Slice { .. } => "slice_binding",
            KnowledgeBindingTarget::SlicePhase { .. } => "slice_phase_binding",
        }
        .into(),
    };
    Ok(resource)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let overflow =
            PageCursor::new(tenant, workspace, principal, manifest, &digest, i64::MAX, 0);
        assert_eq!(
            validate_position(8, &overflow, None),
            Err(Error::InvalidArguments)
        );
        let past_resource =
            PageCursor::new(tenant, workspace, principal, manifest, &digest, 7, 100);
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
        let read_only: String =
            sqlx::query_scalar("SELECT current_setting('transaction_read_only')")
                .fetch_one(&mut *owned)
                .await
                .unwrap();
        assert_eq!(read_only, "on");
        owned.rollback().await.unwrap();
    }

    fn fixture() -> (
        PagedPipelineKnowledgeResourcePin,
        crate::knowledge_lifecycle::VerifiedPublicationEvent,
    ) {
        let document: serde_json::Value = serde_json::from_str(include_str!(
            "../../knowledge_lifecycle/rdf/fixtures/general-constraint.json"
        ))
        .unwrap();
        let tenant = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let unit = Uuid::new_v4();
        let event = Uuid::new_v4();
        let binding = Uuid::new_v4();
        let input = decode(serde_json::json!({
            "tenant": tenant, "workspace": workspace, "change_id": Uuid::new_v4(),
            "event_id": event, "content_revision": 1,
            "planned": {"operation_id":Uuid::new_v4(),"unit_id":unit,"client_label":"fixture",
                "operation":"create","document":document["document"],"replacement_bindings":[],
                "reason":"fixture","authority_basis":"fixture","dependency_operation_ids":[]},
            "principal_id":Uuid::new_v4(),"session_id":Uuid::new_v4(),
            "resolved_sources":[{"pin":{"source_index":0,"digest":"c".repeat(64),
                "evidence_kind":"declaration","observed_at":null,
                "evidence_scope":"workspace","source_iri":"urn:tect:dk:source:fixture"},
                "title":"Pinned source","uri":"urn:test:source","text":"source bytes"}],
            "successor_unit":null
        }))
        .unwrap();
        let verified = crate::knowledge_lifecycle::VerifiedPublicationEvent {
            input,
            rdf_digest: "a".repeat(64),
        };
        let pin = decode(serde_json::json!({
            "ordinal":0,"entry_kind":"dk2_event","unit_id":unit,"revision":1,
            "publication_event_id":event,"rdf_digest":"a".repeat(64),"binding_id":binding,
            "binding_pin":{"binding_iri":format!("urn:tect:dk:binding:{binding}"),
                "target":{"kind":"workspace"},"purpose":"required",
                "version_resolution":{"kind":"current_accepted"}},
            "lifecycle":"active","access_scope":"workspace_members",
            "validation_event_id":null,"validation_event_digest":null,
            "projection":{"policy":"full_resources","inquiry_briefs":[]},
            "resource_digest":"b".repeat(64),"resource_bytes":1
        }))
        .unwrap();
        (pin, verified)
    }

    #[test]
    fn exact_immutable_payload_and_commitment() {
        let (mut pin, verified) = fixture();
        let resource = assemble(&pin, verified.clone(), None).unwrap();
        assert_eq!(
            resource.canonical_text,
            verified
                .input
                .planned
                .document
                .as_ref()
                .unwrap()
                .canonical_text
        );
        assert_eq!(resource.binding, pin.binding_pin);
        let bytes = serde_json::to_vec(&resource).unwrap();
        pin.resource_bytes = bytes.len() as i64;
        pin.resource_digest = sha256(&bytes);
        let (got, serialized) = reconstruct(&pin, verified.clone(), None).unwrap();
        assert_eq!(got, resource);
        assert_eq!(serialized, bytes);
        let mut bad = pin.clone();
        bad.resource_digest = "0".repeat(64);
        assert!(matches!(
            reconstruct(&bad, verified.clone(), None),
            Err(Error::InternalInvariant)
        ));
        let mut newer = verified.clone();
        newer.input.content_revision = 2;
        assert!(matches!(
            reconstruct(&pin, newer, None),
            Err(Error::InternalInvariant)
        ));
        let mut another_event = verified;
        another_event.input.event_id = Uuid::new_v4();
        assert!(matches!(
            reconstruct(&pin, another_event, None),
            Err(Error::InternalInvariant)
        ));
    }

    #[test]
    fn dk1_pin_reconstructs_exact_generic_resource_and_fragments() {
        let (mut pin, _) = fixture();
        pin.entry_kind = PagedKnowledgeEntryKind::Dk1Legacy;
        pin.publication_event_id = None;
        let unit = pin.unit_id;
        let revision = KnowledgeUnitRevision {
            unit_id: unit,
            revision: 1,
            active: true,
            constraint: KnowledgeConstraintDraft {
                title: "Legacy title".into(),
                statement: "Pinned legacy statement".repeat(128),
                modality: KnowledgeModality::Must,
                action: "retain".into(),
                target_iri: "urn:target:legacy".into(),
                conditions: vec!["condition".into()],
                exceptions: vec![],
                source: KnowledgeSourceSnapshot {
                    title: "Source".into(),
                    uri: "urn:source:legacy".into(),
                    text: "source text".into(),
                },
                binding: KnowledgeBinding::Workspace,
                purpose: KnowledgePurpose::ExecutionConstraint,
                version_resolution: KnowledgeVersionResolution::CurrentAccepted,
            },
            source_sha256: sha256(b"source text"),
            rdf_digest: pin.rdf_digest.clone(),
            rdf_digest_method: "rdfc-1.0-sha256".into(),
            rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
            publication_event_id: Uuid::new_v4(),
            unit_iri: format!("urn:tect:dk:unit:{unit}"),
            revision_iri: format!("urn:tect:dk:revision:{unit}:1"),
            source_iri: "urn:tect:dk:source:legacy".into(),
            publication_event_iri: "urn:tect:dk:event:legacy".into(),
            publication_operation: KnowledgeOperation::Create,
            publication_reason: "test".into(),
            publication_authority_basis: "test".into(),
            publication_actor_principal_id: Uuid::new_v4(),
            publication_actor_session_id: Uuid::new_v4(),
            binding_provenance: None,
        };
        let resource = generic::resource::legacy_from_revision(
            pin.lifecycle,
            revision.clone(),
            pin.access_scope,
            pin.binding_pin.clone(),
        );
        assert_eq!(
            resource.canonical_text,
            "Pinned legacy statement".repeat(128)
        );
        assert_eq!(resource.revision, pin.revision);
        let bytes = serde_json::to_vec(&resource).unwrap();
        pin.resource_bytes = bytes.len() as i64;
        pin.resource_digest = sha256(&bytes);
        let (read_resource, read_bytes) = reconstruct_legacy(&pin, revision.clone()).unwrap();
        assert_eq!(read_resource, resource);
        assert_eq!(read_bytes, bytes);
        let mut page = test_page(Uuid::new_v4());
        let tenant = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let principal = Uuid::new_v4();
        assert_eq!(
            pack_resource(
                &mut page,
                pin.clone(),
                resource.clone(),
                &bytes,
                0,
                0,
                2,
                2048,
                tenant,
                workspace,
                principal,
            )
            .unwrap(),
            PackOutcome::Fragment
        );
        assert_eq!(page.fragment.as_ref().unwrap().sha256, pin.resource_digest);
        assert!(page.next_byte_offset > 0);
        let mut bad = pin.clone();
        bad.resource_digest = "0".repeat(64);
        assert!(matches!(
            reconstruct_legacy(&bad, revision.clone()),
            Err(Error::InternalInvariant)
        ));
        bad = pin.clone();
        bad.revision += 1;
        assert!(matches!(
            reconstruct_legacy(&bad, revision.clone()),
            Err(Error::InternalInvariant)
        ));
        bad = pin;
        bad.rdf_digest = "0".repeat(64);
        assert!(matches!(
            reconstruct_legacy(&bad, revision),
            Err(Error::InternalInvariant)
        ));
    }
}
