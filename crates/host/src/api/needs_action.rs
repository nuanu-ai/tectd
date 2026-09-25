pub(crate) fn needs_action(
    kind: &str,
    internal: &str,
    params: Value,
    descriptor_name: &str,
    descriptor: Value,
) -> Result<Value> {
    if !matches!(kind, "needs_input" | "needs_context") {
        return Err(Error::InternalInvariant);
    }
    let (tool, arguments) = public_call(internal, params)?;
    let mut properties = if tool == "get_state" {
        Some(std::collections::BTreeSet::new())
    } else {
        route_for_internal(internal).and_then(|spec| allowed_properties(&spec.schema))
    };
    // Legacy phase completion actions may carry backend-issued dependency
    // receipts even though the current v0.7 public schema deliberately omits
    // these caller-forbidden fields.
    if internal == "slice_pipeline_phase_complete"
        && let Some(properties) = properties.as_mut()
    {
        properties.extend(["consumed_outputs".to_owned(), "consumed_inputs".to_owned()]);
    }
    let known = arguments
        .get("params")
        .and_then(Value::as_object)
        .ok_or(Error::InternalInvariant)?;
    if properties.is_none_or(|allowed| known.keys().any(|key| !allowed.contains(key))) {
        return Err(Error::InternalInvariant);
    }
    let mut action = json!({"kind":kind,"tool":tool,"arguments":arguments});
    action[descriptor_name] = descriptor;
    Ok(action)
}
