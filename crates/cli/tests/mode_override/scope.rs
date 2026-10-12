//! The override of one Scope candidate on `scope.candidates.save`: kept when it
//! quotes an input the user gave to the candidate set, refused in every other case.
use crate::support::Stand;
use crate::walk::{
    PERMISSION, QUOTE, ScopeSet, assert_unpermitted, refused, scope_candidate, scope_draft,
    scope_resave,
};
use serde_json::{Value, json};
use uuid::Uuid;

const SAVE: &str = "scope.candidates.save";
/// What the refusal tells the agent to do, in the order it has to be done.
const STEPS: [&str; 6] = [
    "Ask the user",
    "scope.candidates.record_input",
    "scope.candidates.refresh",
    "scope.candidates.context view inputs",
    "save again",
    "exact_quote",
];

fn overridden(number: usize, mode: &str, source_ref: &Value, quote: &str) -> Value {
    let mut candidate = scope_candidate(number);
    candidate["mode_override"] =
        json!({"mode":mode,"source_ref_id":source_ref,"exact_quote":quote});
    candidate
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_override_quoting_a_recorded_input_is_stored_and_read_back() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let head = ScopeSet::begun(&mut client, &repo)
        .await
        .record_and_refresh(&mut client, PERMISSION)
        .await;
    let refs = head.input_refs(&mut client).await;
    assert_eq!(refs.len(), 2);

    let draft = scope_draft(
        &refs[0],
        vec![overridden(1, "demo", &refs[1], QUOTE), scope_candidate(2)],
    );
    let saved = client.call("save_candidate_set", head.save(draft)).await;
    // The override does not depend on Jev: no advice field, no journal row.
    assert!(saved.get("advice").is_none(), "{saved}");
    assert_eq!(stand.count("advice_journal").await, 0);

    let head = ScopeSet::after(&mut client, saved).await;
    let kept = json!({"mode":"demo","source_ref_id":refs[1],"exact_quote":QUOTE});
    let (listed, details) = head.candidates(&mut client).await;
    for candidates in [&listed, &details] {
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0]["mode_override"], kept);
        assert!(candidates[1].get("mode_override").is_none());
    }
    let stored: Value = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 \
         ORDER BY set_revision DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert_eq!(stored["candidates"][0]["mode_override"], kept);
    assert!(stored["candidates"][1].get("mode_override").is_none());
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_override_without_the_users_quoted_permission_is_refused_and_stores_nothing() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let begun = ScopeSet::begun(&mut client, &repo).await;
    let first_snapshot_refs = begun.input_refs(&mut client).await;
    let head = begun.record_and_refresh(&mut client, PERMISSION).await;
    let refs = head.input_refs(&mut client).await;
    let fields = head.field_refs(&mut client).await;
    let field = |kind: &str, name: &str| {
        fields
            .iter()
            .find(|field| field["kind"] == kind && field["program_field"] == name)
            .unwrap_or_else(|| panic!("{kind} {name} in {fields:?}"))["id"]
            .clone()
    };
    let receipts = stand.count("scope_candidate_receipts").await;

    let level = "/params/draft/candidates/0/mode_override/mode";
    let quote = "/params/draft/candidates/0/mode_override/exact_quote";
    let source = "/params/draft/candidates/0/mode_override/source_ref_id";
    let cases = [
        // A quote that is not in the input, and one that is not its exact wording.
        (
            overridden(1, "demo", &refs[1], "I allow prod"),
            "QUOTE",
            quote,
        ),
        (
            overridden(1, "demo", &refs[1], "i allow demo"),
            "QUOTE",
            quote,
        ),
        // A blank quote.
        (overridden(1, "demo", &refs[1], " \n"), "QUOTE", quote),
        // A field of the Program, quoted exactly: the agent wrote the PRD.
        (
            overridden(
                1,
                "demo",
                &field("program_success", "success"),
                "The cause and correction are verified",
            ),
            "INPUT",
            source,
        ),
        (
            overridden(
                1,
                "demo",
                &field("program_field", "constraints"),
                "No deployment",
            ),
            "INPUT",
            source,
        ),
        // The reference an earlier snapshot issued for an input of this set.
        (
            overridden(1, "demo", &first_snapshot_refs[0], "preview diagnosis"),
            "INPUT",
            source,
        ),
        // An id this candidate set never issued.
        (
            overridden(1, "demo", &json!(Uuid::new_v4()), QUOTE),
            "INPUT",
            source,
        ),
        // A level that is not one of the four.
        (overridden(1, "enterprise", &refs[1], QUOTE), "LEVEL", level),
    ];
    for (candidate, rule, path) in cases {
        let draft = scope_draft(&refs[0], vec![candidate, scope_candidate(2)]);
        let refusal = refused(&mut client, SAVE, head.save(draft)).await;
        assert_unpermitted(
            &refusal,
            &format!("SCOPE-DRAFT-MODE-OVERRIDE-{rule}"),
            path,
            &STEPS,
        );
    }
    // The part at fault is named by the position of its candidate in the draft.
    let draft = scope_draft(
        &refs[0],
        vec![
            scope_candidate(1),
            overridden(2, "demo", &refs[1], "allow prod"),
        ],
    );
    let refusal = refused(&mut client, SAVE, head.save(draft)).await;
    assert_unpermitted(
        &refusal,
        "SCOPE-DRAFT-MODE-OVERRIDE-QUOTE",
        "/params/draft/candidates/1/mode_override/exact_quote",
        &STEPS,
    );
    assert_eq!(refusal.0["refusal"]["required"], "user_permission_quote");

    // An override is stated whole or left out; the schema refuses anything else.
    for broken in [
        Value::Null,
        json!({"mode":"demo","source_ref_id":refs[1]}),
        json!({"mode":"demo","input_id":refs[1],"exact_quote":QUOTE}),
        json!({"mode":"demo","source_ref_id":refs[1],"exact_quote":QUOTE,"reason":"mine"}),
    ] {
        let mut candidate = scope_candidate(1);
        candidate["mode_override"] = broken;
        let draft = scope_draft(&refs[0], vec![candidate]);
        let (error, _) = refused(&mut client, SAVE, head.save(draft)).await;
        assert_eq!(error["code"], "invalid_arguments", "{error}");
    }

    // Nothing of any refused save was stored, and the head did not move.
    assert_eq!(stand.count("scope_candidate_drafts").await, 0);
    assert_eq!(stand.count("scope_candidate_receipts").await, receipts);
    let draft = scope_draft(&refs[0], vec![scope_candidate(1)]);
    let saved = client.call("save_candidate_set", head.save(draft)).await;
    assert_eq!(
        saved["candidate_set"]["revision"].as_i64().unwrap(),
        head.revision().as_i64().unwrap() + 1
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn asking_recording_refreshing_and_quoting_succeeds_after_a_refusal() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let head = ScopeSet::begun(&mut client, &repo).await;
    let refs = head.input_refs(&mut client).await;

    // The agent decides for itself and cites the planning request: refused.
    let draft = |refs: &[Value], cited: &Value| {
        scope_draft(&refs[0], vec![overridden(1, "demo", cited, QUOTE)])
    };
    let refusal = refused(&mut client, SAVE, head.save(draft(&refs, &refs[0]))).await;
    assert_unpermitted(
        &refusal,
        "SCOPE-DRAFT-MODE-OVERRIDE-QUOTE",
        "/params/draft/candidates/0/mode_override/exact_quote",
        &STEPS,
    );

    // Record the user's answer. Before the refresh it has no reference to cite, and
    // a save is refused as stale whatever it cites.
    let recorded = head.record(&mut client, PERMISSION).await;
    assert_eq!(head.input_refs(&mut client).await, refs);
    let unrefreshed = ScopeSet::after(&mut client, recorded.clone()).await;
    let (error, _) = refused(&mut client, SAVE, unrefreshed.save(draft(&refs, &refs[0]))).await;
    assert_eq!(error["code"], "stale_context", "{error}");

    // Refresh, read the answer's reference in view inputs, save again quoting it.
    let refreshed = crate::walk::follow(&mut client, &recorded["actions"][1]).await;
    let head = ScopeSet::after(&mut client, refreshed).await;
    let refs = head.input_refs(&mut client).await;
    assert_eq!(refs.len(), 2);
    let saved = client
        .call("save_candidate_set", head.save(draft(&refs, &refs[1])))
        .await;
    let (_, details) = ScopeSet::after(&mut client, saved)
        .await
        .candidates(&mut client)
        .await;
    assert_eq!(
        details[0]["mode_override"],
        json!({"mode":"demo","source_ref_id":refs[1],"exact_quote":QUOTE})
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn the_same_permission_cited_after_a_refresh_is_no_change_of_the_candidate() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let head = ScopeSet::begun(&mut client, &repo)
        .await
        .record_and_refresh(&mut client, PERMISSION)
        .await;
    let refs = head.input_refs(&mut client).await;
    let draft = scope_draft(
        &refs[0],
        vec![overridden(1, "demo", &refs[1], QUOTE), scope_candidate(2)],
    );
    let saved = client.call("save_candidate_set", head.save(draft)).await;

    // One more input and a refresh: every reference of the new snapshot is new.
    let head = ScopeSet::after(&mut client, saved)
        .await
        .record_and_refresh(&mut client, "Keep the wording of the report as it is.")
        .await;
    let renewed = head.input_refs(&mut client).await;
    assert_eq!(renewed.len(), 3);
    assert!(renewed.iter().all(|reference| !refs.contains(reference)));
    let details = json!({"draft":{"goals":head_goals(&head, &mut client).await,
        "candidates":head.candidates(&mut client).await.1}});

    // The earlier reference belongs to another snapshot now.
    let stale = scope_resave(&details, &renewed[0]);
    let refusal = refused(&mut client, SAVE, head.save(stale)).await;
    assert_unpermitted(
        &refusal,
        "SCOPE-DRAFT-MODE-OVERRIDE-INPUT",
        "/params/draft/candidates/0/mode_override/source_ref_id",
        &STEPS,
    );

    // A changed quote is a change of the candidate and needs its rationale.
    let mut requoted = scope_resave(&details, &renewed[0]);
    requoted["candidates"][0]["mode_override"] =
        json!({"mode":"demo","source_ref_id":renewed[1],"exact_quote":"a throwaway script"});
    let (error, _) = refused(&mut client, SAVE, head.save(requoted.clone())).await;
    assert_eq!(error["code"], "invalid_arguments", "{error}");
    assert!(
        error["details"]["reason"]
            .as_str()
            .unwrap()
            .contains("add change_rationale"),
        "{error}"
    );

    // The same level and quote through the new reference: saved as unchanged.
    let mut same = scope_resave(&details, &renewed[0]);
    same["candidates"][0]["mode_override"]["source_ref_id"] = renewed[1].clone();
    let saved = client.call("save_candidate_set", head.save(same)).await;
    let head = ScopeSet::after(&mut client, saved).await;
    let (_, candidates) = head.candidates(&mut client).await;
    assert_eq!(
        candidates[0]["mode_override"],
        json!({"mode":"demo","source_ref_id":renewed[1],"exact_quote":QUOTE})
    );
    assert_eq!(
        (&candidates[0]["revision"], &candidates[1]["revision"]),
        (&json!(1), &json!(1))
    );

    // With its rationale the changed quote is saved as a new revision, and so is
    // an override taken away.
    requoted["candidates"][0]["change_rationale"] = json!("The user's words on the script");
    let saved = client.call("save_candidate_set", head.save(requoted)).await;
    let head = ScopeSet::after(&mut client, saved).await;
    let (_, candidates) = head.candidates(&mut client).await;
    assert_eq!(candidates[0]["revision"], 2);
    assert_eq!(
        candidates[0]["mode_override"]["exact_quote"],
        "a throwaway script"
    );
    let details = json!({"draft":{"goals":head_goals(&head, &mut client).await,
        "candidates":candidates}});
    let mut plain = scope_resave(&details, &renewed[0]);
    plain["candidates"][0]
        .as_object_mut()
        .unwrap()
        .remove("mode_override");
    let (error, _) = refused(&mut client, SAVE, head.save(plain.clone())).await;
    assert_eq!(error["code"], "invalid_arguments", "{error}");
    plain["candidates"][0]["change_rationale"] =
        json!("The Scope goes back to the Program's level");
    let saved = client.call("save_candidate_set", head.save(plain)).await;
    let (_, candidates) = ScopeSet::after(&mut client, saved)
        .await
        .candidates(&mut client)
        .await;
    assert_eq!(candidates[0]["revision"], 3);
    assert!(candidates[0].get("mode_override").is_none());
    client.finish().await;
}

/// The goals of the saved draft as `details` shows them.
async fn head_goals(head: &ScopeSet, client: &mut crate::recovery_support::Mcp) -> Value {
    head.details(client).await["draft"]["goals"].clone()
}
