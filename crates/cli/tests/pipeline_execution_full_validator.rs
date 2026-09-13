use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn write_json(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn validator(root: &Path, stage: &str) -> std::process::Output {
    Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../skills/references/spec-pipeline-shared/validate-spec-pipeline.js"
        ))
        .arg(root)
        .args(["--stage", stage])
        .output()
        .unwrap()
}

fn minimal_sidecars(root: &Path) {
    write_json(
        root,
        "requirements-ledger.json",
        &json!({
            "schemaVersion":"1.0",
            "source":{"path":"source-spec.md","digest":"sha256:source"},
            "sourceRequirementIds":["REQ-001"],
            "requirements":[{
                "id":"REQ-001","sourceRef":"source-spec.md#contract",
                "text":"The implementation must preserve the source contract.",
                "modality":"MUST","scope":"in","owner":"implementation",
                "observableOutcomes":["The source contract is implemented."],
                "negativeCases":["Corrupted source-contract material is rejected."],
                "status":"covered"
            }]
        }),
    );
    write_json(
        root,
        "decision-traceability.json",
        &json!({"schemaVersion":"1.0","requirements":[{
            "requirementId":"REQ-001","decisionRefs":["decisions/contract.md"],
            "acceptanceObligationIds":["OBL-REQ-001-POSITIVE","OBL-REQ-001-NEGATIVE"]
        }]}),
    );
    write_json(
        root,
        "acceptance-obligations.json",
        &json!({"schemaVersion":"1.0","obligations":[
            {"id":"OBL-REQ-001-POSITIVE","requirementIds":["REQ-001"],
             "kind":"positive","owner":"implementation","observable":"The contract passes.",
             "verification":"Focused integration proof."},
            {"id":"OBL-REQ-001-NEGATIVE","requirementIds":["REQ-001"],
             "kind":"negative","owner":"implementation","observable":"Corruption fails.",
             "verification":"Corrupt the source contract and rerun validation."}
        ]}),
    );
    write_json(
        root,
        "reconciliation-closure.json",
        &json!({"schemaVersion":"1.0","mode":"closure-audit",
            "unresolvedRequirementIds":[],"completionClaim":"ready"}),
    );
    write_json(
        root,
        "synthesis-traceability.json",
        &json!({"schemaVersion":"1.0","requirements":[{
            "requirementId":"REQ-001","modality":"MUST",
            "sectionRefs":["implementation-ready-spec.md#contract"]
        }],"completionClaim":"ready"}),
    );
}

#[test]
fn vendored_full_validator_accepts_minimal_sidecars_and_rejects_corrupted_contract() {
    let temp = tempfile::tempdir().unwrap();
    minimal_sidecars(temp.path());

    for stage in ["reconciliation", "synthesis"] {
        let output = validator(temp.path(), stage);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["valid"], true);
        assert_eq!(receipt["stage"], stage);
        assert!(receipt["errors"].as_array().unwrap().is_empty());
    }

    let ledger_path = temp.path().join("requirements-ledger.json");
    let mut ledger: Value = serde_json::from_slice(&fs::read(&ledger_path).unwrap()).unwrap();
    ledger["requirements"][0]["modality"] = json!("INVALID");
    write_json(temp.path(), "requirements-ledger.json", &ledger);

    let output = validator(temp.path(), "synthesis");
    assert_eq!(output.status.code(), Some(1));
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    let codes = receipt["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|error| error["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"REQUIREMENT_MODALITY_INVALID"));
    assert!(codes.contains(&"SYNTHESIS_MODALITY_MISMATCH"));
}
