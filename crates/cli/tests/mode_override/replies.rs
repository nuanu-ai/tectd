//! A draft that carries no override is saved and answered exactly as it was before
//! the override existed. The two replies are held here as the daemon sent them, with
//! every id and digest replaced by its order of first appearance.
use crate::drafts::{Level, Shape, target};
use crate::support::Stand;
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

const INTRO: &str = "Current data and available calls are below.";
const SCOPE_SAVE: &str = r#"{"actions":[{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"overview"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"},{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"reviews"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"}],"candidate_set":{"boundary":"ongoing","current_snapshot_id":"<uuid 2>","id":"<uuid 1>","input_cursor":1,"latest_input":1,"program_id":"<uuid 3>","revision":2,"status":"review_required","workspace_id":"<uuid 4>"},"counts":{"candidates":2,"rules":4,"source_refs":7,"stale_reasons":0},"current_program_revision":2,"disposition":"saved","field_destinations":{"candidate_graph_contract":{"arguments":{"mode":"describe","route":"scope.candidates.save","tool":"command"},"kind":"ready_call","tool":"help"},"context_snapshot_and_knowledge":{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"overview"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"},"draft":{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"details"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"},"latest_review":{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"reviews"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"},"program":{"arguments":{"params":{"candidate_set_id":"<uuid 1>","limit":25,"view":"program"},"route":"scope.candidates.context"},"kind":"ready_call","tool":"query"}},"omitted_fields":["context.snapshot.method.body","context.snapshot.rules[].text","context.snapshot.source_refs","context.planning_knowledge","context.stale_reasons","draft","latest_review","candidate_graph_contract"],"operation":"saved","recommended_action":0,"snapshot":{"id":"<uuid 2>","method":{"digest":"<digest 1>","id":"tectd-scope-candidates","revision":"6"},"program_revision":2,"sequence":1}}"#;
const SLICE_SAVE: &str = r#"{"actions":[{"arguments":{"params":{"limit":25,"scope_id":"<uuid 1>","view":"details"},"route":"slice.candidates.context"},"kind":"ready_call","tool":"query"}],"candidate_set":{"current_snapshot_id":"<uuid 2>","id":"<uuid 3>","input_cursor":1,"latest_input":1,"revision":4,"scope_id":"<uuid 1>","status":"review_required"},"counts":{"history":2,"inputs":1,"results":0,"reviews":0,"slices":0},"field_destinations":{"complete_planning_context":{"arguments":{"params":{"limit":25,"scope_id":"<uuid 1>","view":"details"},"route":"slice.candidates.context"},"kind":"ready_call","tool":"query"}},"recommended_action":0,"scope_id":"<uuid 1>","snapshot":{"id":"<uuid 2>","sequence":2}}"#;

/// `text` with each whole JSON string that is a UUID or a 64-digit hex digest replaced
/// by its kind and the order in which that value first appears.
fn masked(text: &str) -> String {
    let mut seen = BTreeMap::<&str, String>::new();
    let (mut uuids, mut digests) = (0, 0);
    text.split('"')
        .map(|part| {
            let digest = part.len() == 64 && part.bytes().all(|byte| byte.is_ascii_hexdigit());
            let uuid = part.len() == 36 && Uuid::parse_str(part).is_ok();
            if !digest && !uuid {
                return part.to_owned();
            }
            seen.entry(part)
                .or_insert_with(|| {
                    if uuid {
                        uuids += 1;
                        format!("<uuid {uuids}>")
                    } else {
                        digests += 1;
                        format!("<digest {digests}>")
                    }
                })
                .clone()
        })
        .collect::<Vec<_>>()
        .join("\"")
}

async fn a_save_without_an_override_answers_as_before(level: Level, expected: &str) {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let target = target(&mut client, &repo, level).await;
    let params = target.save(&Shape::of(2, 2), Uuid::new_v4());
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"command","arguments":{"route":level.save_route(),"params":params}}),
        )
        .await;
    assert_ne!(response["result"]["isError"], true, "{response}");
    let content = &response["result"]["content"];
    assert_eq!(content[0]["text"], INTRO);
    let reply = masked(content[1]["text"].as_str().unwrap());
    assert_eq!(reply, expected, "\n{reply}\n");

    // What the save stored and what the agent reads back hold no trace of the field.
    let drafts = match level {
        Level::Scopes => "scope_candidate_drafts",
        Level::Slices => "slice_candidate_drafts",
    };
    let stored: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT payload::text FROM {drafts} WHERE tenant_id=$1"
    ))
    .bind(stand.tenant)
    .fetch_all(&stand.pool)
    .await
    .unwrap();
    assert!(!stored.is_empty());
    let read = target
        .stored(
            &mut client,
            &serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap(),
        )
        .await;
    assert_eq!(read.len(), 2);
    for text in stored.iter().chain(&[json!(read).to_string()]) {
        assert!(!text.contains("mode_override"), "{text}");
    }
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_scope_draft_save_without_an_override_answers_as_before() {
    a_save_without_an_override_answers_as_before(Level::Scopes, SCOPE_SAVE).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_slice_draft_save_without_an_override_answers_as_before() {
    a_save_without_an_override_answers_as_before(Level::Slices, SLICE_SAVE).await;
}
