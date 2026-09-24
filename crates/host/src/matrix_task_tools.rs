use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{MatrixTaskRevision, RecordMatrixTask};
use tect_domain::{Error, Result};
use uuid::Uuid;

const MAX_MATRIX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_OPERATIONAL_FACTS: usize = 1024;

pub(crate) enum MatrixTaskInvocation {
    Record(RecordMatrixTask),
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    task_id: Uuid,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixTaskInvocation> {
    match name {
        "record_matrix_task" => {
            if serde_json::to_vec(&arguments["input"])
                .map_err(|_| Error::InvalidArguments)?
                .len()
                > MAX_MATRIX_INPUT_BYTES
            {
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
            };
            if request.task_id.is_nil()
                || request.request_id.is_nil()
                || request.revision < 1
                || request.expected_current_revision != request.revision - 1
            {
                return Err(Error::InvalidArguments);
            }
            request.input.validate()?;
            Ok(MatrixTaskInvocation::Record(request))
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
        recorded_by_principal_id: Uuid::nil(),
        recorded_by_session_id: Uuid::nil(),
    });
    let response = crate::responses::with_actions(projected, Vec::new(), None);
    if crate::responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
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
    json!({
        "task_id":revision.task_id,
        "revision":revision.revision,
        "request_id":revision.request_id,
        "input":revision.input,
        "input_digest":revision.input_digest,
        "recorded_by_principal_id":revision.recorded_by_principal_id,
        "recorded_by_session_id":revision.recorded_by_session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_params() -> Value {
        crate::api::route_contract("command", "task.source.record").unwrap()["example"]["arguments"]
            ["params"]
            .clone()
    }

    #[test]
    fn source_text_uses_domain_utf8_byte_and_trim_rules() {
        for value in [" \t ".to_owned(), "🦀".repeat(65)] {
            let mut params = example_params();
            params["input"]["criticality"] =
                json!({"state":"known","value":value,"provenance":"source"});
            assert!(parse("record_matrix_task", params).is_err());
        }
        let mut params = example_params();
        params["input"]["criticality"] =
            json!({"state":"known","value":"🦀".repeat(64),"provenance":"source"});
        assert!(parse("record_matrix_task", params).is_ok());
        let mut params = example_params();
        params["input"]["mode"] = json!({"state":"unknown","provenance":" \t "});
        assert!(parse("record_matrix_task", params).is_err());
    }

    #[test]
    fn source_budget_and_response_capacity_fail_before_write() {
        let params = example_params();
        let MatrixTaskInvocation::Record(request) =
            parse("record_matrix_task", params.clone()).unwrap()
        else {
            panic!("record")
        };
        let projected = revision(MatrixTaskRevision {
            task_id: request.task_id,
            revision: request.revision,
            request_id: request.request_id,
            input: request.input.clone(),
            input_digest: "0".repeat(64),
            recorded_by_principal_id: Uuid::nil(),
            recorded_by_session_id: Uuid::nil(),
        });
        let size = crate::responses::encoded_len(&crate::responses::with_actions(
            projected,
            Vec::new(),
            None,
        ))
        .unwrap();
        assert!(guard_record_output(&request, size).is_ok());
        assert!(matches!(
            guard_record_output(&request, size - 1),
            Err(Error::RequestTooLarge)
        ));

        let mut too_many = params.clone();
        too_many["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..=MAX_OPERATIONAL_FACTS).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"absent"}})).collect::<Vec<_>>()});
        assert!(parse("record_matrix_task", too_many).is_err());

        let mut over_bytes = params;
        let control = "\u{0000}".repeat(256);
        over_bytes["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..MAX_OPERATIONAL_FACTS).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":control,"provenance":"source"}})).collect::<Vec<_>>()});
        assert!(serde_json::to_vec(&over_bytes["input"]).unwrap().len() > MAX_MATRIX_INPUT_BYTES);
        assert!(parse("record_matrix_task", over_bytes).is_err());

        let mut within_budget = example_params();
        let control = "\u{0000}".repeat(256);
        within_budget["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..600).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":control,"provenance":"source"}})).collect::<Vec<_>>()});
        assert!(
            serde_json::to_vec(&within_budget["input"]).unwrap().len() <= MAX_MATRIX_INPUT_BYTES
        );
        let MatrixTaskInvocation::Record(large_request) =
            parse("record_matrix_task", within_budget).unwrap()
        else {
            panic!("record")
        };
        assert!(
            guard_record_output(&large_request, crate::frame::MAX_FRAME_BYTES - 16 * 1024).is_ok()
        );
    }

    #[test]
    fn revision_projection_preserves_source_and_server_identity() {
        let example = crate::api::route_contract("command", "task.source.record").unwrap()
            ["example"]["arguments"]["params"]["input"].clone();
        let input = serde_json::from_value(example.clone()).unwrap();
        let task_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let principal = Uuid::new_v4();
        let session = Uuid::new_v4();
        let output = revision(MatrixTaskRevision {
            task_id,
            revision: 2,
            request_id,
            input,
            input_digest: "digest".into(),
            recorded_by_principal_id: principal,
            recorded_by_session_id: session,
        });
        assert_eq!(output["task_id"], json!(task_id));
        assert_eq!(output["revision"], 2);
        assert_eq!(output["request_id"], json!(request_id));
        assert_eq!(output["recorded_by_principal_id"], json!(principal));
        assert_eq!(output["recorded_by_session_id"], json!(session));
        assert_eq!(output["input_digest"], "digest");
        assert_eq!(output["input"], example);
    }
}
