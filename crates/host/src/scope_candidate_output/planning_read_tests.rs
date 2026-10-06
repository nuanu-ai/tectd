use super::*;
#[test]
fn complete_scope_candidate_snapshot_reassembles_with_real_parser() {
    let mut page = fixture();
    let id = page.context.candidate_set.id;
    assert!(page.draft.is_none());
    assert!(serde_json::to_value(&page).unwrap().get("draft").is_none());
    page.view = CandidateContextView::Details;
    page.draft = Some(serde_json::from_value(json!({
        "boundary":"finite","goals":[],"evidence":[],"candidates":[],"blockers":[],
        "pending_question":"質問🙂\\\"\n".repeat(2000),
        "empty_disposition":{"kind":"needs_input","reason":"理由🙂".repeat(2000),"source_ref_id":id},
        "protected_changes":[{"accepted_evidence_id":id,"prior_candidate_id":id,"disposition":"replace","rationale":"変更🙂".repeat(2000),"authority_source_ref_id":id,"replacement_evidence_id":id,"target_candidate_id":id}],
        "delta":{"added":[],"changed":[{"candidate_id":id,"from_revision":1,"to_revision":2,"rationale":"差分🙂".repeat(2000)}],"unchanged":[],"superseded":[]}
    })).unwrap());
    let manifest_body = "独立した知識🙂\\\"\n".repeat(2000);
    page.context.planning_knowledge = Some(serde_json::from_value(json!({
        "manifest":{"id":id,"digest":"manifest-digest","stage":"scope","owner_id":id,"owner_revision":2,"input_revision":0,"request_id":id,"policy_id":"policy","policy_version":"1","task_context_digest":"context-digest","task_context":{},"workspace_generation":1,
        "needs":{"stage":"scope","roles":[],"expected_abstraction":"test","policy_id":"policy","policy_version":"1","method":{"id":"knowledge-method","version":"1","digest":"method-digest","body":manifest_body,"origin_refs":["test"]}},"selected":[],"unresolved_needs":[]}
    })).unwrap());
    let expected = serde_json::to_vec(&serde_json::to_value(&page).unwrap()).unwrap();
    use tect_application::CandidateOutputGuard;
    let guard = crate::scope_guidance::CandidateEncoding { capacity: 8192 };
    let outcome = BeginCandidateSetOutcome::Created(page.context.clone());
    let begun = begin(outcome.clone(), 8192).unwrap();
    guard.check_begin(&outcome).unwrap();
    assert!(encoded_len(&begun).unwrap() <= 8192);
    let stored = StoredCandidateContext {
        context: page.context.clone(),
        program: tect_domain::Program::draft(id, id, 0),
        draft: page.draft.clone(),
        reviews: vec![],
    };
    let saved = stored_mutation(stored.clone(), 8192).unwrap();
    let refreshed = super::stored(stored.clone(), 8192).unwrap();
    assert_eq!(refreshed["operation"], "refreshed");
    assert_eq!(
        refreshed["field_destinations"]["draft"]["arguments"]["params"]["view"],
        "details"
    );
    assert!(encoded_len(&refreshed).unwrap() <= 8192);
    assert_eq!(
        crate::scope_guidance::CandidateEncoding { capacity: 1 }.check_stored(&stored),
        Err(Error::RequestTooLarge)
    );
    guard.check_stored(&stored).unwrap();
    assert!(encoded_len(&saved).unwrap() <= 8192);
    println!(
        "scope.candidates.begin envelope_bytes={}; scope.candidates.save envelope_bytes={}; scope.candidates.refresh envelope_bytes={}",
        encoded_len(&begun).unwrap(),
        encoded_len(&saved).unwrap(),
        encoded_len(&refreshed).unwrap()
    );
    let mut params = json!({"candidate_set_id":id,"view":"details","limit":25});
    let mut window = crate::planning_read::Window::default();
    let mut assembled = Vec::new();
    let mut maximum = 0;
    loop {
        let output = page_read(page.clone(), params.clone(), &window, None, 8192).unwrap();
        let size = encoded_len(&output).unwrap();
        maximum = maximum.max(size);
        assert!(size <= 8192);
        assembled.extend_from_slice(output["text"].as_str().unwrap().as_bytes());
        if output["next_offset_bytes"].is_null() {
            break;
        }
        let call =
            crate::api::decode_public_call("query", output["actions"][0]["arguments"].clone())
                .unwrap();
        let crate::tools::Invocation::ScopeCandidate(
            crate::scope_candidate_tools::ScopeCandidateInvocation::Window {
                window: next,
                params: selectors,
                ..
            },
        ) = crate::tools::parse_invocation(call.name, call.arguments).unwrap()
        else {
            panic!("real scope candidate continuation")
        };
        window = next;
        params = selectors;
    }
    assert!(assembled == expected);
    let reconstructed: CandidateContextPage = serde_json::from_slice(&assembled).unwrap();
    assert_eq!(reconstructed.draft, page.draft);
    assert_eq!(
        reconstructed.context.snapshot.method.body,
        page.context.snapshot.method.body
    );
    assert_eq!(
        reconstructed
            .context
            .planning_knowledge
            .as_ref()
            .unwrap()
            .manifest
            .as_ref()
            .unwrap()
            .needs
            .method
            .body,
        manifest_body
    );
    let mut changed_manifest = page.clone();
    changed_manifest
        .context
        .planning_knowledge
        .as_mut()
        .unwrap()
        .manifest
        .as_mut()
        .unwrap()
        .needs
        .method
        .body
        .push_str("変更");
    assert!(
        page_read(changed_manifest, params.clone(), &window, None, 8192)
            .unwrap_err()
            .refusal()
            .is_some()
    );

    let mut changed = page.clone();
    changed.draft.as_mut().unwrap().pending_question = Some("new question".into());
    assert!(
        page_read(changed, params.clone(), &window, None, 8192)
            .unwrap_err()
            .refusal()
            .is_some()
    );
    println!("scope.candidates.context max_envelope_bytes={maximum}");
    assert!(
        page_read(page, params, &window, Some(999), 8192)
            .unwrap_err()
            .refusal()
            .is_some()
    );
}

pub(super) fn fixture() -> CandidateContextPage {
    let id = Uuid::new_v4();
    let page: CandidateContextPage=serde_json::from_value(json!({"context":{"candidate_set":{"id":id,"workspace_id":id,"program_id":id,"revision":2,"status":"draft","boundary":"finite","current_snapshot_id":id,"input_cursor":0,"latest_input":0},"snapshot":{"id":id,"sequence":1,"program_revision":1,"program_latest_input":0,"planning_latest_input":0,"selected_worktree_ids":[],"selected_sources_digest":"d","method":{"id":"m","revision":"1","digest":"d","body":"🙂\\\"\n".repeat(4000),"origin_refs":["test"]},"registry_revision":"1","registry_digest":"d","rules":[],"source_refs":[]},"current_program_revision":1,"stale_reasons":[]},"view":"overview","program":null,"historical":null,"items":[],"next_after":null,"required_protected_changes":[],"terminal_note":null})).unwrap();
    page
}
