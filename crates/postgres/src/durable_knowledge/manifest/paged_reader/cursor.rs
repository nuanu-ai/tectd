use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

/// Public checksum is for accidental corruption only. Every use reauthorizes
/// the manifest and all children; callers may construct any valid position.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageCursor {
    version: u8,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: String,
    pub(super) ordinal: i64,
    pub(super) byte_offset: usize,
    checksum: String,
}

impl PageCursor {
    pub(super) fn new(
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

    pub(super) fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("cursor serializes"))
    }

    pub(super) fn decode(
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
