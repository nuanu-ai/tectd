//! Explicit Program page reads; compact mutation receipts remain unchanged.
#[path = "program/bytes.rs"]
mod bytes;
use super::super::Mcp;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tect_domain::{ProgramPage, ProgramStatus, ProgramStep};
use uuid::Uuid;

pub struct ProgramReadProvenance {
    pub initial_action: Value,
    pub initial_query: Value,
    pub raw_exchanges: Vec<Value>,
    pub representation_digest: Option<String>,
    pub maximum_mcp_bytes: usize,
    pub pages: usize,
    pub terminal_actions: Vec<Value>,
    pub terminal_recommended_action: Option<usize>,
    pub next_after_input: Option<i64>,
}
pub struct ResolvedProgramPage {
    pub value: Value,
    pub provenance: ProgramReadProvenance,
}
impl ResolvedProgramPage {
    pub fn program(&self) -> &Value {
        &self.value["program"]
    }
    pub fn inputs(&self) -> &[Value] {
        self.value["inputs"].as_array().unwrap()
    }
    pub fn next_after_input(&self) -> Option<i64> {
        self.provenance.next_after_input
    }
}
pub struct ProgramFixture {
    pub mutation: Value,
}
pub(super) fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("Program object")
        .keys()
        .map(String::as_str)
        .collect()
}
fn uuid(value: &Value) {
    let id = Uuid::parse_str(value.as_str().expect("Program UUID")).unwrap();
    assert!(!id.is_nil(), "Program UUID must not be nil");
}
pub(super) fn ready(action: &Value) -> &Value {
    assert_eq!(keys(action), BTreeSet::from(["kind", "tool", "arguments"]));
    assert_eq!(action["kind"], "ready_call");
    assert_eq!(action["tool"], "query");
    assert_eq!(
        keys(&action["arguments"]),
        BTreeSet::from(["route", "params"])
    );
    assert_eq!(action["arguments"]["route"], "program.get");
    &action["arguments"]["params"]
}
pub(super) fn metadata(value: &Value) -> (Vec<Value>, Option<usize>) {
    let actions = value["actions"]
        .as_array()
        .expect("actual Program actions")
        .clone();
    let rec = value
        .get("recommended_action")
        .expect("actual Program recommendation");
    let recommendation = if rec.is_null() {
        None
    } else {
        let index = usize::try_from(rec.as_u64().expect("nonnegative recommendation")).unwrap();
        assert!(index < actions.len());
        Some(index)
    };
    (actions, recommendation)
}
impl ProgramFixture {
    pub fn from_mutation(mutation: Value) -> Self {
        let header = &mutation["program"];
        assert_eq!(
            keys(header),
            BTreeSet::from([
                "id",
                "workspace_id",
                "revision",
                "status",
                "current_step",
                "input_cursor",
                "latest_input"
            ])
        );
        uuid(&header["id"]);
        uuid(&header["workspace_id"]);
        assert!(header["revision"].as_i64().is_some_and(|n| n > 0));
        let _: ProgramStatus = serde_json::from_value(header["status"].clone()).unwrap();
        let _: ProgramStep = serde_json::from_value(header["current_step"].clone()).unwrap();
        let cursor = header["input_cursor"].as_i64().unwrap();
        let latest = header["latest_input"].as_i64().unwrap();
        assert!(cursor >= 0 && latest >= cursor);
        let action = &mutation["field_destinations"]["full_program_and_original_inputs"];
        assert_eq!(
            ready(action),
            &json!({"program_id":header["id"], "program_revision":header["revision"], "after_input":0, "limit":25})
        );
        Self { mutation }
    }
    pub async fn read_page(&self, client: &mut Mcp) -> ResolvedProgramPage {
        let action =
            self.mutation["field_destinations"]["full_program_and_original_inputs"].clone();
        let mut read = bytes::read(client, action).await;
        verify(&read.value, &self.mutation["program"]);
        let next = read
            .value
            .get("next_after_input")
            .expect("actual next cursor");
        read.provenance.next_after_input = if next.is_null() {
            None
        } else {
            Some(next.as_i64().unwrap())
        };
        if read.provenance.representation_digest.is_some() {
            match read.provenance.next_after_input {
                None => {
                    assert!(read.provenance.terminal_actions.is_empty());
                    assert_eq!(read.provenance.terminal_recommended_action, None);
                }
                Some(cursor) => {
                    assert_eq!(read.provenance.terminal_actions.len(), 1);
                    let params = ready(&read.provenance.terminal_actions[0]);
                    assert_eq!(
                        params,
                        &json!({"program_id":self.mutation["program"]["id"], "program_revision":self.mutation["program"]["revision"], "after_input":cursor, "limit":25})
                    );
                    assert_eq!(read.provenance.terminal_recommended_action, Some(0));
                }
            }
        }
        read
    }
}
fn verify(value: &Value, header: &Value) {
    let expected = if value.get("actions").is_some() {
        BTreeSet::from([
            "program",
            "inputs",
            "next_after_input",
            "actions",
            "recommended_action",
        ])
    } else {
        BTreeSet::from(["program", "inputs", "next_after_input"])
    };
    assert_eq!(keys(value), expected, "actual ProgramPage fields");
    let typed: ProgramPage = serde_json::from_value(value.clone()).unwrap();
    for (key, pin) in header.as_object().unwrap() {
        assert_eq!(
            value["program"].get(key),
            Some(pin),
            "Program header pin {key}"
        );
    }
    assert!(typed.inputs.len() <= 25, "bounded original input page");
    let mut previous = 0;
    let mut ids = BTreeSet::new();
    for (input, raw) in typed.inputs.iter().zip(value["inputs"].as_array().unwrap()) {
        assert_eq!(
            keys(raw),
            BTreeSet::from(["id", "sequence", "request_id", "session_id", "input"])
        );
        for id in [input.id, input.request_id, input.session_id] {
            assert!(!id.is_nil());
        }
        assert!(ids.insert(input.id), "duplicate original input");
        assert!(input.sequence > previous && input.sequence <= typed.program.latest_input);
        previous = input.sequence;
    }
    if let Some(next) = typed.next_after_input {
        assert!(!typed.inputs.is_empty());
        assert_eq!(
            next, previous,
            "collection next cursor is last delivered input"
        );
        assert!(next < typed.program.latest_input);
    } else {
        assert_eq!(
            previous, typed.program.latest_input,
            "initial page reached latest input"
        );
    }
}
