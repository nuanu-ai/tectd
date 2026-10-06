use super::*;

const FAILURE_BUDGET: usize = 8192;
fn encode(
    error: &Error,
    call: Option<(&str, &Value)>,
    data: &Value,
) -> std::result::Result<(Value, usize), FailureBuildError> {
    let response = content(failure_intro(error, call), data.clone(), true);
    let size = serde_json::to_vec(&response)
        .map_err(|_| FailureBuildError::construction(Error::TransportUnavailable))?
        .len();
    Ok((response, size))
}
/// Normal diagnostics stay exact. Oversized dynamic fields are omitted honestly,
/// after discarding state/inline contracts and reducing recovery actions.
pub(super) fn fit(
    error: &Error,
    call: Option<(&str, &Value)>,
    mut data: Value,
    capacity: usize,
) -> std::result::Result<Value, FailureBuildError> {
    let capacity = capacity.min(FAILURE_BUDGET);
    let (full, size) = encode(error, call, &data)?;
    if size <= capacity {
        return Ok(full);
    }
    let mut recovery = bounded_recovery(error, call).map_err(FailureBuildError::construction)?;
    strip_failure_contracts(&mut recovery);
    data["actions"] = json!(recovery);
    data["recommended_action"] = json!((!recovery.is_empty()).then_some(0));
    data["error"]
        .as_object_mut()
        .ok_or(FailureBuildError::construction(Error::InternalInvariant))?
        .remove("schema_help");
    let (value, size) = encode(error, call, &data)?;
    if size <= capacity {
        return Ok(value);
    }
    let receipt_recovery = crate::pipeline_output::receipt_diff::is_receipt_failure(error);
    if !receipt_recovery {
        data["actions"] = json!([]);
        data["recommended_action"] = Value::Null;
    }
    let (value, size) = encode(error, call, &data)?;
    if size <= capacity {
        return Ok(value);
    }
    let mut fields = Vec::new();
    dynamic_fields(&data["error"], "", &mut fields);
    fields.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut omitted = Vec::new();
    for (field, utf8_bytes) in fields {
        remove_field(&mut data["error"], &field)?;
        omitted.push(json!({"field":field,"utf8_bytes":utf8_bytes}));
        data["error"]["diagnostic_delivery"] = json!({"complete":false,"omitted":omitted});
        let (value, size) = encode(error, call, &data)?;
        if size <= capacity {
            return add_recovery(error, call, data, recovery, capacity, value);
        }
    }
    // A long omission list must not itself bypass the envelope budget.
    if !omitted.is_empty() {
        data["error"]["diagnostic_delivery"] = json!({"complete":false});
    }
    let (value, size) = encode(error, call, &data)?;
    if size <= capacity {
        return add_recovery(error, call, data, recovery, capacity, value);
    }
    Err(FailureBuildError::EnvelopeCannotFit)
}
fn add_recovery(
    error: &Error,
    call: Option<(&str, &Value)>,
    mut data: Value,
    recovery: Vec<Value>,
    capacity: usize,
    without: Value,
) -> std::result::Result<Value, FailureBuildError> {
    data["actions"] = json!(recovery);
    data["recommended_action"] = json!((!recovery.is_empty()).then_some(0));
    let (with, size) = encode(error, call, &data)?;
    Ok(if size <= capacity { with } else { without })
}
fn dynamic_fields(value: &Value, path: &str, out: &mut Vec<(String, usize)>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "code" | "rule" | "violation_code" | "diagnostic_delivery"
                ) {
                    continue;
                }
                let path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                dynamic_fields(value, &path, out);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                dynamic_fields(value, &format!("{path}/{index}"), out);
            }
        }
        Value::String(value) => out.push((path.into(), value.len())),
        _ => {}
    }
}
fn remove_field(error: &mut Value, path: &str) -> std::result::Result<(), FailureBuildError> {
    let (parent, key) = path
        .rsplit_once('/')
        .ok_or(FailureBuildError::construction(Error::InternalInvariant))?;
    let object = error
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .ok_or(FailureBuildError::construction(Error::InternalInvariant))?;
    object
        .remove(&key.replace("~1", "/").replace("~0", "~"))
        .ok_or(FailureBuildError::construction(Error::InternalInvariant))?;
    Ok(())
}
