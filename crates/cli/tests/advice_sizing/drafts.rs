//! Workspaces brought to the point where a draft can be saved, and the drafts saved.
use crate::planning::{ready_program, ready_source_candidate, route};
use crate::recovery_support::Mcp;
use crate::recovery_support::candidate_reads::{CandidateFixture, read_ready_json};
use crate::recovery_support::native_reads::{ScopeOpenFixture, SlicePlanningFixture};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

/// What the shared fixtures record as the Program narrative and as the planning
/// request of the Scope candidate set a Scope is opened from.
pub const PROGRAM_WORDS: &str =
    "Diagnose the incorrect preview, then select the smallest correction.";
pub const OPENED_SCOPE_WORDS: &str =
    "Open one native Scope for diagnosis and its result-driven correction decision.";
pub const SCOPE_WORDS: &str = "Plan Scopes for the preview diagnosis and its correction.";
pub const QUOTE: &str = "preview diagnosis";
pub const SLICE_WORDS: &str = "Start with the reproduction of the preview difference.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Scopes,
    Slices,
}

impl Level {
    pub fn save_route(self) -> &'static str {
        match self {
            Self::Scopes => "scope.candidates.save",
            Self::Slices => "slice.candidates.save",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Scopes => "program_to_scopes",
            Self::Slices => "scope_to_slices",
        }
    }
}

/// A candidate set that accepts a draft, as its context read shows it.
pub struct Target {
    pub level: Level,
    pub context: Value,
    pub program_id: Value,
    source_ref: Value,
}

/// `candidates` candidates with `items` included items each. At the Slice level a
/// decision node may stand second, between the first two work nodes.
pub struct Shape {
    pub candidates: usize,
    pub items: usize,
    pub outcome: String,
    pub decision: bool,
}

impl Shape {
    pub fn of(candidates: usize, items: usize) -> Self {
        Self {
            candidates,
            items,
            outcome: "The cause is demonstrated".into(),
            decision: false,
        }
    }
}

pub async fn target(client: &mut Mcp, repo: &Path, level: Level) -> Target {
    match level {
        Level::Scopes => scope_target(client, repo).await,
        Level::Slices => slice_target(client, repo).await,
    }
}

/// A Program with a begun Scope candidate set: Jev plays no part up to here.
async fn scope_target(client: &mut Mcp, repo: &Path) -> Target {
    let program = ready_program(client, repo).await;
    let begun = client
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),"program_id":program["id"],
                "program_revision":program["revision"],"boundary":"ongoing","input":SCOPE_WORDS}),
        )
        .await;
    let overview = CandidateFixture::from_mutation(begun)
        .read_overview(client)
        .await;
    let context = overview.value["context"].clone();
    let inputs = read_ready_json(
        client,
        &json!({"kind":"ready_call","tool":"query","arguments":{
            "route":"scope.candidates.context","params":{
                "candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25}}}),
    )
    .await
    .value;
    Target {
        level: Level::Scopes,
        source_ref: inputs["items"][0]["input"]["source_ref_id"].clone(),
        program_id: program["id"].clone(),
        context,
    }
}

/// An opened Scope whose Slice planning has one recorded input of its own. The Scope
/// candidate draft behind it is saved before Jev is enabled.
async fn slice_target(client: &mut Mcp, repo: &Path) -> Target {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let opened = route(
        client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]}),
    )
    .await;
    let planning = ScopeOpenFixture::from_mutation(opened, "created")
        .read_planning(client)
        .await
        .value;
    let pins = |context: &Value| {
        json!({"scope_id":context["scope"]["id"],
            "candidate_set_id":context["candidate_set"]["id"],
            "revision":context["candidate_set"]["revision"],"request_id":Uuid::new_v4()})
    };
    let mut input = pins(&planning);
    input["input"] = json!(SLICE_WORDS);
    let recorded = route(client, "command", "slice.candidates.input", input).await;
    let recorded = SlicePlanningFixture::from_mutation(recorded)
        .read_details(client)
        .await
        .value;
    let refreshed = route(
        client,
        "command",
        "slice.candidates.refresh",
        pins(&recorded),
    )
    .await;
    let context = SlicePlanningFixture::from_mutation(refreshed)
        .read_details(client)
        .await
        .value;
    assert_eq!(context["stale_reasons"], json!([]));
    Target {
        level: Level::Slices,
        source_ref: Value::Null,
        program_id: source["candidate_set"]["program_id"].clone(),
        context,
    }
}

impl Target {
    pub fn set(&self) -> &Value {
        &self.context["candidate_set"]
    }

    /// The save call for a draft of this shape under a request id.
    pub fn save(&self, shape: &Shape, request_id: Uuid) -> Value {
        self.save_draft(self.draft(shape), request_id)
    }

    /// The save call for a given draft under a request id.
    pub fn save_draft(&self, draft: Value, request_id: Uuid) -> Value {
        let set = self.set();
        let mut params = json!({"kind":"draft","candidate_set_id":set["id"],
            "revision":set["revision"],"snapshot_id":self.context["snapshot"]["id"],
            "input_cursor":set["latest_input"],"request_id":request_id,
            "draft":draft});
        if self.level == Level::Slices {
            params["scope_id"] = self.context["scope"]["id"].clone();
        }
        let manifest = &self.context["planning_knowledge"]["manifest"];
        if manifest["id"].is_string() {
            params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
                "digest":manifest["digest"],
                "workspace_generation":manifest["workspace_generation"]});
        }
        params
    }

    fn draft(&self, shape: &Shape) -> Value {
        let items = (1..=shape.items)
            .map(|item| format!("item {item}"))
            .collect::<Vec<_>>();
        let numbers = 1..=shape.candidates;
        if self.level == Level::Scopes {
            let goals = numbers
                .clone()
                .map(|n| {
                    let mut goal = json!({"identity":{"local":format!("goal-{n}")},
                        "text":format!("Goal {n}"),"source_ref_id":self.source_ref,
                        "resolution":{"kind":"candidate",
                            "reference":{"local":format!("scope-{n}")}}});
                    if n == 1 {
                        goal["exact_quote"] = json!(QUOTE);
                    }
                    goal
                })
                .collect::<Vec<_>>();
            let candidates = numbers
                .map(|n| {
                    let dependencies = if n > 1 {
                        json!([{"local":"scope-1"}])
                    } else {
                        json!([])
                    };
                    json!({"identity":{"local":format!("scope-{n}")},
                        "title":format!("Scope {n}"),"outcome":shape.outcome,
                        "trigger":"Preview differs","delivered_behavior":"Cause is available",
                        "proof":"Direct evidence","includes":items,"excludes":["deployment"],
                        "dependencies":dependencies,
                        "coverage_goals":[{"local":format!("goal-{n}")}],"evidence":[]})
                })
                .collect::<Vec<_>>();
            return json!({"boundary":"ongoing","goals":goals,"evidence":[],
                "candidates":candidates,"blockers":[],"protected_changes":[]});
        }
        let mut nodes = numbers
            .map(|n| {
                json!({"kind":"work","identity":{"local":format!("work-{n}")},
                    "title":format!("Work {n}"),"outcome":shape.outcome,"includes":items,
                    "excludes":["deployment"],"dependencies":[],"proof":["Causal observation"],
                    "pipeline":"slice.debug-root-cause","pipeline_reason":"Cause is unknown",
                    "source_result_ids":[]})
            })
            .collect::<Vec<_>>();
        if shape.decision {
            nodes.insert(
                1,
                json!({"kind":"decision","identity":{"local":"decision"},
                    "title":"Choose correction path","question":"Is Lightweight sufficient?",
                    "resolution_criteria":["Diagnosis identifies the boundary"],
                    "dependencies":[{"local":"work-1"}],"source_result_ids":[]}),
            );
        }
        json!({"coverage_summary":"The whole Scope","nodes":nodes,"supersessions":[]})
    }

    /// A draft a person could have written for the fixture's request, whose second
    /// candidate carries work nobody asked for: a new engine, a cache, a platform.
    pub fn overgrown(&self) -> Value {
        if self.level == Level::Scopes {
            let goal = |n: u8, text: &str| {
                json!({"identity":{"local":format!("goal-{n}")},"text":text,
                    "source_ref_id":self.source_ref,
                    "resolution":{"kind":"candidate","reference":{"local":format!("scope-{n}")}}})
            };
            let mut diagnose = goal(1, "Find why the preview is rendered incorrectly");
            diagnose["exact_quote"] = json!(QUOTE);
            let candidate = |n: u8, title: &str, outcome: &str, includes: Value, after: Value| {
                json!({"identity":{"local":format!("scope-{n}")},"title":title,"outcome":outcome,
                    "trigger":"Preview differs","delivered_behavior":"Cause is available",
                    "proof":"Direct evidence","includes":includes,"excludes":["deployment"],
                    "dependencies":after,"coverage_goals":[{"local":format!("goal-{n}")}],
                    "evidence":[]})
            };
            return json!({"boundary":"ongoing",
                "goals":[diagnose, goal(2, "Select the smallest correction of the preview")],
                "evidence":[],"blockers":[],"protected_changes":[],
                "candidates":[
                    candidate(1, "Diagnose the incorrect preview",
                        "The cause of the incorrect preview is demonstrated on a reproduction",
                        json!(["reproduce the incorrect preview on a fixed input",
                            "trace the rendering path to the first wrong value"]),
                        json!([])),
                    candidate(2, "Correct the preview and rebuild the rendering platform",
                        "The preview is correct and the renderer runs on a new platform",
                        json!(["apply the smallest correction that removes the cause",
                            "replace the preview renderer with a new rendering engine",
                            "add a Redis cache and a Kubernetes deployment for previews"]),
                        json!([{"local":"scope-1"}]))]});
        }
        let work = |n: u8, title: &str, outcome: &str, includes: Value, after: Value| {
            json!({"kind":"work","identity":{"local":format!("work-{n}")},"title":title,
                "outcome":outcome,"includes":includes,"excludes":["deployment"],
                "dependencies":after,"proof":["Causal observation"],
                "pipeline":"slice.debug-root-cause","pipeline_reason":"Cause is unknown",
                "source_result_ids":[]})
        };
        json!({"coverage_summary":"The whole Scope","supersessions":[],"nodes":[
            work(1, "Reproduce the preview difference",
                "The preview difference is reproduced on a fixed input",
                json!(["build a failing reproduction from the reported input"]), json!([])),
            work(2, "Diagnose the cause and migrate the test suite",
                "The cause is demonstrated and all tests run on a new framework",
                json!(["trace the rendering path to the first wrong value",
                    "migrate the whole test suite to a new test framework",
                    "add performance dashboards for the rendering service"]),
                json!([{"local":"work-1"}]))]})
    }

    /// The draft as the store holds it after a save: its candidates or nodes.
    pub async fn stored(&self, client: &mut Mcp, reply: &Value) -> Vec<Value> {
        let details = match self.level {
            Level::Scopes => CandidateFixture::from_mutation(reply.clone())
                .read_details(client)
                .await
                .value["draft"]["candidates"]
                .clone(),
            Level::Slices => SlicePlanningFixture::from_mutation(reply.clone())
                .read_details(client)
                .await
                .value["draft"]["nodes"]
                .clone(),
        };
        details.as_array().unwrap().clone()
    }
}
