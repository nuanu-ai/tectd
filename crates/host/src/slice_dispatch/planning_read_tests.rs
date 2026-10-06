use super::*;
use tect_domain::{
    CandidateBoundary, CandidateMethodSnapshot, NativeScope, OpenScopeContext,
    PipelineCatalogueSnapshot, ResolvedSliceCandidateDraft, SliceCandidateSet,
    SlicePlanningSnapshot,
};

fn fixture() -> SliceCandidateContext {
    let scope_id = Uuid::new_v4();
    let set_id = Uuid::new_v4();
    let snapshot_id = Uuid::new_v4();
    let scope = NativeScope {
        id: scope_id,
        workspace_id: Uuid::new_v4(),
        revision: 1,
        source_candidate_set_id: Uuid::new_v4(),
        source_candidate_set_revision: 3,
        source_snapshot_id: Uuid::new_v4(),
        source_candidate_id: Uuid::new_v4(),
        source_candidate_revision: 1,
        boundary: CandidateBoundary::Finite,
        title: "Scope".repeat(10000),
        outcome: "Observed".into(),
        includes: vec!["included".into()],
        excludes: vec![],
        slice_candidate_set_id: set_id,
        slice_input_cursor: 0,
        slice_latest_input: 0,
    };
    SliceCandidateContext {
        scope,
        candidate_set: SliceCandidateSet {
            id: set_id,
            scope_id,
            revision: 1,
            status: SliceCandidateSetStatus::Draft,
            current_snapshot_id: snapshot_id,
            input_cursor: 0,
            latest_input: 0,
        },
        snapshot: SlicePlanningSnapshot {
            id: snapshot_id,
            sequence: 1,
            scope_revision: 1,
            source_candidate_set_revision: 3,
            source_snapshot_id: Uuid::new_v4(),
            planning_latest_input: 0,
            method: CandidateMethodSnapshot {
                id: "method".into(),
                revision: "1".into(),
                digest: "digest".into(),
                body: "🙂\\\"\n".repeat(10000),
                origin_refs: vec!["test".into()],
            },
            registry_revision: "1".into(),
            registry_digest: "d".into(),
            rules: vec![],
            catalogue: PipelineCatalogueSnapshot {
                revision: "4".into(),
                digest: "d".into(),
                entries: vec![],
            },
            result_ids: vec![],
        },
        draft: Some(ResolvedSliceCandidateDraft {
            coverage_summary: "nonempty draft".into(),
            nodes: vec![],
            supersessions: vec![],
        }),
        reviews: vec![],
        inputs: vec![],
        history: vec![],
        slices: vec![],
        results: vec![],
        checkpoints: vec![],
        stale_reasons: vec![],
        planning_knowledge: None,
    }
}

#[test]
fn scope_open_created_and_replay_use_same_compact_guard_projection() {
    let context = fixture();
    for outcome in [
        OpenScopeOutcome::Created(OpenScopeContext {
            scope: context.scope.clone(),
            planning: context.clone(),
        }),
        OpenScopeOutcome::Replay(OpenScopeContext {
            scope: context.scope.clone(),
            planning: context.clone(),
        }),
    ] {
        let output = compact_open_scope(&outcome).unwrap();
        assert!(responses::encoded_len(&output).unwrap() <= 8192);
        NativePlanningEncoding { capacity: 8192 }
            .check_open_scope(&outcome)
            .unwrap();
        println!(
            "scope.open[{}] envelope_bytes={}",
            output["disposition"],
            responses::encoded_len(&output).unwrap()
        );
        assert_eq!(output["actions"].as_array().unwrap().len(), 2);
        assert_eq!(
            output["actions"][1]["arguments"]["params"]["view"],
            "details"
        );
        for action in output["actions"].as_array().unwrap() {
            crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        }
        assert!(output["scope"].get("title").is_none());
        assert_eq!(
            output["field_destinations"]["scope"]["arguments"]["params"]["scope_id"],
            context.scope.id.to_string()
        );
        assert!(
            NativePlanningEncoding { capacity: 1 }
                .check_open_scope(&outcome)
                .is_err()
        );
    }
}

#[test]
fn complete_slice_planning_details_reassemble_with_nonempty_draft_and_pin_every_change() {
    let context = fixture();
    let query = SliceCandidateContextQuery {
        scope_id: context.scope.id,
        view: SliceCandidateContextView::Details,
        after: None,
        limit: 25,
    };
    let expected = serde_json::to_vec(&serde_json::to_value(&context).unwrap()).unwrap();
    let mut params = serde_json::to_value(&query).unwrap();
    assert!(params.get("after").is_none());
    for after in [0, 3] {
        let mut paged = query.clone();
        paged.after = Some(after);
        assert_eq!(serde_json::to_value(&paged).unwrap()["after"], after);
    }
    let mut window = crate::planning_read::Window::default();
    let mut assembled = Vec::new();
    let mut maximum = 0;
    let digest = loop {
        let output =
            candidate_page_read(context.clone(), &query, params.clone(), &window, 8192).unwrap();
        let bytes = responses::encoded_len(&output).unwrap();
        maximum = maximum.max(bytes);
        assert!(bytes <= 8192);
        assert_eq!(
            output["offset_bytes"].as_u64().unwrap() as usize,
            assembled.len()
        );
        assembled.extend_from_slice(output["text"].as_str().unwrap().as_bytes());
        if output["next_offset_bytes"].is_null() {
            break Some(output["representation_digest"].as_str().unwrap().to_owned());
        }
        let action = &output["actions"][0];
        assert_eq!(action["tool"], "query");
        assert_eq!(action["arguments"]["route"], "slice.candidates.context");
        assert_eq!(
            action["arguments"]["params"]["scope_id"],
            query.scope_id.to_string()
        );
        assert!(action["arguments"]["params"].get("after").is_none());
        assert_eq!(
            action["arguments"]["params"]["offset_bytes"],
            output["next_offset_bytes"]
        );
        assert_eq!(
            action["arguments"]["params"]["representation_digest"],
            output["representation_digest"]
        );
        let call = crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        let crate::tools::Invocation::Slice(SliceInvocation::Window {
            window: next,
            params: selectors,
            ..
        }) = crate::tools::parse_invocation(call.name, call.arguments).unwrap()
        else {
            panic!("real read continuation")
        };
        params = selectors;
        window = next;
    };
    assert!(assembled == expected);
    println!("slice.candidates.context[details] max_envelope_bytes={maximum}");
    assert!(
        serde_json::from_slice::<Value>(&assembled).unwrap()["draft"]["coverage_summary"]
            == "nonempty draft"
    );
    let pinned = crate::planning_read::Window {
        offset_bytes: Some(1),
        limit_bytes: Some(256),
        representation_digest: digest,
    };
    let mut changed = context.clone();
    changed.inputs.push(tect_domain::SlicePlanningInput {
        id: Uuid::new_v4(),
        sequence: 1,
        source_result_id: None,
        input: "new original".into(),
    });
    assert_eq!(
        changed.candidate_set.revision,
        context.candidate_set.revision
    );
    assert!(
        candidate_page_read(changed, &query, params, &pinned, 8192)
            .unwrap_err()
            .refusal()
            .is_some()
    );
}

#[test]
fn full_scope_read_reassembles_through_real_window_parser() {
    let scope = fixture().scope;
    let expected = serde_json::to_vec(&serde_json::to_value(&scope).unwrap()).unwrap();
    let mut params = json!({"scope_id":scope.id});
    let mut window = crate::planning_read::Window::default();
    let mut assembled = Vec::new();
    let mut maximum = 0;
    loop {
        let value = scope_read(scope.clone(), params.clone(), &window, 8192).unwrap();
        let size = responses::encoded_len(&value).unwrap();
        maximum = maximum.max(size);
        assert!(size <= 8192);
        assembled.extend_from_slice(value["text"].as_str().unwrap().as_bytes());
        if value["next_offset_bytes"].is_null() {
            break;
        }
        let call =
            crate::api::decode_public_call("query", value["actions"][0]["arguments"].clone())
                .unwrap();
        let crate::tools::Invocation::Slice(SliceInvocation::Window {
            window: next,
            params: selectors,
            ..
        }) = crate::tools::parse_invocation(call.name, call.arguments).unwrap()
        else {
            panic!("scope continuation")
        };
        window = next;
        params = selectors;
    }
    assert!(assembled == expected);
    println!("scope.context max_envelope_bytes={maximum}");
}
