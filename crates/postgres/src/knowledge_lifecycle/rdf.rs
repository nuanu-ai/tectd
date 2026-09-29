//! Typed DK-2 RDF codec and the narrow native lifecycle adapter.
mod encode;
mod legacy;
mod legacy_operational;
mod model;
mod native;
mod sections;

#[cfg(test)]
mod tests;

pub(crate) use legacy::{decode_document, decode_event};
pub(crate) use model::{RdfDocument, RdfRefs};
pub(crate) use native::{canonical_native_rows, native_publish, native_rows, qualify_native};

use serde::{Deserialize, Serialize};
use tect_domain::{KnowledgePlannedOperation, KnowledgeResolvedSourcePin, Result};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedSourcePayload {
    pub pin: KnowledgeResolvedSourcePin,
    pub title: String,
    pub uri: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RdfPublicationInput {
    pub tenant: Uuid,
    pub workspace: Uuid,
    pub change_id: Uuid,
    pub event_id: Uuid,
    pub content_revision: i64,
    pub planned: KnowledgePlannedOperation,
    pub principal_id: Uuid,
    pub session_id: Uuid,
    pub resolved_sources: Vec<ResolvedSourcePayload>,
    pub successor_unit: Option<Uuid>,
    #[serde(default)]
    pub include_empty_planning_briefs: bool,
}

pub(crate) fn build(input: &RdfPublicationInput) -> Result<RdfDocument> {
    encode::build(input, None, true)
}

pub(crate) fn build_stored(
    input: &RdfPublicationInput,
    operational: Option<&legacy_operational::OperationalReferencesDraft>,
) -> Result<RdfDocument> {
    // Stored events have already crossed their publication gate. Verification
    // re-encodes their historical shape and compares every native RDF row.
    encode::build(input, operational, false)
}

pub(crate) fn validate_rows(rows: &[serde_json::Value], document: &RdfDocument) -> Result<()> {
    model::validate_rows(rows, document)
}
