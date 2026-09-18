use serde::Deserialize;
use serde_json::Value;
use tect_domain::{
    Error, KnowledgeContextQuery, PrepareKnowledgeChange, PublishKnowledgeChange,
    RefreshPipelineKnowledge, Result, ReviewKnowledgeChange,
};
use uuid::Uuid;

pub(crate) enum KnowledgeInvocation {
    Context(KnowledgeContextQuery),
    Change(Uuid),
    Prepare(Box<PrepareKnowledgeChange>),
    Review(ReviewKnowledgeChange),
    Publish(PublishKnowledgeChange),
    Refresh(RefreshPipelineKnowledge),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeArguments {
    change_id: Uuid,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<KnowledgeInvocation> {
    reject_optional_nulls(&arguments)?;
    match name {
        "knowledge_context" => {
            let request: KnowledgeContextQuery = decode(arguments)?;
            request.validate()?;
            Ok(KnowledgeInvocation::Context(request))
        }
        "knowledge_change" => {
            let arguments: ChangeArguments = decode(arguments)?;
            if arguments.change_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(KnowledgeInvocation::Change(arguments.change_id))
        }
        "knowledge_change_prepare" => {
            let request: PrepareKnowledgeChange = decode(arguments)?;
            request.validate()?;
            Ok(KnowledgeInvocation::Prepare(Box::new(request)))
        }
        "knowledge_change_review" => {
            let request: ReviewKnowledgeChange = decode(arguments)?;
            request.validate()?;
            Ok(KnowledgeInvocation::Review(request))
        }
        "knowledge_change_publish" => {
            let request: PublishKnowledgeChange = decode(arguments)?;
            request.validate()?;
            Ok(KnowledgeInvocation::Publish(request))
        }
        "pipeline_knowledge_refresh" => {
            let request: RefreshPipelineKnowledge = decode(arguments)?;
            request.validate()?;
            Ok(KnowledgeInvocation::Refresh(request))
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn decode<T: for<'de> serde::Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
}

fn reject_optional_nulls(value: &Value) -> Result<()> {
    const OPTIONAL: &[&str] = &["unit_id", "revision", "expected_unit_revision", "draft"];
    match value {
        Value::Object(object) => {
            if object
                .iter()
                .any(|(key, value)| OPTIONAL.contains(&key.as_str()) && value.is_null())
            {
                return Err(Error::InvalidArguments);
            }
            for nested in object.values() {
                reject_optional_nulls(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                reject_optional_nulls(nested)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_context_and_change_shapes_are_strict() {
        assert!(matches!(
            parse("knowledge_context", json!({})),
            Ok(KnowledgeInvocation::Context(_))
        ));
        assert!(matches!(
            parse(
                "knowledge_context",
                json!({"unit_id":Uuid::new_v4(),"revision":1})
            ),
            Ok(KnowledgeInvocation::Context(_))
        ));
        assert!(parse("knowledge_context", json!({"unit_id":null})).is_err());
        assert!(parse("knowledge_context", json!({"forged":"tenant"})).is_err());
        assert!(parse("knowledge_change", json!({"change_id":Uuid::new_v4()})).is_ok());
        assert!(parse("knowledge_change", json!({"change_id":Uuid::nil()})).is_err());
    }
}
