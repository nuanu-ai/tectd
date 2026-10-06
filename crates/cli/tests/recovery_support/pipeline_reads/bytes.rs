use super::{FixtureResult, Mcp, ROUTE, require};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 8192;

pub struct ReadProvenance {
    pub initial_action: Option<Value>,
    pub initial_query_arguments: Value,
    pub source: Value,
    pub representation_digest: Option<String>,
    pub pages: usize,
    pub maximum_envelope_bytes: usize,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Value,
}
pub struct ResolvedRead {
    pub value: Value,
    pub provenance: ReadProvenance,
}

fn provenance(
    advertised: Option<&Value>,
    arguments: &Value,
    source: Value,
    digest: Option<String>,
    pages: usize,
    maximum_envelope_bytes: usize,
    terminal: (&Value, bool),
) -> FixtureResult<ReadProvenance> {
    let (terminal, output) = terminal;
    eof_for(terminal, &source, output)?;
    Ok(ReadProvenance {
        initial_action: advertised.cloned(),
        initial_query_arguments: arguments.clone(),
        source,
        representation_digest: digest,
        pages,
        maximum_envelope_bytes,
        terminal_actions: terminal["actions"].as_array().unwrap().clone(),
        terminal_recommended_action: terminal["recommended_action"].clone(),
    })
}

fn pins(params: &Value) -> FixtureResult<Value> {
    let run_id = params.get("run_id").ok_or("read run pin missing")?;
    let mut source = json!({"run_id":run_id});
    let fields: &[&str] = match params["view"].as_str() {
        Some("snapshot") => &["definition_digest"],
        Some("phase_contract") => &["definition_digest", "phase_id"],
        Some("details") => &["run_revision", "section"],
        _ => return Err("unsupported pipeline read view".into()),
    };
    let mut keys = BTreeSet::from(["run_id", "view"]);
    for field in fields {
        source[*field] = params
            .get(*field)
            .ok_or("read identity pin missing")?
            .clone();
        keys.insert(*field);
    }
    if let Some(refresh) = params.get("refresh") {
        require(refresh == false, "read cannot request refresh")?;
        keys.insert("refresh");
    }
    require(
        params
            .as_object()
            .ok_or("read params missing")?
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == keys,
        "unexpected initial pipeline read parameters",
    )?;
    Ok(source)
}
fn verify_value(value: &Value, source: &Value) -> FixtureResult<()> {
    for (key, expected) in source.as_object().unwrap() {
        let actual = if key == "phase_id" {
            &value["phase"]["id"]
        } else {
            &value[key]
        };
        require(actual == expected, "read DTO pin differs from source")?;
    }
    Ok(())
}
fn eof(page: &Value) -> FixtureResult<()> {
    require(
        page.get("actions")
            .is_some_and(|v| v.as_array().is_some_and(Vec::is_empty))
            && page.get("recommended_action").is_some_and(Value::is_null),
        "read EOF changed semantic metadata",
    )
}
fn output_pins(params: &Value) -> FixtureResult<Value> {
    require(params["view"] == "output", "output view missing")?;
    require(
        params
            .as_object()
            .ok_or("output params missing")?
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["run_id", "view", "output_id", "digest"]),
        "unexpected initial output parameters",
    )?;
    Ok(params.clone())
}
fn eof_for(page: &Value, source: &Value, output: bool) -> FixtureResult<()> {
    if !output {
        return eof(page);
    }
    let actions = page
        .get("actions")
        .and_then(Value::as_array)
        .ok_or("output EOF actions missing")?;
    require(
        actions.len() == 1 && page.get("recommended_action") == Some(&json!(0)),
        "output EOF recommendation changed",
    )?;
    let action = &actions[0];
    require(
        action.as_object().is_some_and(|object| object.len() == 3)
            && action["kind"] == "ready_call"
            && action["tool"] == "query"
            && action["arguments"] == json!({"route":ROUTE,"params":{"run_id":source["run_id"]}}),
        "output EOF must retain only its actual Current action",
    )
}
fn verify_read_value(value: &Value, source: &Value, output: bool) -> FixtureResult<()> {
    if !output {
        return verify_value(value, source);
    }
    require(
        value["run_id"] == source["run_id"]
            && value["id"] == source["output_id"]
            && value["digest"] == source["digest"],
        "output DTO pins differ from source",
    )
}

fn continuation(page: &Value, initial: &Value, end: usize) -> FixtureResult<Value> {
    let actions = page["actions"]
        .as_array()
        .ok_or("fragment actions missing")?;
    require(
        actions.len() == 1 && page["recommended_action"] == 0,
        "ambiguous fragment continuation",
    )?;
    let next = &actions[0];
    require(
        next["kind"] == "ready_call"
            && next["tool"] == "query"
            && next["arguments"]["route"] == ROUTE,
        "continuation query changed",
    )?;
    let mut expected = initial.clone();
    expected["offset_bytes"] = json!(end);
    expected["limit_bytes"] = json!(4096);
    expected["representation_digest"] = page["representation_digest"].clone();
    let actual = &next["arguments"]["params"];
    if initial.get("refresh").is_none() && actual.get("refresh").is_some() {
        require(actual["refresh"] == false, "continuation refresh changed")?;
        expected["refresh"] = json!(false);
    }
    require(
        actual == &expected,
        "continuation changed pinned parameters or window",
    )?;
    Ok(next["arguments"].clone())
}

pub(super) async fn read(client: &mut Mcp, action: &Value) -> FixtureResult<ResolvedRead> {
    require(
        action["kind"] == "ready_call"
            && action["tool"] == "query"
            && action["arguments"]["route"] == ROUTE,
        "read is not an actual Ready pipeline query",
    )?;
    read_arguments(client, &action["arguments"], Some(action), false).await
}

pub(super) async fn read_query(client: &mut Mcp, arguments: &Value) -> FixtureResult<ResolvedRead> {
    require(arguments["route"] == ROUTE, "explicit read route changed")?;
    read_arguments(client, arguments, None, false).await
}

pub(super) async fn read_output(
    client: &mut Mcp,
    arguments: &Value,
) -> FixtureResult<ResolvedRead> {
    require(arguments["route"] == ROUTE, "explicit output route changed")?;
    read_arguments(client, arguments, None, true).await
}

async fn read_arguments(
    client: &mut Mcp,
    arguments: &Value,
    advertised: Option<&Value>,
    output: bool,
) -> FixtureResult<ResolvedRead> {
    let initial = &arguments["params"];
    let source = if output {
        output_pins(initial)?
    } else {
        pins(initial)?
    };
    let mut current = arguments.clone();
    let mut bytes = Vec::new();
    let mut digest: Option<String> = None;
    let mut total = None;
    let mut keys = None;
    let mut maximum_envelope_bytes = 0;
    for pages in 1..=MAX_PAGES {
        let raw = client
            .exchange("tools/call", json!({"name":"query","arguments":current}))
            .await;
        require(
            raw.get("error").is_none() && raw["result"]["isError"] != true,
            "advertised pipeline read refused",
        )?;
        let size = serde_json::to_vec(&raw["result"])
            .map_err(|_| "invalid raw envelope")?
            .len();
        require(size <= 8192, "raw MCP tool envelope exceeds budget")?;
        maximum_envelope_bytes = maximum_envelope_bytes.max(size);
        let page = super::super::tool_payload(&raw);
        if page["kind"] != "fragment" {
            require(
                pages == 1 && bytes.is_empty(),
                "fragment became ordinary DTO",
            )?;
            verify_read_value(&page, &source, output)?;
            eof_for(&page, &source, output)?;
            return Ok(ResolvedRead {
                value: page.clone(),
                provenance: provenance(
                    advertised,
                    arguments,
                    source,
                    None,
                    pages,
                    maximum_envelope_bytes,
                    (&page, output),
                )?,
            });
        }
        require(
            page["format"] == "json" && page["encoding"] == "utf-8" && page["source"] == source,
            "fragment format or source changed",
        )?;
        let page_keys = page
            .as_object()
            .ok_or("fragment object missing")?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let page_digest = page["representation_digest"]
            .as_str()
            .ok_or("fragment digest missing")?;
        require(
            page_digest.len() == 64
                && page_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid fragment digest",
        )?;
        let page_total = usize::try_from(
            page["total_bytes"]
                .as_u64()
                .ok_or("fragment total missing")?,
        )
        .map_err(|_| "total overflow")?;
        require(
            (1..=MAX_BYTES).contains(&page_total),
            "fixture representation exceeds byte bound",
        )?;
        if pages == 1 {
            digest = Some(page_digest.into());
            total = Some(page_total);
            keys = Some(page_keys);
        } else {
            require(
                digest.as_deref() == Some(page_digest)
                    && total == Some(page_total)
                    && keys.as_ref() == Some(&page_keys),
                "fragment digest total or keyset changed",
            )?;
        }
        let offset = usize::try_from(page["offset_bytes"].as_u64().ok_or("offset missing")?)
            .map_err(|_| "offset overflow")?;
        let text = page["text"].as_str().ok_or("fragment UTF-8 text missing")?;
        let returned = usize::try_from(
            page["returned_bytes"]
                .as_u64()
                .ok_or("byte count missing")?,
        )
        .map_err(|_| "count overflow")?;
        require(
            offset == bytes.len() && returned == text.len() && returned <= 4096,
            "fragment byte accounting changed",
        )?;
        let end = offset
            .checked_add(returned)
            .ok_or("fragment offset overflow")?;
        require(end <= page_total, "fragment exceeds representation")?;
        bytes.extend_from_slice(text.as_bytes());
        let next = page.get("next_offset_bytes").ok_or("EOF marker missing")?;
        if next.is_null() {
            require(
                end == page_total && format!("{:x}", Sha256::digest(&bytes)) == page_digest,
                "premature EOF or representation SHA mismatch",
            )?;
            eof_for(&page, &source, output)?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "assembled representation is not JSON")?;
            require(value.is_object(), "assembled DTO is not an object")?;
            verify_read_value(&value, &source, output)?;
            return Ok(ResolvedRead {
                value,
                provenance: provenance(
                    advertised,
                    arguments,
                    source,
                    digest,
                    pages,
                    maximum_envelope_bytes,
                    (&page, output),
                )?,
            });
        }
        require(
            returned > 0 && next.as_u64() == Some(end as u64) && end < page_total,
            "fragment continuation failed to advance",
        )?;
        current = continuation(&page, initial, end)?;
    }
    Err("fixture representation exceeds page bound".into())
}

#[cfg(test)]
pub(super) fn test_pins(params: &Value) -> FixtureResult<Value> {
    pins(params)
}
#[cfg(test)]
pub(super) fn test_continuation(page: &Value, initial: &Value, end: usize) -> FixtureResult<Value> {
    continuation(page, initial, end)
}

#[cfg(test)]
pub(super) fn test_provenance(
    action: Option<&Value>,
    arguments: &Value,
    terminal: &Value,
) -> FixtureResult<ReadProvenance> {
    provenance(
        action,
        arguments,
        pins(&arguments["params"])?,
        Some("a".repeat(64)),
        2,
        8192,
        (terminal, false),
    )
}
