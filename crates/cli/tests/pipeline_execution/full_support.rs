use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

const JSON_DIGEST: &str = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
const MARKDOWN_DIGEST: &str = "a6ca93bcc504efd31c0b1f31b2e646d1d6e390c6531a3822a1eb115e3e66bf0c";

fn pattern_matches(pattern: &str, name: &str) -> bool {
    pattern
        .split_once('*')
        .map_or(pattern == name, |(prefix, suffix)| {
            name.starts_with(prefix) && name.ends_with(suffix)
        })
}

fn artifact_name(pattern: &str, sequence: usize) -> String {
    pattern.split_once('*').map_or_else(
        || pattern.to_owned(),
        |(prefix, suffix)| format!("{prefix}fixture-{sequence}{suffix}"),
    )
}

fn artifacts(phase: &Value, verdict: &str) -> Vec<Value> {
    let mut values: Vec<Value> = vec![];
    for requirement in phase["required_artifacts"].as_array().into_iter().flatten() {
        let applies = requirement["when_verdict"].is_null()
            || requirement["when_verdict"].as_str() == Some(verdict);
        if !applies || requirement["required"] != true {
            continue;
        }
        let pattern = requirement["name_pattern"].as_str().unwrap();
        let minimum = requirement["minimum_matches"].as_u64().unwrap() as usize;
        while values
            .iter()
            .filter(|artifact| pattern_matches(pattern, artifact["name"].as_str().unwrap()))
            .count()
            < minimum
        {
            let name = artifact_name(pattern, values.len() + 1);
            if values.iter().any(|artifact| artifact["name"] == name) {
                break;
            }
            let media_type = requirement["media_type"].as_str().unwrap();
            let (body, digest) = if media_type == "application/json" {
                ("{}", JSON_DIGEST)
            } else {
                ("Full pipeline fixture artifact.\n", MARKDOWN_DIGEST)
            };
            values.push(json!({"name":name,"media_type":media_type,"body":body,
                "digest":digest,"reference":format!("fixture-artifact:{pattern}")}));
        }
    }
    values
}

fn receipts(items: &[Value]) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            json!({"instruction_id":item["id"],"version":item["version"],
                "digest":item["digest"]})
        })
        .collect()
}

fn validator_receipts(phase: &Value, verdict: &str, artifacts: &[Value]) -> Vec<Value> {
    phase["validator_contracts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|contract| {
            contract["required_verdicts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|required| required == verdict)
        })
        .map(|contract| {
            let patterns = contract["artifact_patterns"].as_array().unwrap();
            let bound = artifacts
                .iter()
                .filter(|artifact| {
                    patterns.iter().any(|pattern| {
                        pattern_matches(
                            pattern.as_str().unwrap(),
                            artifact["name"].as_str().unwrap(),
                        )
                    })
                })
                .map(|artifact| json!({"name":artifact["name"],"digest":artifact["digest"]}))
                .collect::<Vec<_>>();
            json!({"resource_id":contract["resource_id"],"version":contract["version"],
                "digest":contract["digest"],"stage":contract["stage"],
                "command":format!("node validate-spec-pipeline.js fixture --stage {}",contract["stage"].as_str().unwrap()),
                "exit_code":0,"valid":true,"artifacts":bound})
        })
        .collect()
}

fn fields(phase: &Value, verdict: &str) -> Map<String, Value> {
    let mut fields = phase["required_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            (
                field.as_str().unwrap().to_owned(),
                json!("fixture evidence"),
            )
        })
        .collect::<Map<_, _>>();
    for constraint in phase["output_constraints"].as_array().unwrap() {
        let applies = constraint["when_verdict"].is_null()
            || constraint["when_verdict"].as_str() == Some(verdict);
        if !applies {
            continue;
        }
        let field = constraint["field"].as_str().unwrap();
        match constraint["kind"].as_str().unwrap() {
            "field_equals" => fields.insert(field.into(), constraint["value"].clone()),
            "field_integer_equals" => fields.insert(
                field.into(),
                json!(constraint["value"].as_i64().unwrap().to_string()),
            ),
            "field_integer_minimum" => {
                let minimum = constraint["value"].as_i64().unwrap();
                let current = fields[field]
                    .as_str()
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(minimum);
                fields.insert(field.into(), json!(current.max(minimum).to_string()))
            }
            "field_boolean_equals" => fields.insert(
                field.into(),
                json!(constraint["value"].as_bool().unwrap().to_string()),
            ),
            "field_one_of" => fields.insert(field.into(), constraint["values"][0].clone()),
            _ => continue,
        };
    }
    for constraint in phase["output_constraints"].as_array().unwrap() {
        if constraint["kind"] == "fields_equal"
            && (constraint["when_verdict"].is_null()
                || constraint["when_verdict"].as_str() == Some(verdict))
        {
            let field = constraint["field"].as_str().unwrap();
            let other = constraint["other_field"].as_str().unwrap();
            let equal = match (
                fields[field]
                    .as_str()
                    .and_then(|value| value.parse::<i64>().ok()),
                fields[other]
                    .as_str()
                    .and_then(|value| value.parse::<i64>().ok()),
            ) {
                (Some(left), Some(right)) => json!(left.max(right).to_string()),
                _ => fields[field].clone(),
            };
            fields.insert(field.into(), equal.clone());
            fields.insert(other.into(), equal);
        }
    }
    fields
}

pub(super) fn consumed_outputs(context: &Value) -> Vec<Value> {
    let current = context["run"]["current_phase_ordinal"].as_u64().unwrap();
    context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|binding| {
            binding["stale"] == false && binding["phase_ordinal"].as_u64().unwrap() < current
        })
        .map(|binding| {
            json!({"phase_id":binding["phase_id"],
            "output_revision":binding["output_revision"],"digest":binding["output_digest"]})
        })
        .collect()
}

fn consumed_inputs(context: &Value) -> Vec<Value> {
    context["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|input| input["phase_id"] == context["run"]["current_phase_id"])
        .map(|input| {
            json!({"input_id":input["id"],"sequence":input["sequence"],
            "digest":input["digest"]})
        })
        .collect()
}

pub(super) fn completion(
    context: &Value,
    verdict: &str,
    outcome: &str,
    transition: &str,
    revisit_phase_id: Option<&str>,
    terminal_result: Option<Value>,
) -> Value {
    let phase = &context["definition"]["phases"][0];
    let artifacts = artifacts(phase, verdict);
    let producer = format!("full-producer:{}", phase["id"].as_str().unwrap());
    let body = format!(
        "Full lifecycle receipt for {}.",
        phase["id"].as_str().unwrap()
    );
    let mut output = json!({
        "body":body,
        "producer_context_id":producer,"fields":fields(phase,verdict),"verdict":verdict,
        "dispositions":phase["verdict_routes"].as_array().unwrap().iter()
            .find(|route| route["verdict"]==verdict && route["outcome"]==outcome
                && route["transition"]==transition && revisit_phase_id.map_or(
                    route["revisit_to"].as_array().is_none_or(Vec::is_empty),
                    |target| route["revisit_to"].as_array().is_some_and(
                        |items| items.iter().any(|item| item==target))))
            .unwrap()["dispositions"].clone(),
        "skill_reads":receipts(phase["skills"].as_array().map(Vec::as_slice).unwrap_or_default()),
        "resource_reads":receipts(phase["resources"].as_array().map(Vec::as_slice).unwrap_or_default()),
        "artifacts":artifacts,
        "validator_receipts":validator_receipts(phase,verdict,&artifacts),
        "reference":format!("full-fixture/{}.md",phase["id"].as_str().unwrap())
    });
    if phase["fresh_reviewer_input"] == true {
        let producers = context["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["stale"] == false)
            .map(|item| item["producer_context_id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        output["reviewer_context"] = json!({"reviewer_identity":"reported-independent-reviewer",
            "reviewer_context_id":producer,"producer_context_ids":producers,"fresh_input":true});
    }
    let mut request = json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":phase["id"],
        "outcome":outcome,"transition":transition,"output":output,
        "consumed_outputs":consumed_outputs(context),"consumed_inputs":consumed_inputs(context),
        "publish_blocked_result":false});
    if let Some(target) = revisit_phase_id {
        request["revisit_phase_id"] = json!(target);
    }
    if let Some(result) = terminal_result {
        request["terminal_result"] = result;
    }
    request
}

pub(super) fn successful_route(context: &Value) -> (&str, &str, &str) {
    let phase = &context["definition"]["phases"][0];
    let route = phase["verdict_routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| {
            route["outcome"] == "completed"
                && (route["transition"] == "continue" || route["transition"] == "complete")
                && route["revisit_to"].as_array().is_none_or(Vec::is_empty)
        })
        .unwrap();
    (
        route["verdict"].as_str().unwrap(),
        route["outcome"].as_str().unwrap(),
        route["transition"].as_str().unwrap(),
    )
}
