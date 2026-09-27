use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{
    MatrixRequirementsLocator, MatrixTaskRevision, MatrixTaskSource, RecordMatrixTask,
};
use tect_domain::{EngineeringChoiceSet, Error, Result};
use uuid::Uuid;

const MAX_MATRIX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_OPERATIONAL_FACTS: usize = 1024;

pub(crate) enum MatrixTaskInvocation {
    Record(Box<RecordMatrixTask>),
    BoundRecord(Box<RecordMatrixTask>, MatrixRequirementsLocator),
    Get(Uuid),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordArguments {
    task_id: Uuid,
    revision: i64,
    expected_current_revision: i64,
    request_id: Uuid,
    input: tect_domain::EngineeringMatrixInput,
    #[serde(default)]
    choice_set: Option<EngineeringChoiceSet>,
    #[serde(default)]
    requirements_locator: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    task_id: Uuid,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixTaskInvocation> {
    match name {
        "record_matrix_task" => {
            let input_bytes = serde_json::to_vec(&arguments["input"])
                .map_err(|_| Error::InvalidArguments)?
                .len();
            let choice_bytes = match arguments.get("choice_set") {
                Some(Value::Null) => return Err(Error::InvalidArguments),
                Some(choice_set) => serde_json::to_vec(choice_set)
                    .map_err(|_| Error::InvalidArguments)?
                    .len(),
                None => 0,
            };
            if arguments.get("requirements_locator") == Some(&Value::Null) {
                return Err(Error::InvalidArguments);
            }
            if input_bytes + choice_bytes > MAX_MATRIX_INPUT_BYTES {
                return Err(Error::InvalidArguments);
            }
            reject_unknown_input_fields(&arguments["input"])?;
            let args: RecordArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            let request = RecordMatrixTask {
                task_id: args.task_id,
                revision: args.revision,
                expected_current_revision: args.expected_current_revision,
                request_id: args.request_id,
                input: args.input,
                choice_set: args.choice_set,
            };
            if request.task_id.is_nil()
                || request.request_id.is_nil()
                || request.revision < 1
                || request.expected_current_revision != request.revision - 1
            {
                return Err(Error::InvalidArguments);
            }
            request.input.validate()?;
            if let Some(choice_set) = &request.choice_set {
                if choice_set.task_id != request.task_id.to_string()
                    || choice_set.task_revision != request.revision.to_string()
                {
                    return Err(Error::InvalidArguments);
                }
                choice_set.validate(&request.input)?;
            }
            match args.requirements_locator {
                Some(value) => Ok(MatrixTaskInvocation::BoundRecord(
                    Box::new(request),
                    crate::matrix_requirements_context_tools::parse_locator(value)?,
                )),
                None => Ok(MatrixTaskInvocation::Record(Box::new(request))),
            }
        }
        "get_matrix_task" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixTaskInvocation::Get(args.task_id))
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value.as_object().ok_or(Error::InvalidArguments)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn fact(value: &Value, nested: Option<fn(&Value) -> Result<()>>) -> Result<()> {
    match value["state"].as_str() {
        Some("absent") => fields(value, &["state"]),
        Some("known_empty" | "unknown" | "gap" | "conflict" | "invalid") => {
            fields(value, &["state", "provenance"])
        }
        Some("known") => {
            fields(value, &["state", "value", "provenance"])?;
            if let Some(check) = nested {
                check(&value["value"])?;
            }
            Ok(())
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn intent(value: &Value) -> Result<()> {
    match value["kind"].as_str() {
        Some("production_hotfix") => fields(value, &["kind"]),
        Some("other") => fields(value, &["kind", "description"]),
        _ => Err(Error::InvalidArguments),
    }
}

fn operational_facts(value: &Value) -> Result<()> {
    match value["state"].as_str() {
        Some("absent") => fields(value, &["state"]),
        Some("known_empty") => fields(value, &["state", "provenance"]),
        Some("reported") => {
            fields(value, &["state", "entries"])?;
            let entries = value["entries"].as_array().ok_or(Error::InvalidArguments)?;
            if entries.len() > MAX_OPERATIONAL_FACTS {
                return Err(Error::InvalidArguments);
            }
            for entry in entries {
                fields(entry, &["name", "fact"])?;
                fact(&entry["fact"], None)?;
            }
            Ok(())
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn guard_record_output(request: &RecordMatrixTask, capacity: usize) -> Result<()> {
    // This projection has the same JSON width as a committed revision: UUIDs
    // are fixed-width and the digest is always 64 lowercase hex characters.
    // Check the complete MCP tool response before making the durable write.
    let projected = revision(MatrixTaskRevision {
        task_id: request.task_id,
        revision: request.revision,
        request_id: request.request_id,
        input: request.input.clone(),
        input_digest: "0".repeat(64),
        choice_set: request.choice_set.clone(),
        choice_set_digest: request.choice_set.as_ref().map(|_| "0".repeat(64)),
        recorded_by_principal_id: Uuid::nil(),
        recorded_by_session_id: Uuid::nil(),
    });
    let response = crate::responses::with_actions(projected, Vec::new(), None);
    if crate::responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
}

pub(crate) fn guard_bound_record_output(request: &RecordMatrixTask, capacity: usize) -> Result<()> {
    // Bound declarations can inject at most seven short fields with a digest
    // provenance. Reserve room for those and the immutable context locator.
    guard_record_output(request, capacity.saturating_sub(4096))
}

fn reject_unknown_input_fields(input: &Value) -> Result<()> {
    fields(
        input,
        &[
            "mode",
            "envelope",
            "criticality",
            "intent",
            "urgency",
            "promised_behavior",
            "promised_proof",
            "affected_guarantees",
            "actual_exposure",
            "demand_commitment",
            "latency_commitment",
            "urgent_repair",
        ],
    )?;
    fact(&input["mode"], None)?;
    fields(&input["envelope"], &["scale", "operational_facts"])?;
    fact(&input["envelope"]["scale"], None)?;
    operational_facts(&input["envelope"]["operational_facts"])?;
    for key in [
        "criticality",
        "urgency",
        "promised_behavior",
        "promised_proof",
        "affected_guarantees",
        "actual_exposure",
        "demand_commitment",
        "latency_commitment",
        "urgent_repair",
    ] {
        fact(&input[key], None)?;
    }
    fact(&input["intent"], Some(intent))
}

pub(crate) fn revision(revision: MatrixTaskRevision) -> Value {
    let mut output = json!({
        "task_id":revision.task_id,
        "revision":revision.revision,
        "request_id":revision.request_id,
        "input":revision.input,
        "input_digest":revision.input_digest,
        "recorded_by_principal_id":revision.recorded_by_principal_id,
        "recorded_by_session_id":revision.recorded_by_session_id,
    });
    if let Some(choice_set) = revision.choice_set {
        output["choice_set"] = json!(choice_set);
    }
    if let Some(digest) = revision.choice_set_digest {
        output["choice_set_digest"] = json!(digest);
    }
    output
}

pub(crate) fn source(source: MatrixTaskSource) -> Value {
    let mut output = revision(source.revision);
    if let Some(binding) = source.requirements_binding {
        output["requirements_snapshot_id"] = json!(binding.snapshot_id);
        output["requirements_semantic_digest"] = json!(binding.semantic_digest);
        output["context_authority_schema"] = json!(binding.authority_schema);
        output["requirements_locator"] = binding.locator.as_json();
    } else {
        output["requirements_snapshot_id"] = Value::Null;
        output["requirements_semantic_digest"] = Value::Null;
        output["context_authority_schema"] = Value::Null;
        output["requirements_locator"] = Value::Null;
    }
    output
}

#[cfg(test)]
#[path = "matrix_task_tools/tests.rs"]
mod tests;
