use super::*;

fn completion_args(fake: &AuthorizedFake) -> Value {
    let skills = fake.context.definition.phases[0].skills.iter().map(|skill| {
        json!({"instruction_id":skill.id,"version":skill.version,"digest":skill.digest})
    }).collect::<Vec<_>>();
    json!({"route":"slice.pipeline.phase.complete","params":{
        "request_id":uuid::Uuid::new_v4(),"run_id":fake.context.run.id,
        "run_revision":fake.context.run.revision,"phase_id":"fixture-phase",
        "outcome":"completed","transition":"continue",
        "output":{"body":"body","producer_context_id":"ctx","skill_reads":skills,"resource_reads":[]}}})
}
fn completion_error(args: Value, fake: &AuthorizedFake) -> Error {
    let call = crate::api::decode_public_call("command", args).unwrap();
    let crate::pipeline_tools::PipelineInvocation::Complete(request) =
        crate::pipeline_tools::parse(call.name, call.arguments).unwrap()
    else {
        panic!("complete")
    };
    request.validate(&fake.context.definition).unwrap_err()
}

#[test]
fn omitted_receipt_defaults_recover_full_missing_unicode_diff_for_both_kinds() {
    for kind in [PipelineReceiptKind::Skill, PipelineReceiptKind::Resource] {
        let (mut fake, _) = fixture(kind, true);
        let field = if kind == PipelineReceiptKind::Skill {
            "skill_reads"
        } else {
            "resource_reads"
        };
        let rule = if kind == PipelineReceiptKind::Skill {
            "WP6-SKILL-READ-01"
        } else {
            "WP6-RESOURCE-READ-01"
        };
        let mut omitted = completion_args(&fake);
        omitted["params"]["output"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        let mut explicit = omitted.clone();
        explicit["params"]["output"][field] = json!([]);
        let error = completion_error(omitted.clone(), &fake);
        assert_eq!(error.refusal().unwrap().rule.as_deref(), Some(rule));
        let explicit_error = completion_error(explicit.clone(), &fake);
        assert_eq!(error, explicit_error);
        let explicit_action = crate::pipeline_output::receipt_diff::failure_recovery(
            &explicit_error,
            &explicit["params"],
        )
        .unwrap()
        .unwrap();
        let response =
            responses::failure_bounded(error, Some(("command", &omitted)), None, 8157).unwrap();
        let envelope = json!({"jsonrpc":"2.0","id":7,"result":response});
        let failure_bytes = serde_json::to_vec(&envelope).unwrap().len();
        assert!(failure_bytes <= 8192);
        let data: Value =
            serde_json::from_str(envelope["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(data["error"]["refusal"]["rule"], rule);
        assert_eq!(data["error"]["diagnostic_delivery"]["complete"], false);
        assert_eq!(data["actions"].as_array().unwrap().len(), 1);
        let action = &data["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(
            action["arguments"]["params"]["submitted_receipts"],
            json!([])
        );
        assert_eq!(
            action["arguments"]["params"],
            explicit_action["arguments"]["params"]
        );
        let mut params = action["arguments"]["params"].clone();
        let expected = parse(params.clone())
            .unwrap()
            .resolve_receipt_diff(&fake.context, &DigestPort)
            .unwrap();
        let expected_bytes = serde_json::to_vec(&serde_json::to_value(&expected).unwrap()).unwrap();
        let before = fake.context.clone();
        let mut assembled = Vec::new();
        let mut pages = 0;
        let mut max_mcp_bytes = failure_bytes;
        loop {
            let (page, envelope) = public_read(&mut fake, params).unwrap();
            pages += 1;
            max_mcp_bytes = max_mcp_bytes.max(serde_json::to_vec(&envelope).unwrap().len());
            assert_eq!(
                page["source"]["submitted_digest"],
                expected.submitted_digest
            );
            assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
            if page["next_offset_bytes"].is_null() {
                break;
            }
            let next = &page["actions"][0];
            assert_eq!(next["kind"], "ready_call");
            params = next["arguments"]["params"].clone();
            assert_eq!(params["submitted_receipts"], json!([]));
        }
        assert!(pages > 1);
        assert_eq!(assembled, expected_bytes);
        let full: Value = serde_json::from_slice(&assembled).unwrap();
        assert_eq!(full["diff"]["missing"].as_array().unwrap().len(), 80);
        assert_eq!(full["diff"]["expected_unique_count"], 80);
        assert_eq!(full["diff"]["unexpected"], json!([]));
        assert_eq!(full["diff"]["duplicates"], json!([]));
        assert_eq!(full["diff"]["submitted_count"], 0);
        assert_eq!(full["diff"]["submitted_unique_count"], 0);
        assert_eq!(fake.context, before);
        println!(
            "default_receipt_kind={kind:?}; payload_bytes={}; pages={pages}; max_mcp_bytes={max_mcp_bytes}; failure_bytes={failure_bytes}; missing=80; unexpected=0; duplicates=0; submitted_count=0; default_equals_explicit=true",
            expected_bytes.len()
        );
    }
}

#[test]
fn explicit_null_receipts_are_rejected_and_never_replaced_with_default() {
    for kind in [PipelineReceiptKind::Skill, PipelineReceiptKind::Resource] {
        let (fake, _) = fixture(kind, true);
        let field = if kind == PipelineReceiptKind::Skill {
            "skill_reads"
        } else {
            "resource_reads"
        };
        let mut args = completion_args(&fake);
        args["params"]["output"][field] = json!([]);
        let known_error = completion_error(args.clone(), &fake);
        args["params"]["output"][field] = Value::Null;
        let decoded = crate::api::decode_public_call("command", args.clone())
            .and_then(|call| crate::pipeline_tools::parse(call.name, call.arguments));
        let error = match decoded {
            Err(error) => error,
            Ok(_) => panic!("null receipt array accepted"),
        };
        assert_eq!(
            error.refusal().unwrap().rule.as_deref(),
            Some("WP6-SCHEMA-COMPLETE-01")
        );
        assert_eq!(
            crate::pipeline_output::receipt_diff::failure_recovery(&known_error, &args["params"]),
            Err(Error::InternalInvariant)
        );
        for absent_or_invalid in [json!({}), json!({"output":null}), json!({"output":[]})] {
            let mut invalid = args["params"].clone();
            invalid.as_object_mut().unwrap().remove("output");
            if let Some(value) = absent_or_invalid.get("output") {
                invalid["output"] = value.clone();
            }
            assert_eq!(
                crate::pipeline_output::receipt_diff::failure_recovery(&known_error, &invalid)
                    .unwrap(),
                None
            );
        }
    }
}
