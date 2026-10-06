//! Explicit Help Describe reads; callbacks return actual raw MCP envelopes.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ops::AsyncFnMut};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 8192;
const FOOTER: &str = "Follow the rules from workspace.open or help {\"text\":\"response-rules\"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real.";
type ReadResult<T> = Result<T, String>;

pub struct Read {
    pub value: Value,
    pub initial_arguments: Value,
    pub source: Option<Value>,
    pub representation_digest: Option<String>,
    pub pages: usize,
    pub maximum_envelope_bytes: usize,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Value,
}

fn require(condition: bool, message: &str) -> ReadResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn selectors(initial: &Value) -> ReadResult<Value> {
    let allowed = [
        json!({"mode":"describe","method":"tectd-program"}),
        json!({"mode":"describe","tool":"command","route":"program.begin"}),
        json!({"mode":"describe","tool":"command","route":"knowledge.change_phase_complete"}),
        json!({"mode":"describe","tool":"query","route":"knowledge.search"}),
        json!({"mode":"describe","tool":"query","route":"slice.pipelines"}),
        json!({"mode":"describe","tool":"command","route":"scope.candidates.begin"}),
    ];
    require(
        allowed.contains(initial),
        "unsupported canonical Help Describe selectors",
    )?;
    Ok(json!({"tool":"help","selectors":initial}))
}

fn payload(raw: &Value) -> ReadResult<(Value, usize)> {
    require(raw.get("error").is_none(), "MCP protocol error")?;
    let result = raw
        .get("result")
        .filter(|v| v.is_object())
        .ok_or("MCP result missing")?;
    require(result["isError"] != true, "Help returned a refusal")?;
    require(
        result.get("structuredContent").is_none(),
        "unexpected structured content",
    )?;
    let size = serde_json::to_vec(result).map_err(|e| e.to_string())?.len();
    require(size <= 8192, "raw MCP Help result exceeds envelope budget")?;
    let blocks = result["content"].as_array().ok_or("MCP content missing")?;
    require(
        blocks.len() == 3 && blocks.iter().all(|b| b["type"] == "text"),
        "invalid MCP text blocks",
    )?;
    require(
        blocks[0]["text"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 2000),
        "invalid MCP intro",
    )?;
    require(blocks[2]["text"] == FOOTER, "MCP rules footer changed")?;
    let value: Value = serde_json::from_str(blocks[1]["text"].as_str().ok_or("JSON text missing")?)
        .map_err(|e| e.to_string())?;
    require(value.is_object(), "Help payload must be an object")?;
    Ok((value, size))
}

#[derive(Default)]
struct Collection {
    bytes: Vec<u8>,
    digest: Option<String>,
    total: Option<usize>,
    limit: Option<usize>,
}

impl Collection {
    fn append(&mut self, page: &Value, source: &Value) -> ReadResult<()> {
        require(
            page["format"] == "json" && page["encoding"] == "utf-8",
            "invalid Help fragment encoding",
        )?;
        require(&page["source"] == source, "Help selector source changed")?;
        let digest = page["representation_digest"]
            .as_str()
            .ok_or("Help digest missing")?;
        require(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid Help digest",
        )?;
        let total = count(page, "total_bytes")?;
        require(total <= MAX_BYTES, "Help representation exceeds byte bound")?;
        if let Some(expected) = &self.digest {
            require(
                expected == digest && self.total == Some(total),
                "Help digest or total changed",
            )?;
        } else {
            self.digest = Some(digest.to_owned());
            self.total = Some(total);
        }
        let offset = count(page, "offset_bytes")?;
        let returned = count(page, "returned_bytes")?;
        let text = page["text"].as_str().ok_or("Help UTF-8 fragment missing")?;
        require(offset == self.bytes.len(), "noncontiguous Help fragment")?;
        require(
            returned == text.len()
                && returned <= 4096
                && self.limit.is_none_or(|limit| returned <= limit),
            "Help UTF-8 byte count or window invalid",
        )?;
        let end = offset
            .checked_add(returned)
            .ok_or("Help byte range overflow")?;
        require(end <= total, "Help fragment exceeds total")?;
        self.bytes.extend_from_slice(text.as_bytes());
        Ok(())
    }

    fn continuation(&mut self, page: &Value, initial: &Value) -> ReadResult<Value> {
        let next = count(page, "next_offset_bytes")?;
        require(
            count(page, "returned_bytes")? > 0
                && next == self.bytes.len()
                && next < self.total.ok_or("Help total missing")?,
            "Help fragment failed to advance",
        )?;
        let actions = page["actions"].as_array().ok_or("Help actions missing")?;
        require(
            actions.len() == 1 && page["recommended_action"] == 0,
            "Help continuation must be uniquely recommended",
        )?;
        let action = &actions[0];
        require(
            keys(action)? == BTreeSet::from(["kind", "tool", "arguments"]),
            "Help continuation shape changed",
        )?;
        require(
            action["kind"] == "ready_call" && action["tool"] == "help",
            "Help continuation must be Ready Help",
        )?;
        let args = &action["arguments"];
        let mut expected_keys = keys(initial)?;
        expected_keys.extend(["offset_bytes", "limit_bytes", "representation_digest"]);
        require(
            keys(args)? == expected_keys,
            "Help continuation argument keys changed",
        )?;
        for (key, value) in initial.as_object().unwrap() {
            require(&args[key] == value, "Help continuation selector changed")?;
        }
        require(
            count(args, "offset_bytes")? == next
                && args["representation_digest"].as_str() == self.digest.as_deref(),
            "Help continuation pins changed",
        )?;
        let limit = count(args, "limit_bytes")?;
        require(
            (1..=4096).contains(&limit),
            "invalid Help continuation limit",
        )?;
        if let Some(expected) = self.limit {
            require(limit == expected, "Help continuation limit changed")?;
        } else {
            self.limit = Some(limit);
        }
        Ok(args.clone())
    }

    fn finish(&self) -> ReadResult<Value> {
        require(self.total == Some(self.bytes.len()), "premature Help EOF")?;
        require(
            self.digest.as_deref() == Some(format!("{:x}", Sha256::digest(&self.bytes)).as_str()),
            "assembled Help digest mismatch",
        )?;
        let value: Value = serde_json::from_slice(&self.bytes).map_err(|e| e.to_string())?;
        require(
            value.is_object(),
            "complete Help representation must be object",
        )?;
        Ok(value)
    }
}

fn count(value: &Value, field: &str) -> ReadResult<usize> {
    usize::try_from(
        value[field]
            .as_u64()
            .ok_or_else(|| format!("invalid Help {field}"))?,
    )
    .map_err(|e| e.to_string())
}

fn keys(value: &Value) -> ReadResult<BTreeSet<&str>> {
    Ok(value
        .as_object()
        .ok_or("Help object missing")?
        .keys()
        .map(String::as_str)
        .collect())
}

fn completed_read(
    value: Value,
    terminal: &Value,
    initial: Value,
    collection: Collection,
    source: Value,
    pages: usize,
    maximum_envelope_bytes: usize,
) -> ReadResult<Read> {
    let actions = terminal
        .get("actions")
        .and_then(Value::as_array)
        .ok_or("Help EOF actions missing")?;
    require(
        actions.is_empty()
            && terminal
                .get("recommended_action")
                .is_some_and(Value::is_null),
        "Help EOF delivery metadata changed",
    )?;
    Ok(Read {
        value,
        initial_arguments: initial,
        source: collection.digest.as_ref().map(|_| source),
        representation_digest: collection.digest,
        pages,
        maximum_envelope_bytes,
        terminal_actions: actions.clone(),
        terminal_recommended_action: terminal["recommended_action"].clone(),
    })
}

pub async fn describe(
    mut raw_call: impl AsyncFnMut(Value) -> Value,
    initial: Value,
) -> ReadResult<Read> {
    let source = selectors(&initial)?;
    let mut current = initial.clone();
    let mut collection = Collection::default();
    let mut maximum_envelope_bytes = 0;
    for pages in 1..=MAX_PAGES {
        let (page, size) = payload(&raw_call(current.clone()).await)?;
        maximum_envelope_bytes = maximum_envelope_bytes.max(size);
        if page["kind"] != "fragment" {
            require(
                collection.digest.is_none(),
                "Help fragment changed representation",
            )?;
            return completed_read(
                page.clone(),
                &page,
                initial,
                collection,
                source,
                pages,
                maximum_envelope_bytes,
            );
        }
        collection.append(&page, &source)?;
        let next = page
            .get("next_offset_bytes")
            .ok_or("Help EOF field missing")?;
        if next.is_null() {
            let value = collection.finish()?;
            return completed_read(
                value,
                &page,
                initial,
                collection,
                source,
                pages,
                maximum_envelope_bytes,
            );
        }
        current = collection.continuation(&page, &initial)?;
    }
    Err("Help page bound exceeded".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial() -> Value {
        json!({"mode":"describe","method":"tectd-program"})
    }
    // Synthetic pure transport cases; these are not native acceptance evidence.
    fn envelope(page: Value) -> Value {
        json!({"result":{"isError":false,"content":[{"type":"text","text":"Fixture Help"},
            {"type":"text","text":page.to_string()},{"type":"text","text":FOOTER}]}})
    }
    fn fragments() -> (Value, Vec<Value>) {
        let value = json!({"method":"tectd-program","body":"UTF-8 café method"});
        let text = value.to_string();
        let split = text.find("method").unwrap();
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        let args = json!({"mode":"describe","method":"tectd-program","offset_bytes":split,
            "limit_bytes":4096,"representation_digest":digest});
        let page = |offset, part: &str, next: Value, actions: Value, recommended: Value| {
            json!({"kind":"fragment","format":"json","encoding":"utf-8",
                "source":{"tool":"help","selectors":initial()},"representation_digest":digest,
                "total_bytes":text.len(),"offset_bytes":offset,"returned_bytes":part.len(),
                "text":part,"next_offset_bytes":next,"actions":actions,"recommended_action":recommended})
        };
        let first = page(
            0,
            &text[..split],
            json!(split),
            json!([{"kind":"ready_call","tool":"help","arguments":args}]),
            json!(0),
        );
        let last = page(split, &text[split..], Value::Null, json!([]), Value::Null);
        (value, vec![envelope(first), envelope(last)])
    }
    async fn collect(pages: Vec<Value>) -> ReadResult<Read> {
        let mut pages = pages.into_iter();
        describe(async |_| pages.next().expect("fixture page"), initial()).await
    }

    #[tokio::test]
    async fn describe_collects_exact_bytes_and_keeps_terminal_delivery_separate() {
        let (value, pages) = fragments();
        let expected_next: Value =
            serde_json::from_str(pages[0]["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        let mut pages = pages.into_iter();
        let mut observed = Vec::new();
        let read = describe(
            async |args| {
                observed.push(args);
                pages.next().unwrap()
            },
            initial(),
        )
        .await
        .unwrap();
        assert_eq!(
            observed,
            [initial(), expected_next["actions"][0]["arguments"].clone()]
        );
        assert_eq!(read.value, value);
        assert_eq!(read.pages, 2);
        assert_eq!(read.initial_arguments, initial());
        assert_eq!(
            read.source,
            Some(json!({"tool":"help","selectors":initial()}))
        );
        assert!(read.representation_digest.is_some());
        assert!(read.maximum_envelope_bytes <= 8192);
        assert!(read.value.get("actions").is_none());
        assert!(read.terminal_actions.is_empty());
        assert!(read.terminal_recommended_action.is_null());
        let inline = envelope(
            json!({"method":"tectd-program","body":"inline","actions":[],"recommended_action":null}),
        );
        let read = collect(vec![inline]).await.unwrap();
        assert_eq!(read.value["body"], "inline");
        assert_eq!(read.pages, 1);
        assert!(read.representation_digest.is_none());
    }

    fn alter(raw: &mut Value, change: impl FnOnce(&mut Value)) {
        let mut page: Value =
            serde_json::from_str(raw["result"]["content"][1]["text"].as_str().unwrap()).unwrap();
        change(&mut page);
        raw["result"]["content"][1]["text"] = json!(page.to_string());
    }
    #[tokio::test]
    async fn describe_rejects_changed_selector_pins_and_nonprogress() {
        for change in ["selector", "pin", "progress", "offset"] {
            let (_, mut pages) = fragments();
            alter(&mut pages[0], |page| match change {
                "selector" => page["actions"][0]["arguments"]["method"] = json!("other"),
                "pin" => {
                    page["actions"][0]["arguments"]["representation_digest"] = json!("0".repeat(64))
                }
                "progress" => {
                    page["text"] = json!("");
                    page["returned_bytes"] = json!(0);
                    page["next_offset_bytes"] = json!(0);
                    page["actions"][0]["arguments"]["offset_bytes"] = json!(0);
                }
                _ => page["next_offset_bytes"] = json!(1),
            });
            assert!(collect(pages).await.is_err(), "{change}");
        }
    }
    #[tokio::test]
    async fn describe_rejects_premature_eof_and_continuation_at_total() {
        let (_, mut pages) = fragments();
        alter(&mut pages[0], |page| {
            page["next_offset_bytes"] = Value::Null;
            page["actions"] = json!([]);
            page["recommended_action"] = Value::Null;
        });
        assert!(collect(pages).await.is_err(), "premature EOF");

        let (value, mut pages) = fragments();
        let text = value.to_string();
        alter(&mut pages[0], |page| {
            page["text"] = json!(text);
            page["returned_bytes"] = json!(text.len());
            page["next_offset_bytes"] = json!(text.len());
            page["actions"][0]["arguments"]["offset_bytes"] = json!(text.len());
        });
        alter(&mut pages[1], |page| {
            page["offset_bytes"] = json!(text.len());
            page["text"] = json!("");
            page["returned_bytes"] = json!(0);
        });
        let mut pages = pages.into_iter();
        let mut calls = 0;
        let result = describe(
            async |_| {
                calls += 1;
                pages.next().unwrap()
            },
            initial(),
        )
        .await;
        assert!(result.is_err(), "continuation at total bytes");
        assert_eq!(calls, 1, "contradictory continuation must not execute");
    }
    #[tokio::test]
    async fn describe_rejects_digest_and_eof_corruption() {
        for change in ["digest", "total_drift", "missing_eof", "actions"] {
            let (_, mut pages) = fragments();
            alter(&mut pages[1], |page| match change {
                "digest" => {
                    page["text"] = json!(page["text"].as_str().unwrap().replace("method", "Method"))
                }
                "total_drift" => page["total_bytes"] = json!(999),
                "missing_eof" => {
                    page.as_object_mut().unwrap().remove("next_offset_bytes");
                }
                _ => page["actions"] = json!([{}]),
            });
            assert!(collect(pages).await.is_err(), "{change}");
        }
    }
}
