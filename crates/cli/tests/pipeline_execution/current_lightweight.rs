use serde_json::{Value, json};

const CURRENT: &str =
    include_str!("../../../host/pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json");
const COMPAT: &str =
    include_str!("../../../host/pipeline-definitions/lightweight-tdd-0.7.0-native.k1k5.json");
const BOUNDARY: &str = "Structural fixture only: no commands executed, independent semantic QA, deployment, live verification or paid acceptance.";

pub(super) fn is_canonical_phase(phase: &Value) -> bool {
    [CURRENT, COMPAT].into_iter().any(|source| {
        let definition: tect_domain::PipelineDefinitionSnapshot =
            serde_json::from_str(source).unwrap();
        assert_eq!(
            definition.kind,
            tect_domain::PipelineKind::LightweightTddDevelopment
        );
        definition
            .phases
            .iter()
            .any(|stored| serde_json::to_value(stored).unwrap() == *phase)
    })
}

fn command_receipt(status: &str, scope: &str, target: &str) -> String {
    json!({"command":format!("STRUCTURAL_FIXTURE_NOT_EXECUTED:{scope}"),
        "target":target,"status":status,"exit_code":if status == "failed_as_expected" {1} else {0},
        "fresh":true,"skipped":false,"scopes":[scope]})
    .to_string()
}

pub(super) fn phase_output(phase: &Value, outcome: &str, transition: &str) -> Value {
    let marker = phase["id"].as_str().unwrap();
    let routes = phase["verdict_routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|route| route["outcome"] == outcome && route["transition"] == transition)
        .collect::<Vec<_>>();
    assert_eq!(
        routes.len(),
        1,
        "{marker} must declare exactly one {outcome}/{transition} route"
    );
    let route = routes[0];
    let mut fields = phase["required_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            let field = field.as_str().unwrap();
            (
                field.to_owned(),
                json!(format!("STRUCTURAL_FIXTURE_NOT_EXECUTED:{marker}:{field}")),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let values = match marker {
        "K1" => {
            json!({"fit":"bounded_understood","parent":"current_confirmed","preflight":"current_clear","authority":"authorized","route":"none",
            "request":"Structural fixture for a bounded correction.","acceptance_checks":"Fixture consumer-path regression plus affected checks."})
        }
        "K2" => {
            json!({"source_provenance":"fixture:owned-source","worktree_provenance":"fixture:isolated-worktree","isolation":"confirmed","ownership":"confirmed",
            "overlap":"clear","target_proof_plan":"Fixture consumer-path focused and affected test plan.","test_target":"fixture:consumer-path","route":"none"})
        }
        "K3" => {
            json!({"review_mode":"self","findings":"Fixture self review considers whether the requested consumer path remains disconnected.",
            "verdict":route["verdict"],"reviewer":"structural-fixture-self","rules_digest":phase["instructions"][0]["digest"],"missing_proof":"none","next_owner":"none"})
        }
        "K4" => {
            json!({"target_binding":"fixture:consumer-path","red_receipt":command_receipt("failed_as_expected","focused","fixture:consumer-path"),
            "green_receipt":command_receipt("passed","focused","fixture:consumer-path"),"anti_pattern_review":"reviewed_clear","authority_boundary":"authorized","missing_proof":"none",
            "changes":"Fixture minimal correction; no source command executed.","deviations":"none"})
        }
        "K5" => {
            json!({"focused_proof":command_receipt("passed","focused","fixture:consumer-path"),"affected_proof":command_receipt("passed","affected","fixture:affected-path"),
            "deploy_impact":"no_deploy_required","truth_level":"local_verified","missing_proof":"none","promotion":"no_promotion","handoff":"none",
            "result":"Fixture local result carrier; no actual command or independent semantic verification."})
        }
        _ => panic!("current Lightweight fixture cannot select a historical phase"),
    };
    fields.extend(values.as_object().unwrap().clone());
    if route["verdict"] != "pass" {
        if marker == "K1" {
            fields.insert("route".into(), route["verdict"].clone());
        }
        if marker == "K2" {
            fields.insert(
                "route".into(),
                json!(if route["verdict"] == "escalate" {
                    "escalate"
                } else {
                    "defer"
                }),
            );
        }
        if fields.contains_key("missing_proof") {
            fields.insert(
                "missing_proof".into(),
                json!(format!("fixture:{}", route["verdict"].as_str().unwrap())),
            );
        }
        if marker == "K5" {
            fields.insert("truth_level".into(), json!("fixture_not_verified"));
        }
    }
    if let Some(resources) = phase.get("resources") {
        assert!(
            resources
                .as_array()
                .expect("resources must be an array")
                .is_empty()
        );
    }
    let reads = |name: &str| {
        let items = match phase.get(name) {
            None if name == "resources" => &[][..],
            Some(value) => value
                .as_array()
                .expect("asset reads must be an array")
                .as_slice(),
            None => panic!("required asset reads missing: {name}"),
        };
        items.iter().map(|item| json!({"instruction_id":item["id"],"version":item["version"],"digest":item["digest"]})).collect::<Vec<_>>()
    };
    json!({"body":BOUNDARY,"producer_context_id":format!("structural-fixture:{marker}"),"fields":fields,
        "verdict":route["verdict"],"dispositions":route["dispositions"],"reference":format!("fixture:{marker}"),
        "skill_reads":reads("skills"),"resource_reads":reads("resources")})
}
