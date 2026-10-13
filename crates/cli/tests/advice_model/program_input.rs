//! The `model` point on `program.record_input`, where it is the only point: an input
//! recorded on the Program itself gets `model_advice` and no `advice`, and Jev reads
//! the Program's statement, its product level, the words recorded for the Program
//! before the input, and the input.
use crate::drafts::PROGRAM_WORDS;
use crate::inputs::{FIRST, SECOND, asked, criteria, statement, word};
use crate::model::{advised, audit, journalled, points, rows, settled};
use crate::planning::ready_program;
use crate::recovery_support::Mcp;
use crate::support::{BUDGET, Jev, KEY, Stand, command, configure};
use crate::{silent_on_executors, specialist_only};
use serde_json::{Value, json};
use std::path::Path;
use tect_domain::{MODEL_NOTE, ProgramMode};
use uuid::Uuid;

pub const ROUTE: &str = "program.record_input";

/// A Program that takes new input.
pub struct Stated {
    pub id: Value,
}

impl Stated {
    /// An Open Program stated in full at `prod`, holding the one input it was begun
    /// with. Jev plays no part up to here.
    pub async fn ready(client: &mut Mcp, repo: &Path) -> Self {
        let program = ready_program(client, repo).await;
        assert_eq!(program["status"], "open", "{program}");
        Self {
            id: program["id"].clone(),
        }
    }

    /// The record-input call for `text` under a request id.
    pub fn input(&self, text: &str, request_id: Uuid) -> Value {
        json!({"program_id":self.id,"request_id":request_id,"input":text})
    }

    /// Every input text the store holds for the Program, in recorded order.
    pub async fn recorded(&self, stand: &Stand) -> Vec<String> {
        let id = Uuid::parse_str(self.id.as_str().unwrap()).unwrap();
        sqlx::query_scalar(
            "SELECT input FROM program_inputs WHERE tenant_id=$1 AND program_id=$2 \
             ORDER BY sequence",
        )
        .bind(stand.tenant)
        .bind(id)
        .fetch_all(&stand.pool)
        .await
        .unwrap()
    }
}

/// The whole `state` Jev is sent for `new_input` recorded on the fixture's Program
/// after these earlier inputs of it.
pub fn state(mode: ProgramMode, earlier: &[&str], new_input: &str) -> Value {
    let words = earlier
        .iter()
        .map(|text| word("program_input", text))
        .collect::<Vec<_>>();
    json!({"level":"input_to_program",
        "mode":{"level":mode.as_str(),"description":mode.description()},
        "program":statement(),"user_words":words,"new_input":new_input})
}

/// The one row a recorded Program input wrote: that of `model`.
pub async fn the_row(stand: &Stand, request_id: Uuid) -> Value {
    let mut written = rows(stand, request_id).await;
    assert_eq!(points(&written), ["model"]);
    written.remove(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_carries_model_advice_alone_and_the_journal_holds_its_one_row() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let program = Stated::ready(&mut client, &repo).await;
    configure(&mut client, true, 0).await;

    let request_id = Uuid::new_v4();
    let params = program.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, ROUTE, params.clone()).await;
    // The one point of the command gave its field; `intake` has none here.
    assert_eq!(recorded["model_advice"], advised());
    assert!(recorded.get("advice").is_none(), "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    println!("{ROUTE}: reply envelope bytes with model_advice={envelope}, of {BUDGET}");
    assert_eq!(recorded["operation"], "updated");
    assert_eq!(recorded["program"]["id"], program.id);
    assert_eq!(recorded["program"]["current_step"], "compose");
    assert_eq!(recorded["program"]["latest_input"], 2);
    assert_eq!(program.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    // Kinds of work and probabilities only: no text of the input, no name of a model.
    let field = recorded["model_advice"].to_string();
    for absent in ["preview", "ignores", "jev-latest", "jev-2026-10"] {
        assert!(!field.contains(absent), "{absent}");
    }

    // Jev was asked once, the two questions of the candidate levels, over the
    // Program-level state: no Scope, no candidates, and the new input once.
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(asked(&requests[0]), ["practical", "specialist"]);
    let sent = &requests[0]["state"];
    assert_eq!(*sent, state(ProgramMode::Prod, &[PROGRAM_WORDS], FIRST));
    assert_eq!(sent.as_object().unwrap().len(), 5);
    assert!(sent.get("scope").is_none() && sent.get("candidates").is_none());
    assert_eq!(requests[0].to_string().matches(FIRST).count(), 1);
    assert_eq!(
        criteria(&requests[0], "practical"),
        ["ABSTAIN", "implementation", "none", "routine"]
    );
    assert_eq!(
        criteria(&requests[0], "specialist"),
        ["ABSTAIN", "demanding", "none", "standard"]
    );
    for question in ["practical", "specialist"] {
        let instructions = requests[0]["questions"][question]["instructions"]
            .as_str()
            .unwrap();
        assert!(
            instructions.contains("choose the more capable one"),
            "{question}"
        );
    }

    // One journal row for the request: the point, the route of the command, exactly
    // the field the agent received, the request as it was sent, and the Program.
    let row = the_row(&stand, request_id).await;
    assert_eq!(row["advice"], recorded["model_advice"]);
    assert_eq!(
        row["advice"].to_string(),
        recorded["model_advice"].to_string()
    );
    assert_eq!(row["request"], requests[0]);
    assert_eq!(row["point"], "model");
    assert_eq!(row["route"], ROUTE);
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], program.id);
    for absent in ["candidate_set_id", "scope_id", "slice_id", "phase"] {
        assert_eq!(row[absent], Value::Null, "{absent}");
    }
    assert_eq!(
        journalled(&row),
        (
            &json!(true),
            &json!("sent"),
            &json!("advised"),
            &Value::Null
        )
    );
    assert_eq!(row["model"], "jev-2026-10");
    assert_eq!(
        (&row["input_tokens"], &row["output_tokens"]),
        (&json!(640), &json!(90))
    );
    assert!(!row.to_string().contains(KEY));
    // No review is held to this point: nothing is kept of its answers.
    assert_eq!(stand.count("advice_candidate_answers").await, 0);
    // The audit route shows the row under its point and its route.
    let page = audit(&mut client).await;
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["advice"], recorded["model_advice"]);
    assert_eq!(page["entries"][0]["route"], ROUTE);
    assert_eq!(page["entries"][0]["program_id"], program.id);
    assert_eq!(
        (&page["counters"]["called"], &page["counters"]["advised"]),
        (&json!(1), &json!(1))
    );

    // A replay under the same request id returns the same reply, byte for byte: no
    // call, no row, and the input is not recorded twice.
    for _ in 0..2 {
        let (replayed, bytes) = command(&mut client, ROUTE, params.clone()).await;
        assert_eq!(replayed, recorded);
        assert_eq!(replayed.to_string(), recorded.to_string());
        assert_eq!(bytes, envelope);
    }
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 1);
    assert_eq!(program.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);

    // The next input: the earlier words now hold the first one, in recorded order,
    // and the new input stands in `new_input` alone. A question Jev abstains on
    // keeps `ABSTAIN`.
    stand.jev.answer(Jev::Answers(specialist_only));
    let second_id = Uuid::new_v4();
    let (second, _) = command(&mut client, ROUTE, program.input(SECOND, second_id)).await;
    assert_eq!(
        second["model_advice"],
        json!({"status":"advised","note":MODEL_NOTE,"practical":"ABSTAIN","practical_p":0.6,
            "specialist":"demanding","specialist_p":0.9})
    );
    assert!(second.get("advice").is_none(), "{second}");
    assert_eq!(second["program"]["latest_input"], 3);
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["state"],
        state(ProgramMode::Prod, &[PROGRAM_WORDS, FIRST], SECOND)
    );
    for once in [PROGRAM_WORDS, FIRST, SECOND] {
        assert_eq!(requests[1].to_string().matches(once).count(), 1, "{once}");
    }
    let row = the_row(&stand, second_id).await;
    assert_eq!(row["advice"], second["model_advice"]);
    assert_eq!(row["request"], requests[1]);
    assert_eq!(
        program.recorded(&stand).await,
        [PROGRAM_WORDS, FIRST, SECOND]
    );

    // A replay of the first input, now that a later one is recorded, still returns
    // the field stored for it and asks nobody.
    let (replayed, _) = command(&mut client, ROUTE, params).await;
    assert_eq!(replayed["model_advice"], recorded["model_advice"]);
    assert_eq!(replayed["program"]["latest_input"], 3);
    assert_eq!(stand.jev.calls(), 2);
    assert_eq!(stand.count("advice_journal").await, 2);

    // Jev abstains on both questions: no content.
    stand.jev.answer(Jev::Answers(silent_on_executors));
    let third_id = Uuid::new_v4();
    let third = "Nothing more for now.";
    let (silent, _) = command(&mut client, ROUTE, program.input(third, third_id)).await;
    assert_eq!(silent["model_advice"], json!({"status":"abstained"}));
    let row = the_row(&stand, third_id).await;
    assert_eq!(row["advice"], silent["model_advice"]);
    assert_eq!(
        journalled(&row),
        (
            &json!(true),
            &json!("sent"),
            &json!("abstained"),
            &Value::Null
        )
    );
    assert_eq!(stand.jev.calls(), 3);
    client.finish().await;
}

/// Disabled means the reply without the feature: with no setting, or with a setting
/// that says disabled, the command answers without the field, writes no advice row
/// and calls nobody.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_with_jev_disabled_carries_no_field_and_writes_no_row() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let program = Stated::ready(&mut client, &repo).await;
    let params = program.input(FIRST, Uuid::new_v4());

    // No setting row, then a setting that says disabled: the same reply, byte for
    // byte, without the field, and nothing written or asked.
    let (plain, plain_bytes) = command(&mut client, ROUTE, params.clone()).await;
    assert!(plain.get("advice").is_none() && plain.get("model_advice").is_none());
    assert_eq!(stand.count("advice_config").await, 0);
    assert_eq!(program.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    configure(&mut client, false, 0).await;
    let (disabled, disabled_bytes) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(disabled, plain);
    assert_eq!(disabled.to_string(), plain.to_string());
    assert_eq!(disabled_bytes, plain_bytes);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);

    // Enabled: the same reply with exactly the one field more.
    configure(&mut client, true, 1).await;
    let (enabled, _) = command(&mut client, ROUTE, params.clone()).await;
    let mut without = enabled.clone();
    let fields = without.as_object_mut().unwrap();
    assert_eq!(fields.remove("model_advice").unwrap(), advised());
    assert_eq!(without, plain);
    assert_eq!(without.to_string(), plain.to_string());
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (1, 1)
    );

    // Disabled again: the stored field is not shown and nothing more is written.
    configure(&mut client, false, 2).await;
    let (again, again_bytes) = command(&mut client, ROUTE, params).await;
    assert_eq!(again.to_string(), plain.to_string());
    assert_eq!(again_bytes, plain_bytes);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (1, 1)
    );
    assert_eq!(program.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);

    // A first input, not a replay, in a workspace whose setting says disabled.
    let (mut other, repo) = stand.workspace().await;
    let fresh = Stated::ready(&mut other, &repo).await;
    configure(&mut other, false, 0).await;
    let params = fresh.input(FIRST, Uuid::new_v4());
    let (recorded, recorded_bytes) = command(&mut other, ROUTE, params).await;
    assert!(recorded.get("advice").is_none() && recorded.get("model_advice").is_none());
    assert_eq!(
        recorded.as_object().unwrap().keys().collect::<Vec<_>>(),
        plain.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    // The two Programs differ in their ids and in nothing that has a length.
    assert_eq!(recorded_bytes, plain_bytes);
    println!("{ROUTE}: reply envelope bytes with Jev disabled={plain_bytes}");
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (1, 1)
    );
    assert_eq!(fresh.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    client.finish().await;
    other.finish().await;
}

/// A corner that is not of this command alone. The journal finds a replay by
/// workspace, route, point and request id, and a request id is unique per Program,
/// not per workspace. An agent that reuses one request id on another Program of the
/// workspace has its input recorded there, and gets the field stored for the first
/// Program, with no call.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_request_id_reused_on_another_program_gets_the_field_stored_for_the_first() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let first = Stated::ready(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let (recorded, _) = command(&mut client, ROUTE, first.input(FIRST, request_id)).await;
    assert_eq!(recorded["model_advice"], advised());

    // Another Program of the same workspace, just begun: it has no mode, so an input
    // of its own would get `mode_not_set`.
    let other_words = "Plan the export of saved previews.";
    let begun = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":other_words}),
        )
        .await;
    let second = Stated {
        id: begun["program"]["id"].clone(),
    };
    assert_ne!(second.id, first.id);
    let (reused, _) = command(&mut client, ROUTE, second.input(SECOND, request_id)).await;
    assert_eq!(reused["program"]["id"], second.id);
    assert_eq!(second.recorded(&stand).await, [other_words, SECOND]);
    assert_eq!(reused["model_advice"], recorded["model_advice"]);
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 1);
    client.finish().await;
}
