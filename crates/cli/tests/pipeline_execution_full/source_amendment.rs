use super::*;

pub(super) async fn run(
    mut client: &mut Mcp,
    pool: &PgPool,
    mut context: Value,
) -> (Value, Uuid, Value, Value, Value) {
    let phase_five_binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap()
        .clone();
    let phase_five_output = context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["id"] == phase_five_binding["output_id"])
        .unwrap()
        .clone();
    let phase_five_artifact = phase_five_output["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let phase_five_ledger: Value =
        serde_json::from_str(phase_five_artifact["body"].as_str().unwrap()).unwrap();
    let definition_digest_before_amendment = context["run"]["definition_digest"].clone();
    let successor_body = "# Amended design source\n\nThe direct operator instruction authorizes this bounded amendment.";
    let successor_digest = format!("{:x}", Sha256::digest(successor_body.as_bytes()));
    let authority_text = "Direct operator instruction: amend the Full Design source and rerun phase 5 through reconciliation.";
    let amendment_request_id = Uuid::new_v4();
    let amendment = json!({
        "request_id":amendment_request_id,
        "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "input":authority_text,
        "source_amendment":{
            "target_phase_id":"slice-component-decision-interrogator",
            "predecessor":{
                "output_id":phase_five_binding["output_id"],
                "output_revision":phase_five_binding["output_revision"],
                "output_digest":phase_five_binding["output_digest"],
                "artifact_name":"requirements-ledger.json",
                "artifact_digest":phase_five_artifact["digest"],
                "source_path":phase_five_ledger["source"]["path"],
                "source_digest":phase_five_ledger["source"]["digest"]
            },
            "successor":{
                "path":"source-spec.md",
                "artifact":{
                    "name":"source-spec.md","media_type":"text/markdown",
                    "body":successor_body,"digest":successor_digest
                }
            },
            "authorization_scope":"amend the current Full Design source and rerun phase 5 dependency closure",
            "authorization_provenance":"exact direct operator instruction persisted in input"
        }
    });

    let before_rejections = context.clone();
    for (field, wrong) in [
        ("output_id", json!(Uuid::new_v4())),
        (
            "output_revision",
            json!(phase_five_binding["output_revision"].as_i64().unwrap() + 1),
        ),
        ("output_digest", json!("0".repeat(64))),
        ("artifact_digest", json!("1".repeat(64))),
        ("source_path", json!("other-source.md")),
        ("source_digest", json!("sha256:other-source")),
    ] {
        let mut wrong_predecessor = amendment.clone();
        wrong_predecessor["request_id"] = json!(Uuid::new_v4());
        wrong_predecessor["source_amendment"]["predecessor"][field] = wrong;
        let error = route_error(
            &mut client,
            "command",
            "slice.pipeline.input",
            wrong_predecessor,
        )
        .await;
        assert_eq!(
            error["error"]["refusal"]["code"], "INVALID_OUTPUT",
            "{field}: {error}"
        );
        assert_eq!(error["error"]["refusal"]["rule"], "WP6-INPUT-01");
    }
    let mut wrong_artifact = amendment.clone();
    wrong_artifact["request_id"] = json!(Uuid::new_v4());
    wrong_artifact["source_amendment"]["predecessor"]["artifact_name"] =
        json!("decision-traceability.json");
    let wrong_artifact_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        wrong_artifact,
    )
    .await;
    assert_eq!(
        wrong_artifact_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        wrong_artifact_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let mut out_of_scope = amendment.clone();
    out_of_scope["request_id"] = json!(Uuid::new_v4());
    out_of_scope["source_amendment"]["target_phase_id"] = json!("slice-design-spec-shaper");
    let out_of_scope_error =
        route_error(&mut client, "command", "slice.pipeline.input", out_of_scope).await;
    assert_eq!(
        out_of_scope_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        out_of_scope_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let mut digest_mismatch = amendment.clone();
    digest_mismatch["request_id"] = json!(Uuid::new_v4());
    digest_mismatch["source_amendment"]["successor"]["artifact"]["digest"] = json!("0".repeat(64));
    let digest_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        digest_mismatch,
    )
    .await;
    assert_eq!(digest_error["error"]["code"], "invalid_arguments");

    let mut missing_authority = amendment.clone();
    missing_authority["request_id"] = json!(Uuid::new_v4());
    missing_authority["source_amendment"]["authorization_scope"] = json!("");
    let authority_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        missing_authority,
    )
    .await;
    assert_eq!(authority_error["error"]["code"], "invalid_arguments");
    let mut path_name_mismatch = amendment.clone();
    path_name_mismatch["request_id"] = json!(Uuid::new_v4());
    path_name_mismatch["source_amendment"]["successor"]["artifact"]["name"] =
        json!("other-source.md");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.input",
            path_name_mismatch
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let after_rejections = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    for collection in ["run", "inputs", "bindings"] {
        assert_eq!(after_rejections[collection], before_rejections[collection]);
    }
    assert_eq!(
        after_rejections["run"]["definition_digest"],
        definition_digest_before_amendment
    );

    let amended = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        amendment.clone(),
    )
    .await;
    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        amendment.clone(),
    )
    .await;
    assert_eq!(replay, amended);
    context = amended["context"].clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-component-decision-interrogator"
    );
    assert_eq!(
        context["inputs"].as_array().unwrap().last().unwrap()["input"],
        authority_text
    );
    assert_eq!(
        context["inputs"].as_array().unwrap().last().unwrap()["phase_id"],
        "slice-component-decision-interrogator"
    );
    let persisted: (String, Uuid, Value) = sqlx::query_as(
        "SELECT input,actor_session_id,request_payload FROM slice_pipeline_inputs WHERE request_id=$1",
    )
    .bind(amendment_request_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, authority_text);
    assert!(!persisted.1.is_nil());
    assert_eq!(persisted.2["input"], amendment["input"]);
    assert_eq!(
        persisted.2["source_amendment"]["authorization_scope"],
        amendment["source_amendment"]["authorization_scope"]
    );
    assert_eq!(
        persisted.2["source_amendment"]["authorization_provenance"],
        amendment["source_amendment"]["authorization_provenance"]
    );

    (
        amendment,
        persisted.1,
        definition_digest_before_amendment,
        phase_five_binding,
        phase_five_output,
    )
}
