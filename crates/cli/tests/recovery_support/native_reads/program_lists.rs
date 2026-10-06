//! Explicit native Program List reads; collection controls are retained, never executed.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ops::AsyncFnMut};
use tect_domain::{ProgramCursor, ProgramList};
use uuid::Uuid;
const FOOTER: &str = "Follow the rules from workspace.open or help {\"text\":\"response-rules\"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real.";
type ReadResult<T> = Result<T, String>;
pub struct ListRead {
    pub value: Value,
    pub initial_query: Value,
    pub source: Option<Value>,
    pub representation_digest: Option<String>,
    pub pages: usize,
    pub maximum_mcp_bytes: usize,
    pub first_byte_query: Option<Value>,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Option<usize>,
}
fn check(ok: bool, reason: &str) -> ReadResult<()> {
    if ok { Ok(()) } else { Err(reason.into()) }
}
fn field_set(fields: &str) -> BTreeSet<&str> {
    fields.split_whitespace().collect()
}
fn keys(value: &Value) -> ReadResult<BTreeSet<&str>> {
    Ok(value
        .as_object()
        .ok_or("object required")?
        .keys()
        .map(String::as_str)
        .collect())
}
fn number(value: &Value) -> ReadResult<usize> {
    usize::try_from(value.as_u64().ok_or("nonnegative integer required")?)
        .map_err(|e| e.to_string())
}
fn uuid(value: &Value) -> ReadResult<Uuid> {
    let id = Uuid::parse_str(value.as_str().ok_or("UUID required")?).map_err(|e| e.to_string())?;
    check(!id.is_nil(), "nil UUID")?;
    Ok(id)
}
fn envelope(raw: &Value) -> ReadResult<(Value, usize)> {
    check(raw.get("error").is_none(), "protocol error")?;
    let result = raw
        .get("result")
        .filter(|v| v.is_object())
        .ok_or("MCP result missing")?;
    check(
        result["isError"] != true && result.get("structuredContent").is_none(),
        "unsuccessful MCP result",
    )?;
    let size = serde_json::to_vec(result).map_err(|e| e.to_string())?.len();
    check(size <= 8192, "MCP result budget")?;
    let blocks = result["content"].as_array().ok_or("text blocks missing")?;
    check(
        blocks.len() == 3 && blocks.iter().all(|b| b["type"] == "text"),
        "text block shape",
    )?;
    check(
        blocks[0]["text"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 2000)
            && blocks[2]["text"] == FOOTER,
        "intro or rules changed",
    )?;
    let page: Value = serde_json::from_str(blocks[1]["text"].as_str().ok_or("JSON text missing")?)
        .map_err(|e| e.to_string())?;
    check(page.is_object(), "Program object required")?;
    Ok((page, size))
}

struct Request {
    workspace: Option<Uuid>,
    after: Option<String>,
    limit: usize,
}
impl Request {
    fn parse(params: &Value) -> ReadResult<Self> {
        check(
            keys(params)?.is_subset(&field_set("workspace_id after limit")),
            "initial selectors",
        )?;
        let workspace = params.get("workspace_id").map(uuid).transpose()?;
        let after = params
            .get("after")
            .map(|v| {
                let text = v.as_str().ok_or("collection cursor string required")?;
                ProgramCursor::parse(text).map_err(|e| e.to_string())?;
                Ok::<_, String>(text.to_owned())
            })
            .transpose()?;
        let limit = params.get("limit").map(number).transpose()?.unwrap_or(25);
        check((1..=100).contains(&limit), "collection limit")?;
        Ok(Self {
            workspace,
            after,
            limit,
        })
    }
}
fn ready(action: &Value) -> ReadResult<&Value> {
    check(
        keys(action)? == field_set("kind tool arguments")
            && action["kind"] == "ready_call"
            && action["tool"] == "query",
        "actual Ready Query",
    )?;
    check(
        keys(&action["arguments"])? == field_set("route params")
            && action["arguments"]["route"] == "program.list",
        "List route",
    )?;
    Ok(&action["arguments"]["params"])
}
fn metadata(page: &Value) -> ReadResult<(Vec<Value>, Option<usize>)> {
    let actions = page["actions"]
        .as_array()
        .ok_or("actual actions missing")?
        .clone();
    let value = page
        .get("recommended_action")
        .ok_or("recommendation missing")?;
    let rec = if value.is_null() {
        None
    } else {
        Some(number(value)?)
    };
    check(
        rec.is_none_or(|i| i < actions.len()),
        "recommendation range",
    )?;
    Ok((actions, rec))
}
#[derive(Default)]
struct Bytes {
    data: Vec<u8>,
    source: Option<Value>,
    digest: Option<String>,
    total: usize,
    window: Option<usize>,
    first_query: Option<Value>,
}
impl Bytes {
    fn append(&mut self, page: &Value, request: &Request) -> ReadResult<()> {
        check(
            keys(page)?
                == field_set(
                    "kind format encoding source representation_digest total_bytes offset_bytes returned_bytes text next_offset_bytes actions recommended_action",
                ),
            "fragment shape",
        )?;
        check(
            page["kind"] == "fragment" && page["format"] == "json" && page["encoding"] == "utf-8",
            "fragment format",
        )?;
        let source = &page["source"];
        check(keys(source)? == field_set("workspace_id"), "source shape")?;
        let workspace = uuid(&source["workspace_id"])?;
        check(
            request.workspace.is_none_or(|id| id == workspace),
            "workspace selector drift",
        )?;
        let digest = page["representation_digest"]
            .as_str()
            .ok_or("digest missing")?;
        let total = number(&page["total_bytes"])?;
        check(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && total > 0
                && total <= 8 * 1024 * 1024,
            "digest or total",
        )?;
        if let Some(pin) = &self.source {
            check(
                pin == source && self.digest.as_deref() == Some(digest) && self.total == total,
                "fragment pins changed",
            )?;
        } else {
            self.source = Some(source.clone());
            self.digest = Some(digest.to_owned());
            self.total = total;
        }
        let text = page["text"].as_str().ok_or("UTF-8 fragment missing")?;
        let returned = number(&page["returned_bytes"])?;
        check(
            number(&page["offset_bytes"])? == self.data.len()
                && returned == text.len()
                && returned <= 4096
                && self.window.is_none_or(|w| returned <= w)
                && self
                    .data
                    .len()
                    .checked_add(returned)
                    .is_some_and(|end| end <= total),
            "fragment byte bounds",
        )?;
        self.data.extend_from_slice(text.as_bytes());
        Ok(())
    }
    fn next(&mut self, page: &Value, request: &Request) -> ReadResult<Value> {
        check(
            number(&page["returned_bytes"])? > 0
                && number(&page["next_offset_bytes"])? == self.data.len()
                && self.data.len() < self.total,
            "nonprogress or contradictory continuation",
        )?;
        let (actions, rec) = metadata(page)?;
        check(
            actions.len() == 1 && rec == Some(0),
            "unique byte continuation",
        )?;
        let params = ready(&actions[0])?;
        let mut fields =
            field_set("workspace_id limit offset_bytes limit_bytes representation_digest");
        if request.after.is_some() {
            fields.insert("after");
        }
        check(keys(params)? == fields, "continuation selectors shape")?;
        let window = number(&params["limit_bytes"])?;
        check(
            params["workspace_id"] == self.source.as_ref().ok_or("source missing")?["workspace_id"]
                && number(&params["limit"])? == request.limit
                && params.get("after").and_then(Value::as_str) == request.after.as_deref()
                && number(&params["offset_bytes"])? == self.data.len()
                && params["representation_digest"].as_str() == self.digest.as_deref()
                && (1..=4096).contains(&window)
                && self.window.is_none_or(|w| w == window),
            "continuation pin drift",
        )?;
        self.window = Some(window);
        let args = actions[0]["arguments"].clone();
        if self.first_query.is_none() {
            self.first_query = Some(args.clone());
        }
        Ok(args)
    }
    fn finish(&self) -> ReadResult<Value> {
        check(
            self.data.len() == self.total
                && self.digest.as_deref()
                    == Some(format!("{:x}", Sha256::digest(&self.data)).as_str()),
            "EOF length or SHA",
        )?;
        serde_json::from_slice(&self.data).map_err(|e| e.to_string())
    }
}
fn logical(value: &Value, request: &Request, fragmented: bool) -> ReadResult<ProgramList> {
    let fields = if fragmented {
        field_set("programs next_after")
    } else {
        field_set("programs next_after actions recommended_action")
    };
    check(keys(value)? == fields, "full ProgramList shape")?;
    let list: ProgramList = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    check(
        list.programs.len() <= request.limit,
        "collection limit exceeded",
    )?;
    let mut ids = BTreeSet::new();
    for (summary, raw) in list
        .programs
        .iter()
        .zip(value["programs"].as_array().ok_or("programs array")?)
    {
        check(
            keys(raw)? == field_set("id status revision name current_step")
                && !summary.id.is_nil()
                && summary.revision > 0
                && ids.insert(summary.id),
            "whole summary shape or identity",
        )?;
    }
    if let Some(after) = &list.next_after {
        ProgramCursor::parse(after).map_err(|e| e.to_string())?;
        check(
            list.programs
                .last()
                .is_some_and(|p| p.cursor().encode() == *after),
            "collection next cursor",
        )?;
    }
    Ok(list)
}
fn terminal(list: &ProgramList, actions: &[Value], rec: Option<usize>) -> ReadResult<()> {
    check(rec == Some(0), "List recommendation")?;
    let count = list.programs.len() + usize::from(list.next_after.is_some()) + 1;
    check(actions.len() == count, "terminal action count")?;
    for (summary, action) in list.programs.iter().zip(actions) {
        check(
            action
                == &json!({"kind":"ready_call","tool":"query","arguments":{"route":"program.get","params":{"program_id":summary.id}}}),
            "summary Get control",
        )?;
    }
    if let Some(after) = &list.next_after {
        check(
            ready(&actions[list.programs.len()])? == &json!({"after":after,"limit":25}),
            "collection List control",
        )?;
    }
    let begin = actions.last().ok_or("begin missing")?;
    check(
        begin["kind"] == "needs_input"
            && begin["tool"] == "command"
            && begin["arguments"]["route"] == "program.begin"
            && keys(&begin["arguments"]["params"])? == field_set("request_id"),
        "begin control",
    )?;
    uuid(&begin["arguments"]["params"]["request_id"])?;
    check(
        begin["input"]["fields"]
            .as_array()
            .is_some_and(|v| v.len() == 1 && v[0]["path"] == "arguments.params.input"),
        "begin input descriptor",
    )?;
    Ok(())
}
pub async fn read_program_list(
    mut raw_call: impl AsyncFnMut(Value) -> Value,
    params: Value,
) -> ReadResult<ListRead> {
    let request = Request::parse(&params)?;
    let initial = json!({"route":"program.list","params":params});
    let mut args = initial.clone();
    let mut bytes = Bytes::default();
    let mut maximum_mcp_bytes = 0;
    for pages in 1..=8192 {
        let (page, size) = envelope(&raw_call(args.clone()).await)?;
        maximum_mcp_bytes = maximum_mcp_bytes.max(size);
        let fragment = page["kind"] == "fragment";
        let value = if fragment {
            bytes.append(&page, &request)?;
            if !page["next_offset_bytes"].is_null() {
                args = bytes.next(&page, &request)?;
                continue;
            }
            bytes.finish()?
        } else {
            check(
                bytes.source.is_none() && pages == 1,
                "fragment became inline",
            )?;
            page.clone()
        };
        let typed = logical(&value, &request, fragment)?;
        let (actions, rec) = metadata(&page)?;
        terminal(&typed, &actions, rec)?;
        return Ok(ListRead {
            value,
            initial_query: initial,
            source: bytes.source,
            representation_digest: bytes.digest,
            pages,
            maximum_mcp_bytes,
            first_byte_query: bytes.first_query,
            terminal_actions: actions,
            terminal_recommended_action: rec,
        });
    }
    Err("List byte page bound exceeded".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_null_cursor_is_rejected_before_calling_native_transport() {
        let mut calls = 0;
        let result = read_program_list(
            async |_| {
                calls += 1;
                Value::Null
            },
            json!({"limit":1,"after":null}),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls, 0);
    }
}
