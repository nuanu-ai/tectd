use super::{create_program, draft, id, planning_ref, rows, success_ref};
use crate::recovery_support::Mcp;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

// Both directions retain the original begin receipt, including after a context-only refresh.
pub(super) async fn run(first: &mut Mcp, second: &mut Mcp, pool: &PgPool) {
    for (initial, corrected, refresh) in [("finite", "ongoing", false), ("ongoing", "finite", true)]
    {
        let program =
            create_program(first, "Inspect email preferences", "Boundary correction").await;
        let begin = json!({"request_id":Uuid::new_v4(),"program_id":program,
            "program_revision":2,"boundary":initial,"input":"Inspect email preferences"});
        let origin = first.call("begin_candidate_set", begin.clone()).await;
        let set = id(&origin["context"]["candidate_set"]["id"]);
        let mut current = origin.clone();
        if refresh {
            current = first
                .call(
                    "refresh_candidate_set",
                    json!({"candidate_set_id":set,
                "revision":1,"program_revision":2,"request_id":Uuid::new_v4()}),
                )
                .await;
            assert_eq!(current["context"]["candidate_set"]["status"], "draft");
            assert_eq!(current["context"]["candidate_set"]["revision"], 2);
        }
        let source = if corrected == "finite" {
            success_ref(&current["context"])
        } else {
            planning_ref(&current["context"], 1)
        };
        let save = json!({"kind":"draft","candidate_set_id":set,
            "revision":current["context"]["candidate_set"]["revision"],
            "snapshot_id":current["context"]["snapshot"]["id"],"input_cursor":1,
            "request_id":Uuid::new_v4(),"draft":draft(corrected,
                json!({"identity":{"local":"goal"},"text":"Inspect email preferences",
                    "source_ref_id":source,"resolution":{"kind":"candidate","reference":{"local":"candidate"}}}),
                vec![],json!({"identity":{"local":"candidate"},"title":"Inspect preferences",
                    "outcome":"Users inspect email preferences","trigger":"Open settings",
                    "delivered_behavior":"Display existing email preferences","proof":"Read API test",
                    "coverage_goals":[{"local":"goal"}]}),vec![])});
        let before = rows(pool, set).await;
        for (field, value, expected) in [
            ("revision", json!(99), "stale_revision"),
            ("snapshot_id", json!(Uuid::new_v4()), "stale_context"),
            ("input_cursor", json!(0), "input_pending"),
        ] {
            let mut bad = save.clone();
            bad[field] = value;
            bad["request_id"] = json!(Uuid::new_v4());
            let error = first.call_error("save_candidate_set", bad).await;
            assert_eq!(error["error"]["code"], expected);
            assert_eq!(rows(pool, set).await, before);
        }
        let mut invalid = save.clone();
        invalid["draft"]["candidates"][0]["title"] = json!("");
        let error = first.call_error("save_candidate_set", invalid).await;
        assert_eq!(error["error"]["code"], "invalid_arguments");
        assert_eq!(rows(pool, set).await, before);
        let unchanged = first
            .call(
                "candidate_context",
                json!({"candidate_set_id":set,"view":"overview","limit":25}),
            )
            .await;
        assert_eq!(unchanged["context"]["candidate_set"]["boundary"], initial);

        // Concurrent exact retries resolve to one corrected draft and one save receipt.
        let (saved, replay) = tokio::join!(
            first.call("save_candidate_set", save.clone()),
            second.call("save_candidate_set", save.clone())
        );
        assert_eq!(saved, replay);
        assert_eq!(saved["context"]["candidate_set"]["boundary"], corrected);
        assert_eq!(saved["draft"]["boundary"], corrected);
        let revision = current["context"]["candidate_set"]["revision"]
            .as_i64()
            .unwrap()
            + 1;
        assert_eq!(saved["context"]["candidate_set"]["revision"], revision);
        let after = rows(pool, set).await;
        assert_eq!(after.3, 1);
        assert_eq!(after.5, before.5 + 1);
        let mut conflict = save.clone();
        conflict["draft"]["boundary"] = json!(initial);
        let error = first.call_error("save_candidate_set", conflict).await;
        assert_eq!(error["error"]["code"], "input_conflict");
        assert_eq!(rows(pool, set).await, after);
        let replay = first.call("begin_candidate_set", begin.clone()).await;
        assert_eq!(replay["context"]["candidate_set"]["boundary"], initial);
        assert_eq!(replay["context"]["candidate_set"]["revision"], 1);
        let mut different = begin.clone();
        different["request_id"] = json!(Uuid::new_v4());
        let existing = first.call("begin_candidate_set", different).await;
        assert_eq!(existing["context"]["candidate_set"]["boundary"], corrected);
        assert_eq!(existing["context"]["candidate_set"]["revision"], revision);
        let mut origin_conflict = begin;
        origin_conflict["boundary"] = json!(corrected);
        let error = first
            .call_error("begin_candidate_set", origin_conflict)
            .await;
        assert_eq!(error["error"]["code"], "input_conflict");

        // A new request cannot change a boundary after even one draft was saved.
        let mut late = save;
        late["request_id"] = json!(Uuid::new_v4());
        late["revision"] = json!(revision);
        late["draft"]["boundary"] = json!(initial);
        let error = first.call_error("save_candidate_set", late.clone()).await;
        assert_eq!(error["error"]["code"], "invalid_arguments");
        assert_eq!(rows(pool, set).await, after);
        let reviewed = first.call("save_candidate_set", json!({"kind":"review",
            "candidate_set_id":set,"revision":revision,"snapshot_id":current["context"]["snapshot"]["id"],
            "input_cursor":1,"request_id":Uuid::new_v4(),"review":{"verdict":"revise",
                "summary":"Refine the proof before readiness","findings":[],
                "candidate_decisions":[{"candidate_id":saved["draft"]["candidates"][0]["id"],
                    "decision":"accept","rationale":"Useful vertical result"}]}})).await;
        late["request_id"] = json!(Uuid::new_v4());
        late["revision"] = reviewed["context"]["candidate_set"]["revision"].clone();
        let after_review = rows(pool, set).await;
        let error = first.call_error("save_candidate_set", late).await;
        assert_eq!(error["error"]["code"], "invalid_arguments");
        assert_eq!(rows(pool, set).await, after_review);
    }
}
