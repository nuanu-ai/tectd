//! Candidate sets brought to where new user input is recorded, the inputs recorded,
//! and what the tests read back of them.
use crate::drafts::{self, Level, Shape, Target};
use crate::recovery_support::{Mcp, tool_payload};
use crate::support::{Stand, command};
use serde_json::{Value, json};
use std::path::Path;
use tect_domain::ProgramMode;
use uuid::Uuid;

pub const FIRST: &str = "Also show which saved setting the preview ignores.";
pub const SECOND: &str = "Keep the correction inside the preview renderer.";

pub fn route(level: Level) -> &'static str {
    match level {
        Level::Scopes => "scope.candidates.record_input",
        Level::Slices => "slice.candidates.input",
    }
}

pub fn word(source: &str, text: &str) -> Value {
    json!({"source":source,"text":text})
}

/// A candidate set that takes new input, and the set as its latest reply showed it.
pub struct Planned {
    pub target: Target,
    set: Value,
}

impl Planned {
    /// A set that has no saved draft.
    pub async fn bare(client: &mut Mcp, repo: &Path, level: Level) -> Self {
        let target = drafts::target(client, repo, level).await;
        let set = target.set().clone();
        Self { target, set }
    }

    /// A set whose draft of this shape is saved. Jev is not enabled yet, so the save
    /// fires no advice point.
    pub async fn drafted(client: &mut Mcp, repo: &Path, level: Level, shape: &Shape) -> Self {
        let mut planned = Self::bare(client, repo, level).await;
        let draft = planned.target.save(shape, Uuid::new_v4());
        planned.save(client, draft).await;
        planned
    }

    /// Save a draft through the level's save route and follow the set.
    pub async fn save(&mut self, client: &mut Mcp, params: Value) {
        let (saved, _) = command(client, self.target.level.save_route(), params).await;
        assert!(saved.get("advice").is_none(), "{saved}");
        self.follow(&saved);
    }

    /// Take the set as a command reply shows it, for the next call.
    pub fn follow(&mut self, reply: &Value) {
        self.set = reply["candidate_set"].clone();
        assert!(self.set["revision"].is_i64(), "{reply}");
    }

    pub fn set(&self) -> &Value {
        &self.set
    }

    pub fn scope_id(&self) -> Value {
        match self.target.level {
            Level::Scopes => Value::Null,
            Level::Slices => self.target.context["scope"]["id"].clone(),
        }
    }

    /// The record-input call for `text` under a request id, at the set's revision.
    pub fn input(&self, text: &str, request_id: Uuid) -> Value {
        let mut params = json!({"candidate_set_id":self.set["id"],
            "revision":self.set["revision"],"request_id":request_id,"input":text});
        if self.target.level == Level::Slices {
            params["scope_id"] = self.scope_id();
        }
        params
    }

    /// The source mark of the words recorded for this set itself.
    pub fn own_source(&self) -> &'static str {
        match self.target.level {
            Level::Scopes => "scope_candidate_input",
            Level::Slices => "slice_candidate_input",
        }
    }

    /// The user's words behind this set before any input a test records.
    pub fn first_words(&self) -> Vec<Value> {
        match self.target.level {
            Level::Scopes => vec![
                word("program_input", drafts::PROGRAM_WORDS),
                word("scope_candidate_input", drafts::SCOPE_WORDS),
            ],
            Level::Slices => vec![
                word("program_input", drafts::PROGRAM_WORDS),
                word("scope_candidate_input", drafts::OPENED_SCOPE_WORDS),
                word("slice_candidate_input", drafts::SLICE_WORDS),
            ],
        }
    }

    /// The whole `state` Jev is sent for `new_input` after these words, for a Program
    /// of this level. `candidates` is the saved draft as Jev reads it, if one is saved.
    pub fn state(
        &self,
        mode: ProgramMode,
        user_words: &[Value],
        new_input: &str,
        candidates: Option<Value>,
    ) -> Value {
        let mut state = json!({"level":self.target.level.name(),
            "mode":{"level":mode.as_str(),"description":mode.description()},
            "program":{"name":"Notification preview",
                "intent":"Correct preview behavior from demonstrated evidence",
                "basis":"The preview differs from saved settings",
                "boundaries":"Preview diagnosis and bounded correction",
                "constraints":"No deployment or adjacent notification work",
                "success":"The cause and correction are verified"},
            "user_words":user_words,"new_input":new_input});
        if self.target.level == Level::Slices {
            state["scope"] = json!({"title":"Preview diagnosis and correction decision",
                "outcome":"The cause is demonstrated and the correction path selected",
                "includes":["diagnosis","decision"],"excludes":["deployment"]});
        }
        if let Some(candidates) = candidates {
            state["candidates"] = candidates;
        }
        state
    }

    /// Every input text the store holds for this set, in recorded order.
    pub async fn recorded(&self, stand: &Stand) -> Vec<String> {
        let table = match self.target.level {
            Level::Scopes => "scope_candidate_inputs",
            Level::Slices => "slice_planning_inputs",
        };
        let id = Uuid::parse_str(self.set["id"].as_str().unwrap()).unwrap();
        sqlx::query_scalar(&format!(
            "SELECT input FROM {table} WHERE tenant_id=$1 AND candidate_set_id=$2 \
             ORDER BY sequence"
        ))
        .bind(stand.tenant)
        .bind(id)
        .fetch_all(&stand.pool)
        .await
        .unwrap()
    }
}

/// The names of the questions of one request to Jev.
pub fn asked(request: &Value) -> Vec<&str> {
    let questions = request["questions"].as_object().unwrap();
    questions.keys().map(String::as_str).collect()
}

/// The criteria one question of a request offers, `ABSTAIN` included, in name order.
pub fn criteria<'a>(request: &'a Value, question: &str) -> Vec<&'a str> {
    let criteria = request["questions"][question]["criteria"]
        .as_object()
        .unwrap();
    criteria.keys().map(String::as_str).collect()
}

/// The placement criteria of a level when no candidate can be named.
pub fn placement_without_candidates(level: Level) -> Vec<&'static str> {
    let mut placement = vec!["ABSTAIN", "new_candidate", "outside_program"];
    if level == Level::Slices {
        placement.push("separate_scope");
    }
    placement
}

/// The `intake` rows of the journal through the audit route, newest first, without bodies.
pub async fn audit(client: &mut Mcp) -> Value {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"query","arguments":{"route":"workspace.advisory.audit",
                "params":{"limit":10,"point":"intake","bodies":false}}}),
        )
        .await;
    tool_payload(&response)
}
