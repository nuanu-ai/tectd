use super::{Bytes, ReadResult, Request, check, field_set, keys, ready, uuid};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tect_domain::{ProgramPage, ProgramStep};

pub(super) fn logical(value: &Value, request: &Request, bytes: &Bytes) -> ReadResult<ProgramPage> {
    let expected = if bytes.source.is_some() {
        field_set("program inputs next_after_input")
    } else {
        field_set("program inputs next_after_input actions recommended_action")
    };
    check(keys(value)? == expected, "ProgramPage shape")?;
    let mut program_fields = field_set(
        "id workspace_id status revision name intent basis boundaries constraints success working_notes pending_question current_step input_cursor latest_input",
    );
    if value["program"].get("planning_knowledge").is_some() {
        program_fields.insert("planning_knowledge");
    }
    check(
        keys(&value["program"])? == program_fields,
        "full Program field set",
    )?;
    let typed: ProgramPage = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let p = &typed.program;
    check(
        p.id == request.id
            && !p.workspace_id.is_nil()
            && p.revision > 0
            && request.revision.is_none_or(|r| r as i64 == p.revision),
        "decoded Program identity/revision",
    )?;
    check(
        p.input_cursor >= 0 && p.latest_input >= p.input_cursor,
        "Program cursors",
    )?;
    if let Some(source) = &bytes.source {
        check(
            source == &json!({"program_id":p.id,"program_revision":p.revision}),
            "decoded source pin",
        )?;
    }
    let after = request.after.unwrap_or(p.input_cursor as usize);
    check(
        bytes.normalized_after.is_none_or(|a| a == after),
        "omitted cursor normalization mismatch",
    )?;
    check(
        typed.inputs.len() <= request.limit,
        "collection limit exceeded",
    )?;
    let mut previous = after;
    let mut ids = BTreeSet::new();
    for (input, raw) in typed.inputs.iter().zip(value["inputs"].as_array().unwrap()) {
        check(
            keys(raw)? == field_set("id sequence request_id session_id input"),
            "whole input shape",
        )?;
        check(
            !input.id.is_nil()
                && !input.request_id.is_nil()
                && !input.session_id.is_nil()
                && ids.insert(input.id),
            "input UUIDs",
        )?;
        check(
            input.sequence > previous as i64 && input.sequence <= p.latest_input,
            "input ordering/range",
        )?;
        previous = input.sequence as usize;
    }
    if let Some(next) = typed.next_after_input {
        check(
            !typed.inputs.is_empty() && next == previous as i64 && next < p.latest_input,
            "collection next cursor",
        )?;
    } else {
        check(
            previous as i64 >= p.latest_input,
            "collection EOF before latest input",
        )?;
    }
    Ok(typed)
}
fn command<'a>(action: &'a Value, route: &str, kind: &str) -> ReadResult<&'a Value> {
    check(
        action["kind"] == kind
            && action["tool"] == "command"
            && action["arguments"]["route"] == route,
        "inline command identity",
    )?;
    Ok(&action["arguments"]["params"])
}
fn inline_actions(page: &ProgramPage, actions: &[Value], rec: Option<usize>) -> ReadResult<()> {
    check(rec == Some(0), "inline recommendation")?;
    let p = &page.program;
    let mut index = 0;
    if p.planning_knowledge
        .as_ref()
        .is_some_and(|s| !s.stale_reasons.is_empty())
    {
        let args = command(
            actions.get(index).ok_or("refresh missing")?,
            "program.knowledge.refresh",
            "ready_call",
        )?;
        check(
            keys(args)? == field_set("program_id revision input_cursor request_id")
                && args["program_id"] == p.id.to_string()
                && args["revision"] == p.revision
                && args["input_cursor"] == p.input_cursor,
            "refresh pins",
        )?;
        uuid(&args["request_id"])?;
        index += 1;
    }
    check(
        actions.get(index)
            == Some(
                &json!({"kind":"ready_call","tool":"help","arguments":{"mode":"describe","method":"tectd-program"}}),
            ),
        "inline method Help",
    )?;
    index += 1;
    if p.current_step == ProgramStep::Compose {
        if let Some(next) = page.next_after_input {
            check(
                ready(actions.get(index).ok_or("collection action missing")?)?
                    == &json!({"program_id":p.id,"after_input":next,"limit":25}),
                "inline collection action",
            )?;
            index += 1;
        }
        let args = command(
            actions.get(index).ok_or("save missing")?,
            "program.save",
            "needs_input",
        )?;
        let delivered = page
            .inputs
            .last()
            .map_or(p.input_cursor, |i| i.sequence)
            .max(p.input_cursor);
        check(
            args["program_id"] == p.id.to_string()
                && args["revision"] == p.revision
                && args["input_cursor"] == delivered,
            "save pins",
        )?;
        if let Some(manifest) = p
            .planning_knowledge
            .as_ref()
            .and_then(|s| s.manifest.as_ref())
        {
            check(
                args["consumed_knowledge"]
                    == json!({"manifest_id":manifest.id,"digest":manifest.digest,"workspace_generation":manifest.workspace_generation}),
                "save manifest guard",
            )?;
        }
    } else {
        let args = command(
            actions.get(index).ok_or("record missing")?,
            "program.record_input",
            "needs_input",
        )?;
        check(
            args["program_id"] == p.id.to_string(),
            "record Program identity",
        )?;
        uuid(&args["request_id"])?;
        if let Some(manifest) = p
            .planning_knowledge
            .as_ref()
            .and_then(|s| s.manifest.as_ref())
        {
            check(
                args["task_context"]
                    == serde_json::to_value(&manifest.task_context).map_err(|e| e.to_string())?,
                "record task context",
            )?;
        }
    }
    check(actions.len() == index + 1, "unexpected inline actions")
}
pub(super) fn terminal(
    page: &ProgramPage,
    request: &Request,
    bytes: &Bytes,
    actions: &[Value],
    rec: Option<usize>,
) -> ReadResult<()> {
    if bytes.source.is_none() {
        return inline_actions(page, actions, rec);
    }
    if let Some(next) = page.next_after_input {
        check(
            actions.len() == 1 && rec == Some(0),
            "fragment collection continuation",
        )?;
        check(
            ready(&actions[0])?
                == &json!({"program_id":page.program.id,"program_revision":page.program.revision,"after_input":next,"limit":request.limit}),
            "fragment terminal collection pins",
        )
    } else {
        check(
            actions.is_empty() && rec.is_none(),
            "fragment terminal EOF controls",
        )
    }
}
#[cfg(test)]
mod tests {
    use super::super::tests::{alter, collect, fixture, fragments, raw};
    use serde_json::json;
    use uuid::Uuid;
    #[tokio::test]
    async fn empty_page_beyond_latest_keeps_the_explicit_cursor() {
        let mut value = fixture(false);
        value["inputs"] = json!([]);
        value["actions"][1]["arguments"]["params"]["input_cursor"] = json!(1);
        let params = json!({"program_id":Uuid::from_u128(1),"after_input":4});
        let read = collect(vec![raw(value)], params.clone()).await.unwrap();
        assert_eq!(read.provenance.initial_query["params"], params);
        assert!(read.value["inputs"].as_array().unwrap().is_empty());
        assert!(read.value["next_after_input"].is_null());
    }
    #[tokio::test]
    async fn selector_revision_byte_and_progress_corruption_are_refused() {
        let params =
            json!({"program_id":Uuid::from_u128(1),"after_input":1,"limit":1,"program_revision":1});
        for case in [
            "selector",
            "revision",
            "digest",
            "nonprogress",
            "continuation_at_total",
            "early_eof",
            "missing_nullable",
        ] {
            let mut pages = fragments(true, 1, 1);
            if case == "missing_nullable" {
                let mut value = fixture(true);
                value["program"].as_object_mut().unwrap().remove("name");
                pages = vec![raw(value)];
            } else {
                alter(&mut pages[0], |page| match case {
                    "selector" => {
                        page["actions"][0]["arguments"]["params"]["program_id"] =
                            json!(Uuid::from_u128(99))
                    }
                    "revision" => page["source"]["program_revision"] = json!(2),
                    "digest" => page["representation_digest"] = json!("0".repeat(64)),
                    "nonprogress" => {
                        page["text"] = json!("");
                        page["returned_bytes"] = json!(0);
                        page["next_offset_bytes"] = json!(0);
                        page["actions"][0]["arguments"]["params"]["offset_bytes"] = json!(0);
                    }
                    "continuation_at_total" => {
                        page["total_bytes"] = page["returned_bytes"].clone();
                    }
                    _ => {
                        page["next_offset_bytes"] = serde_json::Value::Null;
                        page["actions"] = json!([]);
                        page["recommended_action"] = serde_json::Value::Null;
                    }
                });
            }
            assert!(collect(pages, params.clone()).await.is_err(), "{case}");
        }
        let mut pages = fragments(true, 1, 1);
        alter(&mut pages[1], |page| {
            page["text"] = json!(page["text"].as_str().unwrap().replace("whole", "WHOLE"))
        });
        assert!(
            collect(pages, params).await.is_err(),
            "corrupted reconstructed bytes"
        );
    }
}
