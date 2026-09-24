use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{MatrixTaskRevision, RecordMatrixTask};
use tect_domain::{EngineeringChoiceSet, Error, Result};
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
    #[serde(default)]
    choice_set: Option<EngineeringChoiceSet>,
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
            choice_set: request.choice_set.clone(),
            choice_set_digest: request.choice_set.as_ref().map(|_| "0".repeat(64)),
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

        let mut combined = example_params();
        for candidate in combined["choice_set"]["candidates"].as_array_mut().unwrap() {
            candidate["approach"] = json!("a".repeat(4096));
        }
        let choice_bytes = serde_json::to_vec(&combined["choice_set"]).unwrap().len();
        let mut low = 1;
        let mut high = MAX_OPERATIONAL_FACTS;
        let mut found_boundary = false;
        while low <= high {
            let count = low + (high - low) / 2;
            combined["input"]["envelope"]["operational_facts"] = json!({"state":"reported","entries":(0..count).map(|i| json!({"name":format!("fact-{i}"),"fact":{"state":"known","value":"\u{0000}".repeat(256),"provenance":"source"}})).collect::<Vec<_>>()});
            let input_bytes = serde_json::to_vec(&combined["input"]).unwrap().len();
            if input_bytes > MAX_MATRIX_INPUT_BYTES {
                high = count - 1;
            } else if input_bytes + choice_bytes <= MAX_MATRIX_INPUT_BYTES {
                low = count + 1;
            } else {
                assert!(parse("record_matrix_task", combined).is_err());
                found_boundary = true;
                break;
            }
        }
        assert!(
            found_boundary,
            "combined input and choice-set boundary was not reached"
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
            choice_set: None,
            choice_set_digest: None,
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
        assert!(output.get("choice_set").is_none());
        assert!(output.get("choice_set_digest").is_none());
    }

    #[test]
    fn choice_set_accepts_zero_one_and_two_candidates_and_absence() {
        let example = example_params();
        for count in 0..=2 {
            let mut params = example.clone();
            params["choice_set"]["candidates"]
                .as_array_mut()
                .unwrap()
                .truncate(count);
            let MatrixTaskInvocation::Record(request) =
                parse("record_matrix_task", params.clone()).unwrap()
            else {
                panic!("record")
            };
            assert_eq!(request.choice_set.as_ref().unwrap().candidates.len(), count);
            let projected = revision(MatrixTaskRevision {
                task_id: request.task_id,
                revision: request.revision,
                request_id: request.request_id,
                input: request.input.clone(),
                input_digest: "0".repeat(64),
                choice_set: request.choice_set.clone(),
                choice_set_digest: Some("0".repeat(64)),
                recorded_by_principal_id: Uuid::nil(),
                recorded_by_session_id: Uuid::nil(),
            });
            assert_eq!(projected["choice_set"], params["choice_set"]);
            assert_eq!(projected["choice_set_digest"].as_str().unwrap().len(), 64);
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
        }
        let mut legacy = example;
        legacy.as_object_mut().unwrap().remove("choice_set");
        let MatrixTaskInvocation::Record(request) = parse("record_matrix_task", legacy).unwrap()
        else {
            panic!("record")
        };
        assert!(request.choice_set.is_none());
    }

    #[test]
    fn choice_set_rejects_invalid_references_binding_and_nested_fields() {
        let example = example_params();
        for (path, value) in [
            ("task_id", json!(Uuid::new_v4().to_string())),
            ("task_revision", json!("2")),
            ("schema", json!("other")),
        ] {
            let mut params = example.clone();
            params["choice_set"][path] = value;
            assert!(
                parse("record_matrix_task", params).is_err(),
                "accepted {path}"
            );
        }
        let mut invalid_reference = example.clone();
        invalid_reference["choice_set"]["candidates"][0]["assumption_fact_ids"] =
            json!(["not.a.matrix.fact"]);
        assert!(parse("record_matrix_task", invalid_reference).is_err());
        let mut duplicate_id = example.clone();
        duplicate_id["choice_set"]["candidates"][1]["candidate_id"] = json!("approach-a");
        assert!(parse("record_matrix_task", duplicate_id).is_err());
        let mut unknown_set = example.clone();
        unknown_set["choice_set"]["unexpected"] = json!(true);
        assert!(parse("record_matrix_task", unknown_set).is_err());
        let mut unknown_candidate = example.clone();
        unknown_candidate["choice_set"]["candidates"][0]["unexpected"] = json!(true);
        assert!(parse("record_matrix_task", unknown_candidate).is_err());
        let mut explicit_null = example;
        explicit_null["choice_set"] = Value::Null;
        assert!(parse("record_matrix_task", explicit_null).is_err());
    }

    #[test]
    fn changed_choice_set_replay_is_passed_through_for_storage_conflict_check() {
        let original = example_params();
        let mut changed = original.clone();
        changed["choice_set"]["decision_question"] = json!("A changed question?");
        let MatrixTaskInvocation::Record(before) = parse("record_matrix_task", original).unwrap()
        else {
            panic!("record")
        };
        let MatrixTaskInvocation::Record(after) = parse("record_matrix_task", changed).unwrap()
        else {
            panic!("record")
        };
        assert_eq!(before.request_id, after.request_id);
        assert_eq!(before.task_id, after.task_id);
        assert_ne!(before.choice_set, after.choice_set);
    }
}
