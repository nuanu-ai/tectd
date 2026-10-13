//! One walk with Jev enabled through both planning levels of a Program at `prod`: a
//! Scope candidate the user permitted at `demo`, the Scope opened from it, and inside
//! that Scope a work node permitted at `mvp`. It returns what Jev was sent, in order.
use crate::planning::route;
use crate::support::{Jev, Stand, configure};
use crate::walk::{
    PERMISSION, QUOTE, ScopeSet, SliceSet, scope_candidate, scope_draft, slice_draft, work_node,
};
use serde_json::{Value, json};

/// The six requests of the walk: `scope_save` and `slice_save` are `sizing`,
/// `slice_pipeline` is the `pipeline` point of the same Slice save, the three inputs
/// are `intake`. Each advice binary reads the requests of its own point.
pub struct Sent {
    pub scope_save: Value,
    pub scope_input: Value,
    pub slice_first_input: Value,
    pub slice_save: Value,
    pub slice_pipeline: Value,
    pub slice_input: Value,
}

fn settled(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("small", 0.7),
        "placement" => ("new_candidate", 0.7),
        "nearest" => ("none", 0.7),
        "fit" => ("in_line", 0.7),
        size if size.ends_with(".size") => ("bounded", 0.7),
        advice if advice.ends_with(".advice") => ("keep", 0.7),
        pipeline if pipeline.ends_with(".pipeline") => ("slice.debug-root-cause", 0.7),
        _ => ("requested", 0.7),
    }
}

pub async fn walk(stand: &Stand) -> Sent {
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let head = ScopeSet::begun(&mut client, &repo)
        .await
        .record_and_refresh(&mut client, PERMISSION)
        .await;
    let refs = head.input_refs(&mut client).await;
    configure(&mut client, true, 0).await;

    // Program to Scopes: the first candidate is permitted demo.
    let mut permitted = scope_candidate(1);
    permitted["mode_override"] = json!({"mode":"demo","source_ref_id":refs[1],"exact_quote":QUOTE});
    let draft = scope_draft(&refs[0], vec![permitted, scope_candidate(2)]);
    let saved = client.call("save_candidate_set", head.save(draft)).await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    let head = ScopeSet::after(&mut client, saved)
        .await
        .record_and_refresh(&mut client, "Also keep the audit log readable.")
        .await;
    let (_, stored) = head.candidates(&mut client).await;
    let reviewed = client
        .call("save_candidate_set", head.accept(&stored))
        .await;
    let ready = ScopeSet::after(&mut client, reviewed).await;

    // Scope to Slices, inside the Scope opened from the permitted candidate.
    let planning = SliceSet::opened(&mut client, &ready, &stored[0]).await;
    let head = planning
        .record_and_refresh(
            &mut client,
            "The importer has paying users: for it I allow mvp.",
        )
        .await;
    let input_id = head.items(&mut client, "inputs").await[0]["id"].clone();
    let mut permitted = work_node(1);
    permitted["mode_override"] =
        json!({"mode":"mvp","input_id":input_id,"exact_quote":"for it I allow mvp"});
    let draft = slice_draft(vec![permitted, work_node(2)]);
    let saved = route(
        &mut client,
        "command",
        "slice.candidates.save",
        head.save(draft),
    )
    .await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    SliceSet::after(&mut client, saved)
        .await
        .record(&mut client, "Name the file after the day.")
        .await;
    client.finish().await;

    let sent: [Value; 6] = stand.jev.requests().try_into().expect("six requests");
    let [
        scope_save,
        scope_input,
        slice_first_input,
        slice_save,
        slice_pipeline,
        slice_input,
    ] = sent;
    Sent {
        scope_save,
        scope_input,
        slice_first_input,
        slice_save,
        slice_pipeline,
        slice_input,
    }
}
