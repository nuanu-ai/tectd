//! What the override tests do through the bridge beyond the shared fixtures: follow
//! an advertised call, read a refusal, and walk a Scope candidate set or the Slice
//! candidate set of an opened Scope from one head to the next.
use crate::drafts::{Level, target};
use crate::planning::route;
use crate::recovery_support::candidate_reads::{
    CandidateFixture, read_query_json, read_ready_json,
};
use crate::recovery_support::native_reads::{ScopeOpenFixture, SlicePlanningFixture};
use crate::recovery_support::{Mcp, tool_payload};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

/// What the user is taken to have answered when asked for the override.
pub const PERMISSION: &str =
    "Yes. For the export part I allow demo, a throwaway script is enough there.";
pub const QUOTE: &str = "I allow demo";

/// The call an action advertises, made as the agent would make it.
pub async fn follow(client: &mut Mcp, action: &Value) -> Value {
    assert_eq!(action["kind"], "ready_call", "{action}");
    let response = client
        .exchange(
            "tools/call",
            json!({"name":action["tool"],"arguments":action["arguments"]}),
        )
        .await;
    assert_ne!(response["result"]["isError"], true, "{response}");
    tool_payload(&response)
}

/// A refused command: the error the agent reads and the sentence above it.
pub async fn refused(client: &mut Mcp, route: &str, params: Value) -> (Value, String) {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":"command","arguments":{"route":route,"params":params}}),
        )
        .await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    let intro = response["result"]["content"][0]["text"].as_str().unwrap();
    (tool_payload(&response)["error"].clone(), intro.to_owned())
}

/// The refusal of an override: its code, the rule and the part it points at, and an
/// intro that names every step in the order the agent has to take them.
pub fn assert_unpermitted(refusal: &(Value, String), rule: &str, path: &str, steps: &[&str]) {
    let (error, intro) = refusal;
    assert_eq!(error["code"], "MODE_OVERRIDE_NOT_PERMITTED", "{error}");
    assert_eq!(error["refusal"]["code"], "MODE_OVERRIDE_NOT_PERMITTED");
    assert_eq!(error["refusal"]["rule"], rule, "{error}");
    assert_eq!(error["refusal"]["path"], path, "{error}");
    assert_eq!(error["refusal"]["next_action"], steps[1], "{error}");
    let mut from = 0;
    for step in steps {
        let found = intro[from..]
            .find(step)
            .unwrap_or_else(|| panic!("`{step}` is missing or out of order in: {intro}"));
        from += found + step.len();
    }
    assert!(intro.ends_with("Nothing was stored."), "{intro}");
}

/// The head of a Scope candidate set as its overview shows it.
pub struct ScopeSet {
    pub context: Value,
}

impl ScopeSet {
    /// A Program with a begun Scope candidate set whose one input is the planning request.
    pub async fn begun(client: &mut Mcp, repo: &Path) -> Self {
        let context = target(client, repo, Level::Scopes).await.context;
        Self { context }
    }

    /// The head a successful command left, read through the reply's own action.
    pub async fn after(client: &mut Mcp, reply: Value) -> Self {
        let overview = CandidateFixture::from_mutation(reply)
            .read_overview(client)
            .await;
        Self {
            context: overview.value["context"].clone(),
        }
    }

    pub fn id(&self) -> &Value {
        &self.context["candidate_set"]["id"]
    }

    pub fn revision(&self) -> &Value {
        &self.context["candidate_set"]["revision"]
    }

    async fn page(&self, client: &mut Mcp, view: &str, after: Option<i64>) -> Value {
        let mut params = json!({"candidate_set_id":self.id(),"view":view,"limit":25});
        if let Some(after) = after {
            params["after"] = json!(after);
        }
        let action = json!({"kind":"ready_call","tool":"query","arguments":{
            "route":"scope.candidates.context","params":params}});
        read_ready_json(client, &action).await.value
    }

    async fn view(&self, client: &mut Mcp, view: &str) -> Value {
        self.page(client, view, None).await
    }

    /// Every item of one kind in a listing view, following `next_after` to its end.
    async fn items(&self, client: &mut Mcp, view: &str, kind: &str) -> Vec<Value> {
        let mut items = Vec::new();
        let mut after = None;
        loop {
            let page = self.page(client, view, after).await;
            items.extend(
                page["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|item| item.get(kind).cloned()),
            );
            after = page["next_after"].as_i64();
            if after.is_none() {
                return items;
            }
        }
    }

    /// The source reference of each recorded input in the current snapshot, in order.
    pub async fn input_refs(&self, client: &mut Mcp) -> Vec<Value> {
        let inputs = self.items(client, "inputs", "input").await;
        inputs
            .iter()
            .map(|input| input["source_ref_id"].clone())
            .collect()
    }

    /// The source references of the Program's fields in the current snapshot.
    pub async fn field_refs(&self, client: &mut Mcp) -> Vec<Value> {
        self.view(client, "program").await["program"]["field_refs"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// The whole saved draft and its context, as view `details` shows them.
    pub async fn details(&self, client: &mut Mcp) -> Value {
        self.view(client, "details").await
    }

    /// The saved draft's candidates as the `candidates` view and as `details` show them.
    pub async fn candidates(&self, client: &mut Mcp) -> (Vec<Value>, Vec<Value>) {
        let listed = self.items(client, "candidates", "candidate").await;
        let details = self.details(client).await["draft"]["candidates"]
            .as_array()
            .unwrap()
            .clone();
        (listed, details)
    }

    /// Record one more input of the user. The set is stale until it is refreshed.
    pub async fn record(&self, client: &mut Mcp, input: &str) -> Value {
        client
            .call(
                "record_candidate_input",
                json!({"candidate_set_id":self.id(),"revision":self.revision(),
                    "request_id":Uuid::new_v4(),"input":input}),
            )
            .await
    }

    /// Record an input and take the refresh its reply advertises.
    pub async fn record_and_refresh(&self, client: &mut Mcp, input: &str) -> Self {
        let recorded = self.record(client, input).await;
        let refresh = &recorded["actions"][1];
        assert_eq!(
            refresh["arguments"]["route"], "scope.candidates.refresh",
            "{recorded}"
        );
        let refreshed = follow(client, refresh).await;
        Self::after(client, refreshed).await
    }

    /// The save call for a draft at this head.
    pub fn save(&self, draft: Value) -> Value {
        self.write("draft", draft)
    }

    /// The review that accepts every one of `candidates`.
    pub fn accept(&self, candidates: &[Value]) -> Value {
        let decisions = candidates
            .iter()
            .map(|candidate| {
                json!({"candidate_id":candidate["id"],"decision":"accept",
                    "rationale":"Coherent Scope"})
            })
            .collect::<Vec<_>>();
        self.write(
            "review",
            json!({"verdict":"ready","summary":"Bounded and ready","findings":[],
                "candidate_decisions":decisions}),
        )
    }

    fn write(&self, kind: &str, body: Value) -> Value {
        let set = &self.context["candidate_set"];
        let mut params = json!({"kind":kind,"candidate_set_id":set["id"],
            "revision":set["revision"],"snapshot_id":self.context["snapshot"]["id"],
            "input_cursor":set["latest_input"],"request_id":Uuid::new_v4(),kind:body});
        let manifest = &self.context["planning_knowledge"]["manifest"];
        if manifest["id"].is_string() {
            params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
                "digest":manifest["digest"],
                "workspace_generation":manifest["workspace_generation"]});
        }
        params
    }

    /// The call that opens a Scope from one accepted candidate of this head.
    pub fn open(&self, candidate: &Value) -> Value {
        let set = &self.context["candidate_set"];
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":set["id"],
            "candidate_set_revision":set["revision"],
            "candidate_snapshot_id":self.context["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})
    }
}

/// A new Scope candidate under a local label, covered by a goal of its own.
pub fn scope_candidate(number: usize) -> Value {
    json!({"identity":{"local":format!("scope-{number}")},"title":format!("Scope {number}"),
        "outcome":"The cause is demonstrated","trigger":"Preview differs",
        "delivered_behavior":"Cause is available","proof":"Direct evidence",
        "includes":["item 1"],"excludes":["deployment"],"dependencies":[],
        "coverage_goals":[{"local":format!("goal-{number}")}],"evidence":[]})
}

/// A first draft of `candidates`, numbered from one in the order given.
pub fn scope_draft(goal_ref: &Value, candidates: Vec<Value>) -> Value {
    let goals = (1..=candidates.len())
        .map(|number| {
            json!({"identity":{"local":format!("goal-{number}")},
                "text":format!("Goal {number}"),"source_ref_id":goal_ref,
                "resolution":{"kind":"candidate",
                    "reference":{"local":format!("scope-{number}")}}})
        })
        .collect::<Vec<_>>();
    json!({"boundary":"ongoing","goals":goals,"evidence":[],"candidates":candidates,
        "blockers":[],"protected_changes":[]})
}

/// The same candidates and goals as the store returned them, for a later save:
/// backend ids and revisions, every goal citing `goal_ref`.
pub fn scope_resave(details: &Value, goal_ref: &Value) -> Value {
    let existing = |entity: &Value| json!({"id":entity["id"],"revision":entity["revision"]});
    let goals = details["draft"]["goals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|goal| {
            json!({"identity":existing(goal),"text":goal["text"],"source_ref_id":goal_ref,
                "resolution":{"kind":"candidate","reference":{"id":goal["resolution"]["id"]}}})
        })
        .collect::<Vec<_>>();
    let candidates = details["draft"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| {
            let goals = candidate["coverage_goal_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| json!({"id":id}))
                .collect::<Vec<_>>();
            let mut resaved = json!({"identity":existing(candidate),"title":candidate["title"],
                "outcome":candidate["outcome"],"trigger":candidate["trigger"],
                "delivered_behavior":candidate["delivered_behavior"],
                "proof":candidate["proof"],"includes":candidate["includes"],
                "excludes":candidate["excludes"],"dependencies":[],
                "coverage_goals":goals,"evidence":[]});
            if let Some(kept) = candidate.get("mode_override") {
                resaved["mode_override"] = kept.clone();
            }
            resaved
        })
        .collect::<Vec<_>>();
    json!({"boundary":"ongoing","goals":goals,"evidence":[],"candidates":candidates,
        "blockers":[],"protected_changes":[]})
}

/// The head of an opened Scope's Slice candidate set as its `details` read shows it.
pub struct SliceSet {
    pub context: Value,
}

impl SliceSet {
    /// The Slice planning of the Scope opened from one accepted candidate of `source`.
    pub async fn opened(client: &mut Mcp, source: &ScopeSet, candidate: &Value) -> Self {
        let opened = route(client, "command", "scope.open", source.open(candidate)).await;
        let planning = ScopeOpenFixture::from_mutation(opened, "created")
            .read_planning(client)
            .await;
        Self {
            context: planning.value,
        }
    }

    /// The head a successful command left, read through the reply's own action.
    pub async fn after(client: &mut Mcp, reply: Value) -> Self {
        let details = SlicePlanningFixture::from_mutation(reply)
            .read_details(client)
            .await;
        Self {
            context: details.value,
        }
    }

    pub fn scope_id(&self) -> &Value {
        &self.context["scope"]["id"]
    }

    fn pins(&self) -> Value {
        let set = &self.context["candidate_set"];
        json!({"scope_id":self.scope_id(),"candidate_set_id":set["id"],
            "revision":set["revision"],"request_id":Uuid::new_v4()})
    }

    /// The items of one listing view of the planning context.
    pub async fn items(&self, client: &mut Mcp, view: &str) -> Vec<Value> {
        let page = read_query_json(
            client,
            &json!({"route":"slice.candidates.context",
                "params":{"scope_id":self.scope_id(),"view":view,"limit":100}}),
        )
        .await
        .value;
        assert!(page["next_after"].is_null(), "{page}");
        page["items"].as_array().unwrap().clone()
    }

    /// Record one more input of the user. The set is stale until it is refreshed.
    pub async fn record(&self, client: &mut Mcp, input: &str) -> Value {
        let mut params = self.pins();
        params["input"] = json!(input);
        route(client, "command", "slice.candidates.input", params).await
    }

    /// Refresh the set at this head.
    pub async fn refresh(&self, client: &mut Mcp) -> Self {
        let refreshed = route(client, "command", "slice.candidates.refresh", self.pins()).await;
        Self::after(client, refreshed).await
    }

    pub async fn record_and_refresh(&self, client: &mut Mcp, input: &str) -> Self {
        let recorded = self.record(client, input).await;
        Self::after(client, recorded).await.refresh(client).await
    }

    /// The save call for a draft at this head.
    pub fn save(&self, draft: Value) -> Value {
        let set = &self.context["candidate_set"];
        let mut params = self.pins();
        params["kind"] = json!("draft");
        params["snapshot_id"] = self.context["snapshot"]["id"].clone();
        params["input_cursor"] = set["latest_input"].clone();
        params["draft"] = draft;
        let manifest = &self.context["planning_knowledge"]["manifest"];
        if manifest["id"].is_string() {
            params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
                "digest":manifest["digest"],
                "workspace_generation":manifest["workspace_generation"]});
        }
        params
    }
}

/// An opened Scope as `scope.context` shows it.
pub async fn scope_context(client: &mut Mcp, scope_id: &Value) -> Value {
    let query = json!({"route":"scope.context","params":{"scope_id":scope_id}});
    read_query_json(client, &query).await.value
}

/// The two override columns of one opened Scope or Slice, as the operator sees them.
pub async fn carried(pool: &sqlx::PgPool, table: &str, id: &Value) -> (Value, Value) {
    let column = match table {
        "native_scopes" => "mode_override_source_ref_id",
        _ => "mode_override_input_id",
    };
    let id = Uuid::parse_str(id.as_str().unwrap()).unwrap();
    let row: (Option<String>, Option<Uuid>) = sqlx::query_as(&format!(
        "SELECT mode_override, {column} FROM {table} WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    (json!(row.0), json!(row.1))
}

/// A new work node under a local label.
pub fn work_node(number: usize) -> Value {
    json!({"kind":"work","identity":{"local":format!("work-{number}")},
        "title":format!("Work {number}"),"outcome":"The cause is demonstrated",
        "includes":["item 1"],"excludes":["deployment"],"dependencies":[],
        "proof":["Causal observation"],"pipeline":"slice.debug-root-cause",
        "pipeline_reason":"Cause is unknown","source_result_ids":[]})
}

/// A first Slice candidate draft of `nodes`.
pub fn slice_draft(nodes: Vec<Value>) -> Value {
    json!({"coverage_summary":"The whole Scope","nodes":nodes,"supersessions":[]})
}

/// A stored work node stated again for a later save, unchanged.
pub fn retained(node: &Value) -> Value {
    json!({"kind":"work","identity":{"candidate_id":node["id"],"revision":node["revision"]},
        "title":node["title"],"outcome":node["outcome"],"includes":node["includes"],
        "excludes":node["excludes"],"dependencies":[],"proof":node["proof"],
        "pipeline":node["pipeline"],"pipeline_reason":node["pipeline_reason"],
        "source_result_ids":[]})
}

/// A Scope candidate set whose draft of `candidates` is saved and reviewed ready with
/// every candidate accepted: its head, the candidates as stored, and the source
/// references of its inputs.
pub async fn reviewed(
    client: &mut Mcp,
    head: ScopeSet,
    candidates: Vec<Value>,
) -> (ScopeSet, Vec<Value>, Vec<Value>) {
    let refs = head.input_refs(client).await;
    let draft = scope_draft(&refs[0], candidates);
    let saved = client.call("save_candidate_set", head.save(draft)).await;
    let head = ScopeSet::after(client, saved).await;
    let (_, stored) = head.candidates(client).await;
    let review = client
        .call("save_candidate_set", head.accept(&stored))
        .await;
    assert_eq!(review["candidate_set"]["status"], "ready", "{review}");
    (ScopeSet::after(client, review).await, stored, refs)
}
