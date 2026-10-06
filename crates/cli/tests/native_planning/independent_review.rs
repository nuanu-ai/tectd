use super::*;
use std::path::Path;

pub(super) async fn complete(
    producer: &mut Mcp,
    context: ResolvedPipeline,
    paths: (&Path, &Path),
    workspace_key: &str,
    worktree_id: &Value,
    fixture_actors: &mut Vec<Value>,
) -> ResolvedPipeline {
    let phase = context.current_phase().unwrap();
    assert_eq!(phase["fresh_reviewer_input"], true);
    assert!(matches!(
        context.run()["current_phase_ordinal"].as_u64(),
        Some(6 | 11)
    ));
    let (v, o, t) = successful_route(&context);
    let refused = refuses_with_code_without_persistence(
        producer,
        &context,
        completion(&context, v, o, t, None, None),
        "INVALID_OUTPUT",
    )
    .await;
    assert_eq!(refused["error"]["code"], "INVALID_OUTPUT");
    for (field, expected) in [
        ("rule", "WP6-REVIEW-INDEPENDENCE-01"),
        ("path", "arguments.params.output.reviewer_context"),
        (
            "expected",
            "a native reviewer actor different from every current producer actor",
        ),
        (
            "actual",
            "current authenticated session produced at least one output",
        ),
        (
            "next_action",
            "use_another_native_session_for_independent_review",
        ),
        ("required", "independent_native_reviewer"),
    ] {
        assert_eq!(refused["error"]["refusal"][field], expected);
    }
    let mut reviewer =
        Mcp::start(paths.0, paths.1, &Uuid::new_v4().to_string(), workspace_key).await;
    reviewer.call("open_workspace", json!({})).await;
    reviewer
        .call("select_worktrees", json!({"worktree_ids":[worktree_id]}))
        .await;
    let state = reviewer.call("get_state", json!({})).await;
    let actor = state["session"]["id"].clone();
    assert!(
        actor
            .as_str()
            .is_some_and(|id| Uuid::parse_str(id).is_ok_and(|id| !id.is_nil()))
    );
    assert!(
        !fixture_actors.contains(&actor),
        "reviewer must differ from all fixture actors"
    );
    fixture_actors.push(actor);
    let observed = current(&mut reviewer, &context).await;
    for key in [
        "id",
        "revision",
        "current_phase_id",
        "status",
        "definition_digest",
    ] {
        assert_eq!(observed.run()[key], context.run()[key]);
    }
    for key in ["outputs", "bindings"] {
        assert_eq!(observed.details_data()[key], context.details_data()[key]);
    }
    assert_eq!(observed.current_phase().unwrap(), phase);
    let (v, o, t) = successful_route(&observed);
    let request = completion(&observed, v, o, t, None, None);
    let result = route(
        &mut reviewer,
        "command",
        "slice.pipeline.phase.complete",
        request,
    )
    .await;
    let result = resolve_pipeline(&mut reviewer, result).await.unwrap();
    reviewer.finish().await;
    result
}
