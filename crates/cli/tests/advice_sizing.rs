//! The `sizing` advice point on `scope.candidates.save` and `slice.candidates.save`,
//! through a real daemon and MCP bridge against PostgreSQL. Jev is a loopback fake;
//! nothing leaves the machine.
#[path = "advice_sizing/disabled.rs"]
mod disabled;
#[path = "advice_sizing/drafts.rs"]
mod drafts;
#[path = "advice_sizing/live.rs"]
mod live;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_sizing/support.rs"]
mod support;
#[path = "advice_sizing/unavailable.rs"]
mod unavailable;

use drafts::{Level, Shape, target};
use serde_json::{Value, json};
use support::{BUDGET, Jev, KEY, Stand, audit, command, configure};
use tect_domain::SIZING_NOTE;
use uuid::Uuid;

/// The first candidate is large, to be split, and its second item is extra; Jev
/// abstains on what to do with every other candidate.
fn mixed(name: &str) -> (&'static str, f64) {
    match name {
        "c1.size" => ("large", 0.6249),
        "c1.advice" => ("split", 1.0),
        "c1.i2" => ("extra", 0.7),
        other if other.ends_with(".size") => ("bounded", 0.55),
        other if other.ends_with(".advice") => ("ABSTAIN", 0.5),
        _ => ("requested", 0.9),
    }
}

fn abstaining(_: &str) -> (&'static str, f64) {
    ("ABSTAIN", 0.9)
}

async fn advised_then_replayed_then_abstained(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let target = target(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(mixed));

    // Two candidates with two included items each; at the Slice level a decision
    // node stands between them, so the second work node is draft position 3.
    let mut shape = Shape::of(2, 2);
    shape.decision = level == Level::Slices;
    let second = if shape.decision { 3 } else { 2 };
    let request_id = Uuid::new_v4();
    let params = target.save(&shape, request_id);
    let (saved, envelope) = command(&mut client, level.save_route(), params.clone()).await;
    let advice = saved["advice"].clone();
    assert_eq!(
        advice,
        json!({"status":"advised","note":SIZING_NOTE,"level":level.name(),"candidates":[
            {"candidate":1,"size":"large","size_p":0.62,"advice":"split","advice_p":1.0,
             "extra_items":[2]},
            {"candidate":second,"size":"bounded","size_p":0.55,"advice":"ABSTAIN",
             "advice_p":0.5,"extra_items":[]}
        ]})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    assert_eq!(target.stored(&mut client, &saved).await.len(), second);

    // What Jev was sent: the user's words by source and in order, and the draft.
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 1);
    let state = &requests[0]["state"];
    assert_eq!(state["level"], level.name());
    let word = |source: &str, text: &str| json!({"source":source,"text":text});
    let mut asked = vec!["c1.advice", "c1.i1", "c1.i2", "c1.size"];
    if level == Level::Scopes {
        assert_eq!(
            state["user_words"],
            json!([
                word("program_input", drafts::PROGRAM_WORDS),
                word("scope_candidate_input", drafts::SCOPE_WORDS)
            ])
        );
        assert!(state.get("scope").is_none());
        assert_eq!(
            state["goals"],
            json!([{"text":"Goal 1","quote":drafts::QUOTE},{"text":"Goal 2"}])
        );
        assert_eq!(
            state["candidates"],
            json!([
                {"key":"c1","title":"Scope 1","outcome":"The cause is demonstrated",
                 "includes":[{"key":"c1.i1","text":"item 1"},{"key":"c1.i2","text":"item 2"}],
                 "excludes":["deployment"],"dependencies":[]},
                {"key":"c2","title":"Scope 2","outcome":"The cause is demonstrated",
                 "includes":[{"key":"c2.i1","text":"item 1"},{"key":"c2.i2","text":"item 2"}],
                 "excludes":["deployment"],"dependencies":["c1"]}
            ])
        );
        asked.extend(["c2.advice", "c2.i1", "c2.i2", "c2.size"]);
        assert!(requests[0]["questions"]["c2.advice"]["criteria"]["separate_scope"].is_null());
    } else {
        assert_eq!(
            state["user_words"],
            json!([
                word("program_input", drafts::PROGRAM_WORDS),
                word("scope_candidate_input", drafts::OPENED_SCOPE_WORDS),
                word("slice_candidate_input", drafts::SLICE_WORDS)
            ])
        );
        assert_eq!(
            state["scope"],
            json!({"title":"Preview diagnosis and correction decision",
                "outcome":"The cause is demonstrated and the correction path selected",
                "includes":["diagnosis","decision"],"excludes":["deployment"]})
        );
        assert_eq!(state["goals"], json!([]));
        assert_eq!(
            state["candidates"][1],
            json!({"key":"c2","kind":"decision","title":"Choose correction path",
                "question":"Is Lightweight sufficient?","dependencies":["c1"]})
        );
        assert_eq!(state["candidates"][2]["key"], "c3");
        assert_eq!(state["candidates"][2]["kind"], "work");
        asked.extend(["c3.advice", "c3.i1", "c3.i2", "c3.size"]);
        assert!(requests[0]["questions"]["c3.advice"]["criteria"]["separate_scope"].is_string());
    }
    assert_eq!(
        requests[0]["questions"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        asked
    );

    // The journal row holds exactly the field the agent received, and the work it was for.
    let row = stand.newest_row().await;
    assert_eq!(row["advice"], advice);
    assert_eq!(row["advice"].to_string(), advice.to_string());
    assert_eq!(row["request"], requests[0]);
    assert_eq!(row["point"], "sizing");
    assert_eq!(row["route"], level.save_route());
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], target.program_id);
    assert_eq!(row["candidate_set_id"], target.set()["id"]);
    let scope_id = match level {
        Level::Scopes => Value::Null,
        Level::Slices => target.context["scope"]["id"].clone(),
    };
    assert_eq!(row["scope_id"], scope_id);
    assert_eq!(
        (&row["called"], &row["outcome"], &row["send_certainty"]),
        (&json!(true), &json!("advised"), &json!("sent"))
    );
    assert_eq!(row["model"], "jev-2026-10");
    assert_eq!(
        (&row["input_tokens"], &row["output_tokens"]),
        (&json!(640), &json!(90))
    );
    assert!(!row.to_string().contains(KEY));
    let page = audit(&mut client).await;
    let entry = page["entries"][0].as_object().unwrap();
    assert_eq!(entry["advice"], advice);
    assert!(!entry.contains_key("request_body") && !entry.contains_key("response_body"));
    assert!(entry["request_bytes"].as_u64().unwrap() > 0);

    // A replay under the same request id returns the same reply: no call, no row.
    for _ in 0..2 {
        let (replayed, _) = command(&mut client, level.save_route(), params.clone()).await;
        assert_eq!(replayed, saved);
    }
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 1);

    // Jev abstains on every question of a draft in another workspace: no content.
    stand.jev.answer(Jev::Answers(abstaining));
    let (mut other, repo) = stand.workspace().await;
    let target = drafts::target(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    let params = target.save(&Shape::of(1, 1), Uuid::new_v4());
    let (saved, _) = command(&mut other, level.save_route(), params).await;
    assert_eq!(saved["advice"], json!({"status":"abstained"}));
    assert_eq!(stand.jev.calls(), 2);
    let row = stand.newest_row().await;
    assert_eq!(row["advice"], saved["advice"]);
    assert_eq!(
        (&row["called"], &row["outcome"]),
        (&json!(true), &json!("abstained"))
    );
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_draft_save_carries_shaped_advice_that_the_journal_holds() {
    advised_then_replayed_then_abstained(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_draft_save_carries_shaped_advice_that_the_journal_holds() {
    advised_then_replayed_then_abstained(Level::Slices).await;
}
