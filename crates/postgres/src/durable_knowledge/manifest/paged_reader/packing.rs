use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PackOutcome {
    Complete,
    Fragment,
    Stop,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pack_resource(
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
