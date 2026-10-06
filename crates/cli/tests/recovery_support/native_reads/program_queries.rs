//! Explicit current Program queries; synthetic tests are not native acceptance evidence.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ops::AsyncFnMut};
use uuid::Uuid;
const FOOTER: &str = "Follow the rules from workspace.open or help {\"text\":\"response-rules\"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real.";
#[path = "program_queries/validation.rs"]
mod validation;
type ReadResult<T> = Result<T, String>;
pub struct Provenance {
    pub initial_query: Value,
    pub source: Option<Value>,
    pub representation_digest: Option<String>,
    pub pages: usize,
    pub maximum_mcp_bytes: usize,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Option<usize>,
}
pub struct QueryRead {
    pub value: Value,
    pub provenance: Provenance,
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
    id: Uuid,
    revision: Option<usize>,
    after: Option<usize>,
    limit: usize,
}
impl Request {
    fn parse(params: &Value) -> ReadResult<Self> {
        check(
            keys(params)?.is_subset(&field_set("program_id program_revision after_input limit")),
            "unknown initial selector",
        )?;
        let revision = params.get("program_revision").map(number).transpose()?;
        let after = params.get("after_input").map(number).transpose()?;
        let limit = params.get("limit").map(number).transpose()?.unwrap_or(25);
        check(
            (1..=100).contains(&limit)
                && revision.is_none_or(|r| r > 0 && r <= i64::MAX as usize)
                && after.is_none_or(|a| a <= i64::MAX as usize),
            "invalid revision or collection limit",
        )?;
        Ok(Self {
            id: uuid(&params["program_id"])?,
            revision,
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
        "actual Ready Query required",
    )?;
    check(
        keys(&action["arguments"])? == field_set("route params")
            && action["arguments"]["route"] == "program.get",
        "Program query route changed",
    )?;
    Ok(&action["arguments"]["params"])
}
fn metadata(page: &Value) -> ReadResult<(Vec<Value>, Option<usize>)> {
    let actions = page["actions"]
        .as_array()
        .ok_or("actual actions missing")?
        .clone();
    let rec = page
        .get("recommended_action")
        .ok_or("actual recommendation missing")?;
    let rec = if rec.is_null() {
        None
    } else {
        Some(number(rec)?)
    };
    check(
        rec.is_none_or(|i| i < actions.len()),
        "recommendation out of range",
    )?;
    Ok((actions, rec))
}
#[derive(Default)]
struct Bytes {
    data: Vec<u8>,
    source: Option<Value>,
    digest: Option<String>,
    total: usize,
    normalized_after: Option<usize>,
    window: Option<usize>,
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
            page["format"] == "json" && page["encoding"] == "utf-8",
            "fragment encoding",
        )?;
        let source = &page["source"];
        check(
            keys(source)? == field_set("program_id program_revision")
                && uuid(&source["program_id"])? == request.id,
            "fragment Program identity",
        )?;
        let revision = number(&source["program_revision"])?;
        check(
            revision > 0
                && revision <= i64::MAX as usize
                && request.revision.is_none_or(|r| r == revision),
            "fragment revision",
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
            "digest or total invalid",
        )?;
        if let Some(pin) = &self.source {
            check(
                pin == source && self.digest.as_deref() == Some(digest) && self.total == total,
                "fragment pins changed",
            )?;
        } else {
            self.source = Some(source.clone());
            self.digest = Some(digest.into());
            self.total = total;
        }
        let text = page["text"].as_str().ok_or("UTF-8 fragment missing")?;
        let returned = number(&page["returned_bytes"])?;
        check(
            number(&page["offset_bytes"])? == self.data.len()
                && returned == text.len()
                && returned <= 4096
                && self.window.is_none_or(|w| returned <= w),
            "fragment offset or byte count",
        )?;
        check(
            self.data
                .len()
                .checked_add(returned)
                .is_some_and(|end| end <= total),
            "fragment exceeds total",
        )?;
        self.data.extend_from_slice(text.as_bytes());
        Ok(())
    }
    fn next(&mut self, page: &Value, request: &Request) -> ReadResult<Value> {
        let (actions, rec) = metadata(page)?;
        check(
            number(&page["returned_bytes"])? > 0
                && number(&page["next_offset_bytes"])? == self.data.len()
                && self.data.len() < self.total,
            "fragment nonprogress or contradictory continuation",
        )?;
        check(
            actions.len() == 1 && rec == Some(0),
            "unique byte continuation required",
        )?;
        let params = ready(&actions[0])?;
        check(
            keys(params)?
                == field_set(
                    "program_id program_revision after_input limit offset_bytes limit_bytes representation_digest",
                ),
            "byte continuation params",
        )?;
        let source = self.source.as_ref().ok_or("source pin missing")?;
        let after = number(&params["after_input"])?;
        let window = number(&params["limit_bytes"])?;
        check(
            params["program_id"] == source["program_id"]
                && params["program_revision"] == source["program_revision"]
                && number(&params["limit"])? == request.limit
                && after <= i64::MAX as usize
                && request.after.is_none_or(|a| a == after),
            "query selectors changed",
        )?;
        check(
            self.normalized_after.is_none_or(|a| a == after)
                && self.window.is_none_or(|w| w == window)
                && (1..=4096).contains(&window),
            "query normalization or window changed",
        )?;
        check(
            number(&params["offset_bytes"])? == self.data.len()
                && params["representation_digest"].as_str() == self.digest.as_deref(),
            "byte continuation pins changed",
        )?;
        self.normalized_after = Some(after);
        self.window = Some(window);
        Ok(actions[0]["arguments"].clone())
    }
    fn finish(&self) -> ReadResult<Value> {
        check(
            self.data.len() == self.total
                && self.digest.as_deref()
                    == Some(format!("{:x}", Sha256::digest(&self.data)).as_str()),
            "EOF length or SHA mismatch",
        )?;
        serde_json::from_slice(&self.data).map_err(|e| e.to_string())
    }
}
pub async fn read_program_query(
    mut raw_call: impl AsyncFnMut(Value) -> Value,
    params: Value,
) -> ReadResult<QueryRead> {
    let request = Request::parse(&params)?;
    let initial = json!({"route":"program.get","params":params});
    let mut args = initial.clone();
    let mut bytes = Bytes::default();
    let mut maximum_mcp_bytes = 0;
    for pages in 1..=8192 {
        let (raw_page, size) = envelope(&raw_call(args.clone()).await)?;
        maximum_mcp_bytes = maximum_mcp_bytes.max(size);
        let fragment = raw_page["kind"] == "fragment";
        let value = if fragment {
            bytes.append(&raw_page, &request)?;
            if !raw_page["next_offset_bytes"].is_null() {
                args = bytes.next(&raw_page, &request)?;
                continue;
            }
            bytes.finish()?
        } else {
            check(
                bytes.source.is_none() && pages == 1,
                "fragment became inline",
            )?;
            raw_page.clone()
        };
        let typed = validation::logical(&value, &request, &bytes)?;
        let (actions, rec) = metadata(&raw_page)?;
        validation::terminal(&typed, &request, &bytes, &actions, rec)?;
        return Ok(QueryRead {
            value,
            provenance: Provenance {
                initial_query: initial,
                source: bytes.source,
                representation_digest: bytes.digest,
                pages,
                maximum_mcp_bytes,
                terminal_actions: actions,
                terminal_recommended_action: rec,
            },
        });
    }
    Err("Program byte page bound exceeded".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{Program, ProgramStep};
    pub(super) fn fixture(one: bool) -> Value {
        let mut program = Program::draft(Uuid::from_u128(1), Uuid::from_u128(2), 0);
        program.input_cursor = 1;
        program.latest_input = 3;
        program.current_step = ProgramStep::Compose;
        let inputs: Vec<Value> = (2..=if one { 2 } else { 3 }).map(|sequence| json!({"id":Uuid::from_u128(sequence as u128 + 10),"sequence":sequence,"request_id":Uuid::from_u128(sequence as u128 + 20),"session_id":Uuid::from_u128(30),"input":format!("whole UTF-8 🙂 {sequence}")})).collect();
        let next = if one { json!(2) } else { Value::Null };
        let mut actions = vec![
            json!({"kind":"ready_call","tool":"help","arguments":{"mode":"describe","method":"tectd-program"}}),
        ];
        if one {
            actions.push(json!({"kind":"ready_call","tool":"query","arguments":{"route":"program.get","params":{"program_id":program.id,"after_input":2,"limit":25}}}));
        }
        actions.push(json!({"kind":"needs_input","tool":"command","arguments":{"route":"program.save","params":{"program_id":program.id,"revision":1,"input_cursor":if one { 2 } else { 3 } }},"input":{}}));
        json!({"program":program,"inputs":inputs,"next_after_input":next,"actions":actions,"recommended_action":0})
    }
    pub(super) fn raw(page: Value) -> Value {
        json!({"result":{"isError":false,"content":[{"type":"text","text":"Program fixture"},{"type":"text","text":page.to_string()},{"type":"text","text":FOOTER}]}})
    }
    pub(super) fn fragments(one: bool, after: usize, limit: usize) -> Vec<Value> {
        let mut value = fixture(one);
        value.as_object_mut().unwrap().remove("actions");
        value.as_object_mut().unwrap().remove("recommended_action");
        let text = value.to_string();
        let split = text.find("whole").unwrap();
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        let byte_args = json!({"route":"program.get","params":{"program_id":Uuid::from_u128(1),"program_revision":1,"after_input":after,"limit":limit,"offset_bytes":split,"limit_bytes":4096,"representation_digest":digest}});
        let terminal = if one {
            json!([{"kind":"ready_call","tool":"query","arguments":{"route":"program.get","params":{"program_id":Uuid::from_u128(1),"program_revision":1,"after_input":2,"limit":limit}}}])
        } else {
            json!([])
        };
        let page = |offset, part: &str, next: Value, actions: Value, rec: Value| json!({"kind":"fragment","format":"json","encoding":"utf-8","source":{"program_id":Uuid::from_u128(1),"program_revision":1},"representation_digest":digest,"total_bytes":text.len(),"offset_bytes":offset,"returned_bytes":part.len(),"text":part,"next_offset_bytes":next,"actions":actions,"recommended_action":rec});
        vec![
            raw(page(
                0,
                &text[..split],
                json!(split),
                json!([{"kind":"ready_call","tool":"query","arguments":byte_args}]),
                json!(0),
            )),
            raw(page(
                split,
                &text[split..],
                Value::Null,
                terminal,
                if one { json!(0) } else { Value::Null },
            )),
        ]
    }
    pub(super) fn alter(raw: &mut Value, f: impl FnOnce(&mut Value)) {
        let mut page: Value =
            serde_json::from_str(raw["result"]["content"][1]["text"].as_str().unwrap()).unwrap();
        f(&mut page);
        raw["result"]["content"][1]["text"] = json!(page.to_string());
    }
    pub(super) async fn collect(pages: Vec<Value>, params: Value) -> ReadResult<QueryRead> {
        let mut pages = pages.into_iter();
        read_program_query(async |_| pages.next().unwrap(), params).await
    }
    #[tokio::test]
    async fn exact_initial_query_and_whole_inputs_survive_inline_and_fragments() {
        let params =
            json!({"program_id":Uuid::from_u128(1),"after_input":1,"limit":1,"program_revision":1});
        let inline = collect(vec![raw(fixture(true))], params.clone())
            .await
            .unwrap();
        assert_eq!(inline.value["inputs"][0]["input"], "whole UTF-8 🙂 2");
        assert_eq!(
            inline.provenance.initial_query,
            json!({"route":"program.get","params":params})
        );
        assert!(inline.provenance.source.is_none());
        assert_eq!(inline.provenance.pages, 1);
        let pages = fragments(true, 1, 1);
        let advertised: Value =
            serde_json::from_str(pages[0]["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        let mut observed = Vec::new();
        let mut pages = pages.into_iter();
        let read = read_program_query(
            async |args| {
                observed.push(args);
                pages.next().unwrap()
            },
            params.clone(),
        )
        .await
        .unwrap();
        assert_eq!(observed[0], json!({"route":"program.get","params":params}));
        assert_eq!(observed[1], advertised["actions"][0]["arguments"]);
        assert_eq!(observed[1]["params"]["after_input"], 1);
        assert_eq!(observed[1]["params"]["limit"], 1);
        assert_eq!(read.value["inputs"], inline.value["inputs"]);
        assert!(read.value.get("actions").is_none());
        assert_eq!(read.provenance.pages, 2);
        assert!(read.provenance.maximum_mcp_bytes <= 8192);
        assert_eq!(
            read.provenance.source,
            Some(json!({"program_id":Uuid::from_u128(1),"program_revision":1}))
        );
        assert!(read.provenance.representation_digest.is_some());
        assert_eq!(read.provenance.terminal_recommended_action, Some(0));
        assert_eq!(
            read.provenance.terminal_actions[0]["arguments"]["params"]["after_input"],
            2
        );
    }
    #[tokio::test]
    async fn omitted_cursor_is_verified_against_decoded_program() {
        let params = json!({"program_id":Uuid::from_u128(1)});
        let read = collect(fragments(false, 1, 25), params.clone())
            .await
            .unwrap();
        assert_eq!(read.provenance.initial_query["params"], params);
        assert_eq!(read.value["inputs"].as_array().unwrap().len(), 2);
        assert!(read.provenance.terminal_actions.is_empty());
        assert_eq!(read.provenance.terminal_recommended_action, None);
        assert!(collect(fragments(false, 0, 25), params).await.is_err());
    }
}
