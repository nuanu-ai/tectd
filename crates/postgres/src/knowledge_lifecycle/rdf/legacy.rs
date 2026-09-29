//! Read-only projection of persisted DK-2 payloads written before operational
//! references were removed from the public document draft.
use super::RdfPublicationInput;
use super::legacy_operational::{OPERATIONAL_SCHEMA_VERSION, OperationalReferencesDraft};
use serde_json::Value;
use tect_domain::{Error, KnowledgeAccessScope, KnowledgeDocumentDraft, Result};

fn project_source_scopes(document: &mut serde_json::Map<String, Value>) -> Result<()> {
    let scope: KnowledgeAccessScope = serde_json::from_value(
        document
            .get("access_scope")
            .cloned()
            .ok_or(Error::InternalInvariant)?,
    )
    .map_err(|_| Error::InternalInvariant)?;
    let Some(sources) = document.get_mut("sources").and_then(Value::as_array_mut) else {
        return Err(Error::InternalInvariant);
    };
    for source in sources {
        let Some(source) = source.as_object_mut() else {
            return Err(Error::InternalInvariant);
        };
        let variant = match source.get("kind").and_then(Value::as_str) {
            Some("snapshot") => "snapshot",
            Some("pipeline_output") => "output",
            _ => continue,
        };
        let Some(payload) = source.get_mut(variant).and_then(Value::as_object_mut) else {
            return Err(Error::InternalInvariant);
        };
        if let Some(value) = payload.remove("access_scope") {
            let old_scope: Option<KnowledgeAccessScope> =
                serde_json::from_value(value).map_err(|_| Error::InternalInvariant)?;
            if old_scope.is_some_and(|old_scope| old_scope != scope) {
                return Err(Error::InternalInvariant);
            }
        }
    }
    Ok(())
}

pub(crate) fn decode_document(
    mut value: Value,
) -> Result<(KnowledgeDocumentDraft, Option<OperationalReferencesDraft>)> {
    let object = value.as_object_mut().ok_or(Error::InternalInvariant)?;
    let version = object
        .remove("schema_version")
        .map(|value| {
            serde_json::from_value::<Option<u32>>(value).map_err(|_| Error::InternalInvariant)
        })
        .transpose()?
        .flatten();
    if version.is_some_and(|version| version != OPERATIONAL_SCHEMA_VERSION) {
        return Err(Error::InternalInvariant);
    }
    let operational = object
        .remove("operational_refs")
        .map(|value| {
            serde_json::from_value::<Option<OperationalReferencesDraft>>(value)
                .map_err(|_| Error::InternalInvariant)
        })
        .transpose()?
        .flatten();
    if operational.is_some() && version != Some(OPERATIONAL_SCHEMA_VERSION) {
        return Err(Error::InternalInvariant);
    }
    project_source_scopes(object)?;
    let document: KnowledgeDocumentDraft =
        serde_json::from_value(value).map_err(|_| Error::InternalInvariant)?;
    if operational
        .as_ref()
        .is_some_and(|refs: &OperationalReferencesDraft| {
            !refs.validate_shape(document.sources.len())
        })
    {
        return Err(Error::InternalInvariant);
    }
    Ok((document, operational))
}

pub(crate) fn decode_event(
    mut value: Value,
) -> Result<(
    RdfPublicationInput,
    Option<OperationalReferencesDraft>,
    Option<Value>,
)> {
    let document = value
        .get_mut("planned")
        .and_then(Value::as_object_mut)
        .ok_or(Error::InternalInvariant)?
        .get_mut("document");
    let mut operational = None;
    let mut original_document = None;
    if let Some(value) = document
        && !value.is_null()
    {
        original_document = Some(value.clone());
        let (projected, refs) = decode_document(value.clone())?;
        *value = serde_json::to_value(projected).map_err(|_| Error::InternalInvariant)?;
        operational = refs;
    }
    let input = serde_json::from_value(value).map_err(|_| Error::InternalInvariant)?;
    Ok((input, operational, original_document))
}
