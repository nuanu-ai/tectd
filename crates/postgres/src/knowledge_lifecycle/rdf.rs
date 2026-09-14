//! Typed DK-2 RDF codec and the narrow native lifecycle adapter.
mod encode;
mod model;
mod native;
mod sections;

#[cfg(test)]
mod tests;

pub(crate) use model::RdfDocument;
pub(crate) use native::{native_publish, native_rows, qualify_native};

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
}

pub(crate) fn build(input: &RdfPublicationInput) -> Result<RdfDocument> {
    encode::build(input)
}

pub(crate) fn validate_rows(rows: &[serde_json::Value], document: &RdfDocument) -> Result<()> {
    model::validate_rows(rows, document)
}
