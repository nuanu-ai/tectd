use super::*;
use tect_domain::PipelineInstructionQuery;

#[test]
fn phase_instruction_continuations_preserve_mode_and_lossless_resolved_pins() {
    let mut context = context(PipelineKnowledgeResourceState::Current, false);
    context.definition.phases[0].instructions[0].body = "正文🙂\\\"\n".repeat(4000);
    let params = json!({"run_id":context.run.id,"phase_id":context.definition.phases[0].id});
    let mut query: PipelineInstructionQuery = serde_json::from_value(params).unwrap();
    let expected = query.resolve(&context).unwrap();
    let mut assembled = Vec::new();
    let mut max = 0;
    loop {
        let resolved = query.resolve(&context).unwrap();
        let page = instruction_pinned(resolved, 8192, &query).unwrap();
        let bytes = responses::encoded_len(&page).unwrap();
        max = max.max(bytes);
        assert!(bytes <= 8192);
        assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
        if page["next_offset_bytes"].is_null() {
            break;
        }
        let action = &page["actions"][0];
        let decoded = crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        let crate::tools::Invocation::Pipeline(
            crate::pipeline_tools::PipelineInvocation::Instruction(next),
        ) = crate::tools::parse_invocation(decoded.name, decoded.arguments).unwrap()
        else {
            panic!("instruction read")
        };
        assert_eq!(next.phase_id, query.phase_id);
        assert!(next.instruction_id.is_none());
        assert!(next.version.is_none());
        assert!(next.digest.is_none());
        query = next;
    }
    assert_eq!(
        assembled,
        serde_json::to_vec(&serde_json::to_value(&expected).unwrap()).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<PipelineInstructionResponse>(&assembled)
            .unwrap()
            .instruction
            .digest,
        expected.instruction.digest
    );
    let mut changed = expected;
    changed.instruction.body.push_str("changed");
    assert!(
        instruction_pinned(changed, 8192, &query)
            .unwrap_err()
            .refusal()
            .is_some()
    );
    println!("phase instruction max_full_mcp_bytes={max}");
}
