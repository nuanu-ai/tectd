use super::*;

fn verify_action(action: &Value) {
    assert_eq!(action["kind"], "ready_call");
    crate::api::decode_public_call(
        action["tool"].as_str().unwrap(),
        action["arguments"].clone(),
    )
    .unwrap();
}

#[test]
fn terminal_recommendations_survive_ordinary_and_fragmented_scope_candidate_reads() {
    for status in [CandidateSetStatus::Ready, CandidateSetStatus::Blocked] {
        for collection in [false, true] {
            let mut page = super::planning_read_tests::fixture();
            page.context.snapshot.method.body = "snapshot body".into();
            page.context.candidate_set.status = status;
            page.view = CandidateContextView::Reviews;
            page.next_after = collection.then_some(7);
            let params = json!({"candidate_set_id":page.context.candidate_set.id,"view":"reviews","limit":25});
            let ordinary = page_read(
                page.clone(),
                params.clone(),
                &crate::planning_read::Window::default(),
                None,
                8192,
            )
            .unwrap();
            assert!(ordinary.get("kind").is_none());
            let expected_recommendation = if collection { json!(0) } else { Value::Null };
            assert_eq!(ordinary["recommended_action"], expected_recommendation);
            assert_eq!(ordinary["actions"].as_array().unwrap().len(), 1);
            let terminal = ordinary["actions"][0].clone();
            if collection {
                verify_action(&terminal);
            } else {
                assert_eq!(terminal["kind"], "needs_input");
                assert_eq!(
                    terminal,
                    record_input_action(
                        page.context.candidate_set.id,
                        page.context.candidate_set.revision
                    )
                    .unwrap()
                );
            }
            assert_eq!(
                terminal["tool"],
                if collection { "query" } else { "command" }
            );
            assert_eq!(
                terminal["arguments"]["route"],
                if collection {
                    "scope.candidates.context"
                } else {
                    "scope.candidates.record_input"
                }
            );
            if collection {
                assert_eq!(terminal["arguments"]["params"]["after"], 7);
            }
            let expected = serde_json::to_vec(&serde_json::to_value(&page).unwrap()).unwrap();
            let mut params = params;
            let mut window = crate::planning_read::Window {
                offset_bytes: Some(0),
                limit_bytes: Some(64),
                representation_digest: None,
            };
            let mut assembled = Vec::new();
            loop {
                let output = page_read(page.clone(), params.clone(), &window, None, 8192).unwrap();
                assert!(encoded_len(&output).unwrap() <= 8192);
                assert_eq!(
                    output["offset_bytes"].as_u64().unwrap() as usize,
                    assembled.len()
                );
                assembled.extend_from_slice(output["text"].as_str().unwrap().as_bytes());
                if output["next_offset_bytes"].is_null() {
                    assert_eq!(output["actions"], ordinary["actions"]);
                    assert_eq!(output["recommended_action"], expected_recommendation);
                    assert_eq!(
                        format!("{:x}", Sha256::digest(&assembled)),
                        output["representation_digest"].as_str().unwrap()
                    );
                    break;
                }
                assert_eq!(output["actions"].as_array().unwrap().len(), 1);
                assert_eq!(output["recommended_action"], 0);
                let action = &output["actions"][0];
                verify_action(action);
                assert_eq!(action["tool"], "query");
                assert_eq!(action["arguments"]["route"], "scope.candidates.context");
                let call =
                    crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
                let crate::tools::Invocation::ScopeCandidate(
                    crate::scope_candidate_tools::ScopeCandidateInvocation::Window {
                        window: next,
                        params: selectors,
                        ..
                    },
                ) = crate::tools::parse_invocation(call.name, call.arguments).unwrap()
                else {
                    panic!("byte continuation")
                };
                params = selectors;
                window = next;
            }
            assert_eq!(assembled, expected);
        }
    }
}
