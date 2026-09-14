use serde_json::Value;
use tect_domain::*;

pub(crate) enum KnowledgeMaintenanceInvocation {
    Query(KnowledgeMaintenanceQuery),
    Observe(ObserveKnowledgeMaintenanceSignal),
    Begin(Box<BeginKnowledgeMaintenanceChange>),
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<KnowledgeMaintenanceInvocation> {
    reject_nulls(&arguments)?;
    match name {
        "knowledge_maintenance" => checked(arguments).map(KnowledgeMaintenanceInvocation::Query),
        "knowledge_maintenance_observe" => {
            checked(arguments).map(KnowledgeMaintenanceInvocation::Observe)
        }
        "knowledge_maintenance_begin" => checked(arguments)
            .map(Box::new)
            .map(KnowledgeMaintenanceInvocation::Begin),
        _ => Err(Error::InvalidArguments),
    }
}

trait Validated {
    fn validate_input(&self) -> Result<()>;
}

impl Validated for KnowledgeMaintenanceQuery {
    fn validate_input(&self) -> Result<()> {
        self.validate()
    }
}
impl Validated for ObserveKnowledgeMaintenanceSignal {
    fn validate_input(&self) -> Result<()> {
        self.validate()
    }
}
impl Validated for BeginKnowledgeMaintenanceChange {
    fn validate_input(&self) -> Result<()> {
        self.validate()
    }
}

fn checked<T: for<'de> serde::Deserialize<'de> + Validated>(value: Value) -> Result<T> {
    let value = serde_json::from_value::<T>(value).map_err(|_| Error::InvalidArguments)?;
    value.validate_input()?;
    Ok(value)
}

fn reject_nulls(value: &Value) -> Result<()> {
    match value {
        Value::Null => return Err(Error::InvalidArguments),
        Value::Array(values) => {
            for value in values {
                reject_nulls(value)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                reject_nulls(value)?;
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
    fn exact_variants_validate_and_unknown_or_null_inputs_fail() {
        assert!(parse("knowledge_maintenance", json!({"limit":25})).is_ok());
        assert!(
            parse(
                "knowledge_maintenance",
                json!({"limit":25,"states":["pending","needs_review"]})
            )
            .is_ok()
        );
        assert!(matches!(
            parse(
                "knowledge_maintenance",
                json!({"limit":25,"states":["pending","pending"]})
            ),
            Err(Error::InvalidArguments)
        ));
        assert!(matches!(
            parse("knowledge_maintenance", json!({"limit":25,"after":null})),
            Err(Error::InvalidArguments)
        ));
        assert!(matches!(
            parse(
                "knowledge_maintenance",
                json!({"limit":25,"sql":"select 1"})
            ),
            Err(Error::InvalidArguments)
        ));
    }
}
