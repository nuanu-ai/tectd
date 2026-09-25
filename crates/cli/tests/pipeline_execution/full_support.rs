use super::recovery_support::{Mcp, action_params, find_action};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

const JSON_DIGEST: &str = "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a";
const MARKDOWN_DIGEST: &str = "a6ca93bcc504efd31c0b1f31b2e646d1d6e390c6531a3822a1eb115e3e66bf0c";

pub(super) fn requirements_ledger(count: usize) -> String {
    let ids = (1..=count)
        .map(|index| format!("REQ-{index:03}"))
        .collect::<Vec<_>>();
    let rows = ids
        .iter()
        .map(|id| json!({"id":id,"modality":"MUST"}))
        .collect::<Vec<_>>();
    json!({"schemaVersion":"1.0","source":{"path":"source-spec.md","digest":"sha256:fixture-source"},
        "sourceRequirementIds":ids,"requirements":rows}).to_string()
}

#[allow(dead_code)] // This shared fixture is used by the Full pipeline integration test.
pub(super) fn replace_ledger(output: &mut Value, count: usize) {
    let artifact = output["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let body = requirements_ledger(count);
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    artifact["body"] = json!(body);
    artifact["digest"] = json!(digest);
    for receipt in output["validator_receipts"].as_array_mut().unwrap() {
        for bound in receipt["artifacts"].as_array_mut().unwrap() {
            if bound["name"] == "requirements-ledger.json" {
                bound["digest"] = json!(digest);
            }
        }
    }
}

#[allow(dead_code)]
pub(super) fn assert_non_coding_definition(context: &Value, expected_kind: &str) {
    assert_eq!(context["definition"]["kind"], expected_kind);
    for phase in context["definition"]["phases"].as_array().unwrap() {
        assert!(
            phase["output_constraints"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .all(|constraint| !matches!(
                    constraint["kind"].as_str(),
                    Some("engineering_review" | "code_authorization")
                )),
            "{} unexpectedly carries an engineering authority constraint",
            phase["id"]
        );
        assert!(
            phase["resources"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .all(|resource| !matches!(
                    resource["id"].as_str(),
                    Some(
                        "tect:engineering-standards"
                            | "tect:engineering-review"
                            | "tect:engineering-review-schema"
                    )
                )),
            "{} unexpectedly carries an engineering authority resource",
            phase["id"]
        );
    }
}

#[allow(dead_code)]
pub(super) fn add_opaque_authority_labels(request: &mut Value) {
    request["output"]["fields"]["engineering_review"] = json!("pass");
    request["output"]["fields"]["code_authorization"] = json!("granted");
}

#[allow(dead_code)]
pub(super) async fn assert_forged_implementation_phase_rejected(
    client: &mut Mcp,
    context: &Value,
    request: Value,
    expected_kind: &str,
) {
    let mut forged_constraints = request.clone();
    forged_constraints["request_id"] = json!(Uuid::new_v4());
    forged_constraints["output_constraints"] = json!([
        {"kind":"engineering_review"},
        {"kind":"code_authorization"}
    ]);
    let refused = client
        .call_error(
            "command",
            json!({"route":"slice.pipeline.phase.complete","params":forged_constraints}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "invalid_arguments");

    let mut request = request;
    request["request_id"] = json!(Uuid::new_v4());
    request["phase_id"] = json!("slice-execution-runner");
    let refused = client
        .call_error(
            "command",
            json!({"route":"slice.pipeline.phase.complete","params":request}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "invalid_arguments");
    let current = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context["run"]["id"]}}),
        )
        .await;
    assert_eq!(current["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        current["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    assert_eq!(
        current["run"]["definition_digest"],
        context["run"]["definition_digest"]
    );
    assert_non_coding_definition(&current, expected_kind);
}

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

fn engineering_report(
    phase: &Value,
    verdict: &str,
    outcome: &str,
    consumed: &[Value],
) -> Option<String> {
    let constraint = phase["output_constraints"]
        .as_array()?
        .iter()
        .find(|constraint| constraint["kind"] == "engineering_review")?;
    let pass = constraint["success_verdicts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == verdict);
    let stage = constraint["stage"].as_str().unwrap();
    let assessments = if pass {
        (1..=10)
            .map(|number| {
                json!({
                    "rule_id":format!("ENG-{number:02}"),"status":"satisfied",
                    "rationale":"The fixture supplies concrete current phase evidence.",
                    "evidence_refs":[format!("fixture:{}",phase["id"].as_str().unwrap())]
                })
            })
            .collect::<Vec<_>>()
    } else {
        vec![json!({"rule_id":"ENG-07","status":"unassessed",
            "rationale":"A required fixture basis is unavailable.",
            "evidence_refs":[format!("missing:{}",phase["id"].as_str().unwrap())]})]
    };
    let files = if !pass || stage == "specification" {
        vec![]
    } else if stage == "implementation" {
        vec![
            json!({"path":"src/fixture.rs","content_kind":"behavioral","line_count":20,
            "count_basis":"observed","content_digest":format!("{:x}",Sha256::digest(b"fixture")),
            "responsibility":"Fixture implementation owner."}),
        ]
    } else {
        vec![
            json!({"path":"src/fixture.rs","content_kind":"behavioral","line_count":20,
            "count_basis":"estimate","responsibility":"Fixture implementation owner."}),
        ]
    };
    let mut report = json!({"stage":stage,"rules_digest":constraint["standards_resource_digest"],
        "verdict":if pass {"pass"} else if outcome == "completed" {"rework"} else {"blocked"},"reviewed_outputs":consumed,
        "source_basis":"Current durable predecessor outputs.","assessments":assessments,
        "findings":if pass {json!([])} else {json!([{"id":"fixture-missing-basis","rule_id":"ENG-07","status":"open","evidence":"The required fixture basis is unavailable."}])},
        "files":files,"summary":if pass {"Fixture engineering review passes."} else {"Fixture engineering review cannot pass without the missing basis."}});
    if !constraint["required_prior_review_phase_ids"]
        .as_array()
        .unwrap()
        .is_empty()
    {
        report["prior_finding_ids"] = json!([]);
        report["resolved_finding_ids"] = json!([]);
    }
    Some(serde_json::to_string(&report).unwrap())
}

fn artifacts(phase: &Value, verdict: &str, outcome: &str, consumed: &[Value]) -> Vec<Value> {
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
            let review = (name == "engineering-review.json")
                .then(|| engineering_report(phase, verdict, outcome, consumed))
                .flatten();
            let (body, digest) = if let Some(body) = review {
                let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
                (body, digest)
            } else if name == "requirements-ledger.json"
                && matches!(
                    phase["id"].as_str(),
                    Some("slice-component-decision-interrogator" | "slice-reconciliation-runner")
                )
            {
                let body = requirements_ledger(20);
                let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
                (body, digest)
            } else if media_type == "application/json" {
                ("{}".to_owned(), JSON_DIGEST.to_owned())
            } else {
                (
                    "Full pipeline fixture artifact.\n".to_owned(),
                    MARKDOWN_DIGEST.to_owned(),
                )
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
        let kind = constraint["kind"].as_str().unwrap();
        match kind {
            "engineering_review" | "code_authorization" | "resolved_knowledge_publication" => {
                continue;
            }
            "field_equals"
            | "field_integer_equals"
            | "field_integer_minimum"
            | "field_boolean_equals"
            | "field_one_of"
            | "field_required" => {}
            _ => continue,
        }
        let applies = constraint["when_verdict"].is_null()
            || constraint["when_verdict"].as_str() == Some(verdict);
        if !applies {
            continue;
        }
        let field = constraint["field"].as_str().unwrap();
        match kind {
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
            "field_required" => {
                fields
                    .entry(field)
                    .or_insert_with(|| json!("fixture evidence"));
                None
            }
            _ => unreachable!("field constraint kinds are filtered above"),
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
    for field in [
        "engineering_finding_ids",
        "resolved_engineering_finding_ids",
        "deferred_engineering_finding_ids",
    ] {
        if fields.contains_key(field) {
            fields.insert(field.into(), json!("[]"));
        }
    }
    if fields.contains_key("engineering_finding_authority") {
        fields.insert(
            "engineering_finding_authority".into(),
            json!("not_applicable"),
        );
    }
    fields
}

#[test]
fn non_field_engineering_constraints_do_not_enter_field_dispatch() {
    let phase = json!({
        "required_fields":[],
        "output_constraints":[
            {"kind":"engineering_review","stage":"plan"},
            {"kind":"code_authorization","required_plan_review_phase_id":"review"}
        ]
    });
    assert!(fields(&phase, "pass").is_empty());
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
    let consumed = consumed_outputs(context);
    let artifacts = artifacts(phase, verdict, outcome, &consumed);
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
        "consumed_outputs":consumed,"consumed_inputs":consumed_inputs(context),
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

#[allow(dead_code)] // Only some integration test targets exercise the refresh helper.
pub(super) async fn refresh_knowledge(client: &mut Mcp, context: &Value) -> Value {
    let stale = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context["run"]["id"]}}),
        )
        .await;
    assert_eq!(stale["run"]["id"], context["run"]["id"]);
    assert_eq!(stale["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        stale["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    if stale["knowledge_resource_status"]["state"] == "inactive" {
        assert!(stale["knowledge_resources"].is_null());
        assert!(stale["knowledge"].is_null());
        assert!(find_action(&stale, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    let resource_state = stale["knowledge_resource_status"]["state"]
        .as_str()
        .unwrap();
    assert!(
        matches!(resource_state, "stale" | "needs_context"),
        "{stale}"
    );
    if resource_state == "stale" {
        assert_eq!(
            stale["run"]["revision"].as_i64().unwrap(),
            stale["knowledge_resources"]["run_revision"]
                .as_i64()
                .unwrap()
                + 1
        );
    }
    let action = find_action(&stale, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    client
        .call(
            "command",
            json!({"route":"pipeline.knowledge_refresh","params":action_params(action)}),
        )
        .await;
    let current = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context["run"]["id"]}}),
        )
        .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    current
}
