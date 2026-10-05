use super::*;
use sha2::{Digest, Sha256};
use tect_domain::{PipelineReceiptKind, PipelineRunContextQuery, PipelineSkillReadReceipt};

struct DigestPort;
impl tect_domain::PipelineDefinitionDigestPort for DigestPort {
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }
}
struct AuthorizedFake {
    context: PipelineRunContext,
    allowed: bool,
    reads: usize,
}
impl AuthorizedFake {
    fn read(&mut self, query: &PipelineRunContextQuery) -> Result<PipelineContextResponse> {
        self.reads += 1;
        if !self.allowed {
            return Err(Error::Forbidden);
        }
        Ok(PipelineContextResponse::ReceiptDiff(Box::new(
            query.resolve_receipt_diff(&self.context, &DigestPort)?,
        )))
    }
}
fn parse(params: Value) -> Result<PipelineRunContextQuery> {
    let call = crate::api::decode_public_call(
        "query",
        json!({"route":"slice.pipeline.context","params":params}),
    )?;
    let crate::pipeline_tools::PipelineInvocation::Context(query) =
        crate::pipeline_tools::parse(call.name, call.arguments)?
    else {
        panic!("context")
    };
    Ok(query)
}
fn public_read(fake: &mut AuthorizedFake, params: Value) -> Result<(Value, Value)> {
    let query = parse(params)?;
    let read = fake.read(&query)?;
    let overhead = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":7,"result":null}))
        .unwrap()
        .len()
        - 4;
    let page = super::super::context_pinned(read, 8192 - overhead, &query)?;
    let envelope = json!({"jsonrpc":"2.0","id":7,"result":responses::success(page.clone())});
    assert!(serde_json::to_vec(&envelope).unwrap().len() <= 8192);
    Ok((page, envelope))
}
fn fixture(kind: PipelineReceiptKind, huge: bool) -> (AuthorizedFake, Value) {
    let mut current = context(PipelineKnowledgeResourceState::Current, false);
    let count = if huge { 80 } else { 1 };
    let expected = (0..count)
        .map(|i| PipelineInstructionSnapshot {
            id: format!("expected-{i}-{}", "界🙂".repeat(12)),
            version: "1".into(),
            digest: "a".repeat(64),
            body: "authorized body".into(),
            origin_refs: vec!["source".into()],
        })
        .collect();
    match kind {
        PipelineReceiptKind::Skill => current.definition.phases[0].skills = expected,
        PipelineReceiptKind::Resource => current.definition.phases[0].resources = expected,
    };
    let submitted = (0..count)
        .flat_map(|i| {
            let value = PipelineSkillReadReceipt {
                instruction_id: format!("extra-{i}-{}", "界🙂".repeat(12)),
                version: "1".into(),
                digest: "b".repeat(64),
            };
            vec![value.clone(), value.clone(), value]
        })
        .collect::<Vec<_>>();
    let params = json!({"run_id":current.run.id,"view":"receipt_diff","phase_id":"fixture-phase","receipt_kind":kind,"submitted_receipts":submitted,"limit_bytes":4096});
    (
        AuthorizedFake {
            context: current,
            allowed: true,
            reads: 0,
        },
        params,
    )
}
#[test]
fn receipt_diff_public_fake_authorized_read_reassembles_full_unicode_multisets_and_pins() {
    for kind in [PipelineReceiptKind::Skill, PipelineReceiptKind::Resource] {
        let (mut fake, mut params) = fixture(kind, true);
        let before = fake.context.clone();
        let original = params["submitted_receipts"].clone();
        let expected = parse(params.clone())
            .unwrap()
            .resolve_receipt_diff(&fake.context, &DigestPort)
            .unwrap();
        let expected_bytes = serde_json::to_vec(&serde_json::to_value(&expected).unwrap()).unwrap();
        let mut assembled = Vec::new();
        let mut pages = 0;
        let mut max_mcp_bytes = 0;
        loop {
            let (page, envelope) = public_read(&mut fake, params.clone()).unwrap();
            pages += 1;
            let mcp_bytes = serde_json::to_vec(&envelope).unwrap().len();
            max_mcp_bytes = max_mcp_bytes.max(mcp_bytes);
            assert!(mcp_bytes <= 8192);
            assert_eq!(
                page["source"]["definition_digest"],
                expected.definition_digest
            );
            assert_eq!(
                page["source"]["definition_version"],
                expected.definition_version
            );
            assert_eq!(
                page["source"]["submitted_digest"],
                expected.submitted_digest
            );
            assert_eq!(page["source"]["phase_id"], expected.phase_id);
            assert_eq!(page["source"]["receipt_kind"], json!(kind));
            assert_eq!(
                page["representation_digest"],
                format!("{:x}", Sha256::digest(&expected_bytes))
            );
            assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
            let Some(next) = page["next_offset_bytes"].as_u64() else {
                assert!(page["actions"].as_array().unwrap().is_empty());
                break;
            };
            let action = &page["actions"][0];
            assert_eq!(action["kind"], "needs_context");
            assert_eq!(action["arguments"]["route"], "slice.pipeline.context");
            assert!(
                action["arguments"]["params"]
                    .get("submitted_receipts")
                    .is_none()
            );
            assert!(!action.to_string().contains("extra-"));
            assert!(
                action["context_input"]["fields"][0]["format"]
                    .as_str()
                    .unwrap()
                    .contains(if kind == PipelineReceiptKind::Skill {
                        "output.skill_reads"
                    } else {
                        "output.resource_reads"
                    })
            );
            params = action["arguments"]["params"].clone();
            assert_eq!(params["offset_bytes"], next);
            assert!(parse(params.clone()).unwrap().validate().is_err()); // The action truthfully requires input.
            params["submitted_receipts"] = original.clone();
        }
        assert!(pages > 1);
        assert_eq!(assembled, expected_bytes);
        let full: Value = serde_json::from_slice(&assembled).unwrap();
        assert_eq!(full["diff"]["expected_unique_count"], 80);
        assert_eq!(full["diff"]["submitted_count"], 240);
        assert_eq!(full["diff"]["submitted_unique_count"], 80);
        for field in ["missing", "unexpected", "duplicates"] {
            assert_eq!(full["diff"][field].as_array().unwrap().len(), 80);
        }
        assert!(
            full["diff"]["duplicates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry["repeat_count"] == 3)
        );
        println!(
            "receipt_kind={kind:?}; payload_bytes={}; submitted_array_bytes={}; pages={pages}; max_mcp_bytes={max_mcp_bytes}; missing=80; unexpected=80; duplicates=80; submitted_count=240; submitted_unique_count=80; repeat_count=3",
            expected_bytes.len(),
            serde_json::to_vec(&original).unwrap().len()
        );
        assert_eq!(fake.context, before);
        assert_eq!(fake.reads, pages);
        assert!(fake.context.delivery_receipt.is_none());
    }
}
#[test]
fn receipt_diff_small_array_ready_continuation_and_eof_remain_exact() {
    let (mut fake, mut params) = fixture(PipelineReceiptKind::Skill, false);
    params["limit_bytes"] = json!(31);
    let original = params["submitted_receipts"].clone();
    let mut pages = 0;
    let final_page = loop {
        let (page, _) = public_read(&mut fake, params.clone()).unwrap();
        pages += 1;
        let Some(next) = page["next_offset_bytes"].as_u64() else {
            break page;
        };
        let action = &page["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(
            action["arguments"]["params"]["submitted_receipts"],
            original
        );
        params = action["arguments"]["params"].clone();
        assert_eq!(params["offset_bytes"], next);
    };
    assert!(pages > 1);
    params["offset_bytes"] = final_page["total_bytes"].clone();
    params["representation_digest"] = final_page["representation_digest"].clone();
    let (eof, _) = public_read(&mut fake, params).unwrap();
    assert_eq!(eof["text"], "");
    assert!(eof["next_offset_bytes"].is_null());
}
#[test]
fn receipt_diff_forged_definition_submitted_representation_and_interpage_access_refuse() {
    let (mut fake, params) = fixture(PipelineReceiptKind::Skill, true);
    let (first, _) = public_read(&mut fake, params.clone()).unwrap();
    let mut next = first["actions"][0]["arguments"]["params"].clone();
    next["submitted_receipts"] = params["submitted_receipts"].clone();
    for (field, bad, path) in [
        (
            "definition_digest",
            "wrong",
            "arguments.params.definition_digest",
        ),
        (
            "submitted_digest",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "arguments.params.submitted_digest",
        ),
        (
            "representation_digest",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "arguments.params.representation_digest",
        ),
    ] {
        let mut forged = next.clone();
        forged[field] = json!(bad);
        let error = public_read(&mut fake, forged).unwrap_err();
        assert_eq!(error.refusal().unwrap().path.as_deref(), Some(path));
    }
    let mut changed = next.clone();
    changed["submitted_receipts"][0]["digest"] = json!("changed");
    assert_eq!(
        public_read(&mut fake, changed)
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("PIPELINE-RECEIPT-DIFF-SUBMITTED-PIN")
    );
    let calls = fake.reads;
    fake.allowed = false;
    assert_eq!(public_read(&mut fake, next).unwrap_err(), Error::Forbidden);
    assert_eq!(fake.reads, calls + 1);
}
#[test]
fn receipt_only_raw_keys_refuse_on_every_other_view_even_null_or_empty() {
    for view in [
        None,
        Some("current"),
        Some("output"),
        Some("delivery_receipt"),
        Some("snapshot"),
        Some("phase_contract"),
        Some("details"),
    ] {
        for (field, value) in [
            ("receipt_kind", Value::Null),
            ("submitted_receipts", Value::Null),
            ("submitted_receipts", json!([])),
            ("submitted_digest", Value::Null),
            ("submitted_digest", json!("")),
        ] {
            let mut params = json!({"run_id":uuid::Uuid::new_v4()});
            if let Some(view) = view {
                params["view"] = json!(view)
            }
            params[field] = value;
            let error = parse(params).unwrap_err();
            let refusal = error.refusal().unwrap();
            assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
            assert_eq!(
                refusal.path.as_deref(),
                Some(format!("arguments.params.{field}").as_str())
            );
        }
    }
    let (mut fake, mut params) = fixture(PipelineReceiptKind::Resource, false);
    params["submitted_receipts"] = json!([]);
    public_read(&mut fake, params).unwrap();
}

#[test]
fn receipt_diff_continuation_readiness_threshold_is_inclusive_and_never_limits_input() {
    for (bytes, expected_kind) in [(1024, "ready_call"), (1025, "needs_context")] {
        let (mut fake, mut params) = fixture(PipelineReceiptKind::Skill, false);
        let mut submitted = json!([{"instruction_id":"","version":"1","digest":"d"}]);
        let overhead = serde_json::to_vec(&submitted).unwrap().len();
        submitted[0]["instruction_id"] = json!("x".repeat(bytes - overhead));
        assert_eq!(serde_json::to_vec(&submitted).unwrap().len(), bytes);
        params["submitted_receipts"] = submitted.clone();
        params["limit_bytes"] = json!(1);
        let (page, _) = public_read(&mut fake, params).unwrap();
        assert_eq!(page["actions"][0]["kind"], expected_kind);
        println!("submitted_array_bytes={bytes}; continuation_kind={expected_kind}");
        if bytes == 1024 {
            assert_eq!(
                page["actions"][0]["arguments"]["params"]["submitted_receipts"],
                submitted
            )
        } else {
            assert!(
                page["actions"][0]["arguments"]["params"]
                    .get("submitted_receipts")
                    .is_none()
            )
        }
    }
}
#[test]
fn actual_completion_receipt_failures_recover_full_diff_without_availability_redirect_or_array_echo()
 {
    for kind in [PipelineReceiptKind::Skill, PipelineReceiptKind::Resource] {
        for huge in [false, true] {
            let (fake, query_params) = fixture(kind, huge);
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
            let mut params = json!({"request_id":uuid::Uuid::new_v4(),"run_id":fake.context.run.id,"run_revision":fake.context.run.revision,
                "phase_id":"fixture-phase","outcome":"completed","transition":"continue","output":{"body":"body","producer_context_id":"ctx","skill_reads":[],"resource_reads":[]}});
            params["output"][field] = query_params["submitted_receipts"].clone();
            let args = json!({"route":"slice.pipeline.phase.complete","params":params});
            let call = crate::api::decode_public_call("command", args.clone()).unwrap();
            let crate::pipeline_tools::PipelineInvocation::Complete(request) =
                crate::pipeline_tools::parse(call.name, call.arguments).unwrap()
            else {
                panic!("complete")
            };
            let error = request.validate(&fake.context.definition).unwrap_err();
            assert_eq!(error.refusal().unwrap().rule.as_deref(), Some(rule));
            let original = serde_json::to_value(error.refusal().unwrap()).unwrap();
            let response =
                responses::failure_bounded(error, Some(("command", &args)), None, 8157).unwrap();
            let envelope = json!({"jsonrpc":"2.0","id":7,"result":response});
            assert!(serde_json::to_vec(&envelope).unwrap().len() <= 8192);
            let data: Value =
                serde_json::from_str(envelope["result"]["content"][1]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(data["error"]["refusal"]["rule"], rule);
            assert_eq!(
                data["error"]["refusal"]["path"],
                format!("arguments.params.output.{field}")
            );
            let actions = data["actions"].as_array().unwrap();
            assert_eq!(actions.len(), 1);
            let recovery = &actions[0];
            assert_eq!(recovery["arguments"]["params"]["view"], "receipt_diff");
            assert_eq!(recovery["arguments"]["params"]["receipt_kind"], json!(kind));
            assert_eq!(recovery["arguments"]["params"]["phase_id"], "fixture-phase");
            if huge {
                assert_eq!(data["error"]["diagnostic_delivery"]["complete"], false);
                assert_eq!(recovery["kind"], "needs_context");
                assert!(
                    recovery["arguments"]["params"]
                        .get("submitted_receipts")
                        .is_none()
                );
                assert!(!recovery.to_string().contains("extra-"));
            } else {
                assert_eq!(data["error"]["refusal"], original);
                assert_eq!(recovery["kind"], "ready_call");
                assert_eq!(
                    recovery["arguments"]["params"]["submitted_receipts"],
                    query_params["submitted_receipts"]
                );
            }
            let mut read_params = recovery["arguments"]["params"].clone();
            if huge {
                read_params["submitted_receipts"] = query_params["submitted_receipts"].clone()
            };
            let read = parse(read_params)
                .unwrap()
                .resolve_receipt_diff(&fake.context, &DigestPort)
                .unwrap();
            assert_eq!(
                read.submitted_digest,
                recovery["arguments"]["params"]["submitted_digest"]
                    .as_str()
                    .unwrap()
            );
            assert_eq!(read.diff.submitted_count, if huge { 240 } else { 3 });
            println!(
                "failure_rule={rule}; huge={huge}; max_frame_bytes={}; recovery_kind={}; diagnostic_omitted={}",
                serde_json::to_vec(&envelope).unwrap().len(),
                recovery["kind"],
                data["error"].get("diagnostic_delivery").is_some()
            );
        }
    }
}

#[path = "receipt_diff/defaults.rs"]
mod defaults;
