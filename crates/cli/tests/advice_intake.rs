//! The `intake` advice point on `scope.candidates.record_input` and
//! `slice.candidates.input`, through a real daemon and MCP bridge against PostgreSQL.
//! Jev is a loopback fake; nothing leaves the machine. The `model` point asks on the
//! same commands after `intake`: this suite reads the requests and the rows of `intake`.
#[path = "mode_override/advised.rs"]
#[allow(dead_code)]
mod advised;
#[path = "advice_intake/disabled.rs"]
mod disabled;
#[path = "advice_sizing/drafts.rs"]
#[allow(dead_code)]
mod drafts;
#[path = "advice_intake/inputs.rs"]
mod inputs;
#[path = "advice_intake/live.rs"]
mod live;
#[path = "advice_intake/no_candidates.rs"]
mod no_candidates;
#[path = "advice_intake/overrides.rs"]
mod overrides;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_intake/result_inputs.rs"]
mod result_inputs;
#[path = "advice_sizing/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "advice_intake/unavailable.rs"]
mod unavailable;
#[path = "mode_override/walk.rs"]
#[allow(dead_code)]
mod walk;

use drafts::{Level, Shape};
use inputs::{FIRST, Planned, SECOND, asked, audit, criteria, route, word};
use serde_json::{Value, json};
use support::{BUDGET, Jev, KEY, Stand, command, configure};
use tect_domain::{INTAKE_NOTE, ProgramMode};
use uuid::Uuid;

/// Bounded new work that extends the candidate Jev names and is in line.
fn extending(name: &str, nearest: &'static str) -> (&'static str, f64) {
    match name {
        "work" => ("bounded", 0.6249),
        "placement" => ("extends", 1.0),
        "nearest" => (nearest, 0.805),
        "fit" => ("in_line", 0.9),
        // The `model` point asks about the same input; this suite is about `intake`.
        "practical" | "specialist" => ("ABSTAIN", 0.9),
        other => panic!("unexpected question {other}"),
    }
}

fn extending_the_second(name: &str) -> (&'static str, f64) {
    extending(name, "c2")
}

fn extending_the_third(name: &str) -> (&'static str, f64) {
    extending(name, "c3")
}

fn abstaining(_: &str) -> (&'static str, f64) {
    ("ABSTAIN", 0.9)
}

/// The saved draft of two candidates as Jev reads it. At the Slice level a decision
/// node stands between the two work nodes.
fn candidates(level: Level) -> Value {
    let work = |key: &str, title: &str, dependencies: Value| {
        json!({"key":key,"title":title,"outcome":"The cause is demonstrated",
            "includes":[{"key":format!("{key}.i1"),"text":"item 1"},
                {"key":format!("{key}.i2"),"text":"item 2"}],
            "excludes":["deployment"],"dependencies":dependencies})
    };
    match level {
        Level::Scopes => json!([
            work("c1", "Scope 1", json!([])),
            work("c2", "Scope 2", json!(["c1"]))
        ]),
        Level::Slices => {
            let mut nodes = [
                work("c1", "Work 1", json!([])),
                work("c3", "Work 2", json!([])),
            ];
            for node in &mut nodes {
                node["kind"] = json!("work");
            }
            let [first, last] = nodes;
            json!([first, {"key":"c2","kind":"decision","title":"Choose correction path",
                "question":"Is Lightweight sufficient?","dependencies":["c1"]}, last])
        }
    }
}

async fn advised_then_replayed_then_abstained(level: Level) {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    // Two candidates with two included items each; at the Slice level a decision node
    // stands between them, so the last work node is draft position 3.
    let mut shape = Shape::of(2, 2);
    shape.decision = level == Level::Slices;
    let mut planned = Planned::drafted(&mut client, &repo, level, &shape).await;
    configure(&mut client, true, 0).await;
    let last = if shape.decision { 3 } else { 2 };
    stand.jev.answer(Jev::Answers(match level {
        Level::Scopes => extending_the_second,
        Level::Slices => extending_the_third,
    }));

    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let before = planned.set()["revision"].as_i64().unwrap();
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    let advice = recorded["advice"].clone();
    assert_eq!(
        advice,
        json!({"status":"advised","note":INTAKE_NOTE,"level":level.name(),
            "work":"bounded","work_p":0.62,"placement":"extends","placement_p":1.0,
            "nearest":last,"nearest_p":0.81,"fit":"in_line","fit_p":0.9})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["candidate_set"]["revision"], before + 1);
    assert_eq!(recorded["candidate_set"]["status"], "review_required");

    // What Jev was sent, whole: the product level with its meaning, the Program's
    // statement, the Scope on the Slice level, the earlier words by source and in
    // order, the new input on its own, and the saved draft.
    let requests = stand.jev.asked("fit");
    assert_eq!(requests.len(), 1);
    let words = planned.first_words();
    assert_eq!(
        requests[0]["state"],
        planned.state(ProgramMode::Prod, &words, FIRST, Some(candidates(level)))
    );
    assert_eq!(asked(&requests[0]), ["fit", "nearest", "placement", "work"]);
    assert_eq!(
        criteria(&requests[0], "work"),
        ["ABSTAIN", "bounded", "large", "none", "oversized", "small"]
    );
    assert_eq!(
        criteria(&requests[0], "fit"),
        [
            "ABSTAIN",
            "beyond_mode",
            "contradicts",
            "in_line",
            "unclear"
        ]
    );
    let mut placement = vec![
        "ABSTAIN",
        "covered",
        "extends",
        "new_candidate",
        "outside_program",
    ];
    // The decision node is shown to Jev and is not one of the candidates to name.
    let named = match level {
        Level::Scopes => ["ABSTAIN", "c1", "c2", "none"],
        Level::Slices => {
            placement.push("separate_scope");
            ["ABSTAIN", "c1", "c3", "none"]
        }
    };
    assert_eq!(criteria(&requests[0], "placement"), placement);
    assert_eq!(criteria(&requests[0], "nearest"), named);

    // The journal row holds exactly the field the agent received, and the work it was for.
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["advice"], advice);
    assert_eq!(row["advice"].to_string(), advice.to_string());
    assert_eq!(row["request"], requests[0]);
    assert_eq!(row["point"], "intake");
    assert_eq!(row["route"], route(level));
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], planned.target.program_id);
    assert_eq!(row["candidate_set_id"], planned.set()["id"]);
    assert_eq!(row["scope_id"], planned.scope_id());
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

    // A replay under the same request id returns the same reply: no call, no row,
    // and the input is not recorded twice. One call and one row are of `model`.
    for _ in 0..2 {
        let (replayed, _) = command(&mut client, route(level), params.clone()).await;
        assert_eq!(replayed, recorded);
    }
    assert_eq!((stand.jev.calls(), stand.jev.asked("fit").len()), (2, 1));
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.rows_of("intake").await, (1, 1));
    let first_word = planned.first_words().pop().unwrap()["text"].clone();
    assert_eq!(
        planned.recorded(&stand).await,
        [first_word.as_str().unwrap(), FIRST]
    );

    // The next input reads the first one among the earlier words, last and under the
    // set's own source, and not itself.
    planned.follow(&recorded);
    let params = planned.input(SECOND, Uuid::new_v4());
    let (next, _) = command(&mut client, route(level), params).await;
    assert_eq!(next["advice"], advice);
    let mut words = planned.first_words();
    words.push(word(planned.own_source(), FIRST));
    assert_eq!(
        stand.jev.asked("fit")[1]["state"],
        planned.state(ProgramMode::Prod, &words, SECOND, Some(candidates(level)))
    );
    assert_eq!(stand.rows_of("intake").await, (2, 2));
    assert_eq!(planned.recorded(&stand).await.len(), 3);

    // Jev abstains on every question about an input in another workspace: no content.
    // Its Program is a demo, and that is the level this request carries.
    stand.jev.answer(Jev::Answers(abstaining));
    let (mut other, repo) = stand.workspace().await;
    let planned = Planned::drafted(&mut other, &repo, level, &Shape::of(1, 1)).await;
    stand
        .set_mode(&planned.target.program_id, Some("demo"))
        .await;
    configure(&mut other, true, 0).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(recorded["advice"], json!({"status":"abstained"}));
    assert_eq!(stand.jev.asked("fit").len(), 3);
    assert_eq!(
        stand.jev.asked("fit")[2]["state"]["mode"],
        json!({"level":"demo","description":ProgramMode::Demo.description()})
    );
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["advice"], recorded["advice"]);
    assert_eq!(
        (&row["called"], &row["outcome"], &row["point"]),
        (&json!(true), &json!("abstained"), &json!("intake"))
    );
    assert_eq!(planned.recorded(&stand).await.len(), 2);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_carries_shaped_advice_that_the_journal_holds() {
    advised_then_replayed_then_abstained(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_carries_shaped_advice_that_the_journal_holds() {
    advised_then_replayed_then_abstained(Level::Slices).await;
}
