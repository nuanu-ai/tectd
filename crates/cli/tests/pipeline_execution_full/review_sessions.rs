use super::*;
use std::path::{Path, PathBuf};

pub(super) struct ReviewSessions {
    socket: PathBuf,
    config: PathBuf,
    key: String,
    worktree_id: Value,
    actors: Vec<Uuid>,
}

fn actor(state: &Value) -> Uuid {
    let actor = Uuid::parse_str(state["session"]["id"].as_str().unwrap()).unwrap();
    assert!(!actor.is_nil());
    actor
}

impl ReviewSessions {
    pub(super) async fn new(producer: &mut Mcp, socket: &Path, config: &Path, key: &str) -> Self {
        let state = producer.call("get_state", json!({})).await;
        let worktree_id = state["selected_worktrees"][0]["id"].clone();
        assert!(!worktree_id.is_null());
        Self {
            socket: socket.to_owned(),
            config: config.to_owned(),
            key: key.to_owned(),
            worktree_id,
            actors: vec![actor(&state)],
        }
    }

    pub(super) async fn advance(&mut self, context: ResolvedPipeline) -> ResolvedPipeline {
        let (verdict, outcome, transition) = successful_route(&context);
        let request = completion(&context, verdict, outcome, transition, None, None);
        self.complete(context, request).await
    }

    pub(super) async fn complete(
        &mut self,
        context: ResolvedPipeline,
        request: Value,
    ) -> ResolvedPipeline {
        let phase = context.current_phase().unwrap();
        assert_eq!(phase["fresh_reviewer_input"], true);
        assert!(matches!(
            context.run()["current_phase_ordinal"].as_u64(),
            Some(6 | 11)
        ));
        let mut reviewer = Mcp::start(
            &self.socket,
            &self.config,
            &Uuid::new_v4().to_string(),
            &self.key,
        )
        .await;
        reviewer.call("open_workspace", json!({})).await;
        reviewer
            .call(
                "select_worktrees",
                json!({"worktree_ids":[self.worktree_id]}),
            )
            .await;
        let state = reviewer.call("get_state", json!({})).await;
        let reviewer_actor = actor(&state);
        assert!(
            !self.actors.contains(&reviewer_actor),
            "reviewer must differ from all fixture actors"
        );
        self.actors.push(reviewer_actor);
        let raw = route(
            &mut reviewer,
            "query",
            "slice.pipeline.context",
            json!({"run_id":context.run()["id"],"refresh":true}),
        )
        .await;
        let observed = resolve_pipeline(&mut reviewer, raw).await.unwrap();
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
        let response = reviewer
            .exchange(
                "tools/call",
                public_call(
                    "command",
                    json!({"route":"slice.pipeline.phase.complete","params":request.clone()}),
                ),
            )
            .await;
        assert_ne!(
            response["result"]["isError"],
            true,
            "phase={} request={} response={}",
            context.run()["current_phase_id"],
            request,
            response
        );
        let result = resolve_pipeline(&mut reviewer, tool_payload(&response))
            .await
            .unwrap();
        assert!(mutation_result_id(&result).is_null());
        reviewer.finish().await;
        result
    }
}
