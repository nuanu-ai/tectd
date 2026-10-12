//! The override of one work node on `slice.candidates.save`: kept when it quotes an
//! input the user gave to the Scope's Slice candidate set, refused in every other case.
use crate::planning::{open_slice, review, route};
use crate::recovery_support::Mcp;
use crate::recovery_support::candidate_reads::read_query_json;
use crate::support::Stand;
use crate::walk::{
    PERMISSION, QUOTE, ScopeSet, SliceSet, assert_unpermitted, refused, retained, reviewed,
    scope_candidate, slice_draft, work_node,
};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

const SAVE: &str = "slice.candidates.save";
/// What the refusal tells the agent to do, in the order it has to be done.
const STEPS: [&str; 7] = [
    "Ask the user",
    "slice.candidates.input",
    "slice.candidates.refresh",
    "slice.candidates.context view inputs",
    "save again",
    "input_id",
    "exact_quote",
];

fn overridden(number: usize, mode: &str, input_id: &Value, quote: &str) -> Value {
    let mut node = work_node(number);
    node["mode_override"] = json!({"mode":mode,"input_id":input_id,"exact_quote":quote});
    node
}

/// Two Scopes opened from two accepted candidates of one Program, and the source
/// references of the Scope candidate set they were opened from.
async fn two_scopes(client: &mut Mcp, repo: &Path) -> (SliceSet, SliceSet, Vec<Value>) {
    let begun = ScopeSet::begun(client, repo).await;
    let (ready, candidates, refs) =
        reviewed(client, begun, vec![scope_candidate(1), scope_candidate(2)]).await;
    let first = SliceSet::opened(client, &ready, &candidates[0]).await;
    let second = SliceSet::opened(client, &ready, &candidates[1]).await;
    (first, second, refs)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_override_quoting_a_recorded_input_is_stored_and_read_back() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let (planning, _, _) = two_scopes(&mut client, &repo).await;
    let head = planning.record_and_refresh(&mut client, PERMISSION).await;
    let inputs = head.items(&mut client, "inputs").await;
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0]["input"], PERMISSION);
    let input_id = inputs[0]["id"].clone();

    // A decision node between two work nodes; only the first work node is overridden.
    let decision = json!({"kind":"decision","identity":{"local":"decision"},
        "title":"Choose correction path","question":"Is Lightweight sufficient?",
        "resolution_criteria":["Diagnosis identifies the boundary"],
        "dependencies":[{"local":"work-1"}],"source_result_ids":[]});
    let draft = slice_draft(vec![
        overridden(1, "demo", &input_id, QUOTE),
        decision,
        work_node(2),
    ]);
    let saved = route(&mut client, "command", SAVE, head.save(draft)).await;
    // The override does not depend on Jev: no advice field, no journal row.
    assert!(saved.get("advice").is_none(), "{saved}");
    assert_eq!(stand.count("advice_journal").await, 0);

    let head = SliceSet::after(&mut client, saved).await;
    let kept = json!({"mode":"demo","input_id":input_id,"exact_quote":QUOTE});
    let listed = head.items(&mut client, "candidates").await;
    let details = head.context["draft"]["nodes"].as_array().unwrap();
    for nodes in [&listed, details] {
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0]["mode_override"], kept);
        assert!(nodes[1].get("mode_override").is_none());
        assert!(nodes[2].get("mode_override").is_none());
    }
    let stored: Value = sqlx::query_scalar(
        "SELECT payload FROM slice_candidate_drafts WHERE tenant_id=$1 \
         ORDER BY set_revision DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert_eq!(stored["nodes"][0]["mode_override"], kept);
    assert!(stored["nodes"][2].get("mode_override").is_none());
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_override_without_the_users_quoted_permission_is_refused_and_stores_nothing() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let (planning, other, scope_refs) = two_scopes(&mut client, &repo).await;
    let head = planning.record_and_refresh(&mut client, PERMISSION).await;
    let own = head.items(&mut client, "inputs").await[0]["id"].clone();
    // The other Scope's Slice candidate set holds an input with the very same words.
    let other = other.record_and_refresh(&mut client, PERMISSION).await;
    let foreign = other.items(&mut client, "inputs").await[0]["id"].clone();
    let receipts = stand.count("native_planning_receipts").await;

    let level = "/params/draft/nodes/0/mode_override/mode";
    let quote = "/params/draft/nodes/0/mode_override/exact_quote";
    let input = "/params/draft/nodes/0/mode_override/input_id";
    let cases = [
        // A quote that is not in the input, one that is not its exact wording, a blank one.
        (overridden(1, "demo", &own, "I allow prod"), "QUOTE", quote),
        (overridden(1, "demo", &own, "i allow demo"), "QUOTE", quote),
        (overridden(1, "demo", &own, "  "), "QUOTE", quote),
        // An input of another Scope's Slice candidate set, quoted exactly.
        (overridden(1, "demo", &foreign, QUOTE), "INPUT", input),
        // An input of the Scope candidate set the Scope was opened from, by its
        // source reference: each level takes its own inputs only.
        (
            overridden(1, "demo", &scope_refs[0], "preview diagnosis"),
            "INPUT",
            input,
        ),
        // An id nothing has.
        (
            overridden(1, "demo", &json!(Uuid::new_v4()), QUOTE),
            "INPUT",
            input,
        ),
        // A level that is not one of the four.
        (overridden(1, "enterprise", &own, QUOTE), "LEVEL", level),
    ];
    for (node, rule, path) in cases {
        let draft = slice_draft(vec![node, work_node(2)]);
        let refusal = refused(&mut client, SAVE, head.save(draft)).await;
        assert_unpermitted(
            &refusal,
            &format!("SLICE-DRAFT-MODE-OVERRIDE-{rule}"),
            path,
            &STEPS,
        );
    }

    // An override is stated whole or left out, a Slice node names an input and not a
    // source reference, and a decision node takes none: the schema refuses the rest.
    let whole = json!({"mode":"demo","input_id":own,"exact_quote":QUOTE});
    let mut decision = json!({"kind":"decision","identity":{"local":"decision"},
        "title":"Choose","question":"Which?","resolution_criteria":["One is chosen"],
        "dependencies":[],"source_result_ids":[]});
    decision["mode_override"] = whole;
    let broken = |stated: Value| {
        let mut node = work_node(1);
        node["mode_override"] = stated;
        node
    };
    for node in [
        decision,
        broken(Value::Null),
        broken(json!({"mode":"demo","input_id":own})),
        broken(json!({"mode":"demo","source_ref_id":own,"exact_quote":QUOTE})),
    ] {
        let draft = slice_draft(vec![node, work_node(2)]);
        let (error, _) = refused(&mut client, SAVE, head.save(draft)).await;
        assert_eq!(error["code"], "invalid_arguments", "{error}");
    }
    assert_eq!(stand.count("slice_candidate_drafts").await, 0);
    assert_eq!(stand.count("native_planning_receipts").await, receipts);

    // One Slice is opened and its result recorded: the set now holds an input that
    // a result produced. Its text contains the summary, and it is not permission.
    let saved = route(
        &mut client,
        "command",
        SAVE,
        head.save(slice_draft(vec![work_node(1)])),
    )
    .await;
    let saved = SliceSet::after(&mut client, saved).await;
    let ready = review(&mut client, &saved.context).await;
    let opened_node = ready["draft"]["nodes"][0].clone();
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&ready, &opened_node, Uuid::new_v4()),
    )
    .await;
    let summary = "The preview reads a stale settings cache";
    route(
        &mut client,
        "command",
        "slice.result.record",
        json!({"request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
            "slice_id":opened["created"]["id"],"slice_revision":opened["created"]["revision"],
            "outcome":"completed","summary":summary,
            "evidence":[{"kind":"test","reference":"fixture","observation":"Persisted"}],
            "scope_impact":"The correction can be chosen","remaining_work":"Choose it"}),
    )
    .await;
    let stale = read_query_json(
        &mut client,
        &json!({"route":"slice.candidates.context","params":{
            "scope_id":ready["scope"]["id"],"view":"details","limit":25}}),
    )
    .await
    .value;
    let head = SliceSet { context: stale }.refresh(&mut client).await;
    let inputs = head.items(&mut client, "inputs").await;
    assert_eq!(inputs.len(), 2);
    assert!(inputs[1]["source_result_id"].is_string());
    assert!(inputs[1]["input"].as_str().unwrap().ends_with(summary));
    let drafts = stand.count("slice_candidate_drafts").await;
    let next = |cited: &Value, quote: &str| {
        slice_draft(vec![
            retained(&opened_node),
            overridden(2, "demo", cited, quote),
        ])
    };
    let refusal = refused(
        &mut client,
        SAVE,
        head.save(next(&inputs[1]["id"], summary)),
    )
    .await;
    assert_unpermitted(
        &refusal,
        "SLICE-DRAFT-MODE-OVERRIDE-INPUT",
        "/params/draft/nodes/1/mode_override/input_id",
        &STEPS,
    );
    assert_eq!(stand.count("slice_candidate_drafts").await, drafts);

    // The user's own input of the same set is still good for the same node.
    let saved = route(&mut client, "command", SAVE, head.save(next(&own, QUOTE))).await;
    let head = SliceSet::after(&mut client, saved).await;
    assert_eq!(
        head.context["draft"]["nodes"][1]["mode_override"],
        json!({"mode":"demo","input_id":own,"exact_quote":QUOTE})
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn asking_recording_refreshing_and_quoting_succeeds_after_a_refusal() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let (head, _, scope_refs) = two_scopes(&mut client, &repo).await;
    assert!(head.items(&mut client, "inputs").await.is_empty());

    // The agent decides for itself; the set holds no input of the user to cite.
    let draft = |cited: &Value| slice_draft(vec![overridden(1, "demo", cited, QUOTE)]);
    let refusal = refused(&mut client, SAVE, head.save(draft(&scope_refs[0]))).await;
    assert_unpermitted(
        &refusal,
        "SLICE-DRAFT-MODE-OVERRIDE-INPUT",
        "/params/draft/nodes/0/mode_override/input_id",
        &STEPS,
    );

    // Record the user's answer. The input has its id at once, but the set is stale
    // and a save is refused until the refresh.
    let recorded = head.record(&mut client, PERMISSION).await;
    let unrefreshed = SliceSet::after(&mut client, recorded).await;
    let input_id = unrefreshed.items(&mut client, "inputs").await[0]["id"].clone();
    let (error, _) = refused(&mut client, SAVE, unrefreshed.save(draft(&input_id))).await;
    assert_eq!(error["code"], "stale_context", "{error}");

    // Refresh, read the answer's id in view inputs, save again quoting it.
    let head = unrefreshed.refresh(&mut client).await;
    let inputs = head.items(&mut client, "inputs").await;
    assert_eq!(inputs[0]["id"], input_id);
    let saved = route(&mut client, "command", SAVE, head.save(draft(&input_id))).await;
    let head = SliceSet::after(&mut client, saved).await;
    assert_eq!(
        head.context["draft"]["nodes"][0]["mode_override"],
        json!({"mode":"demo","input_id":input_id,"exact_quote":QUOTE})
    );

    // Saved again unchanged, the node keeps its revision: the input's id is stable.
    let node = head.context["draft"]["nodes"][0].clone();
    let mut same = retained(&node);
    same["mode_override"] = node["mode_override"].clone();
    let saved = route(
        &mut client,
        "command",
        SAVE,
        head.save(slice_draft(vec![same])),
    )
    .await;
    let head = SliceSet::after(&mut client, saved).await;
    assert_eq!(head.context["draft"]["nodes"][0]["revision"], 1);
    client.finish().await;
}
