//! What the binding tests do beyond the shared fixtures: walk a candidate set of
//! either planning level from one head to the next, save and review drafts at it, and
//! read the kept answers as the operator sees them.
use crate::drafts::{Level, target};
use crate::recovery_support::Mcp;
use crate::support::Stand;
use crate::walk::{
    ScopeSet, SliceSet, retained, scope_candidate, scope_draft, scope_resave, slice_draft,
    work_node,
};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

pub const DEBUG: &str = "slice.debug-root-cause";
pub const RESEARCH: &str = "slice.research";

/// Jev asks for nothing: it keeps every candidate, finds no item extra and chooses
/// the pipeline the drafts of these tests choose.
pub fn content(name: &str) -> (&'static str, f64) {
    match name.rsplit('.').next() {
        Some("size") => ("bounded", 0.7),
        Some("advice") => ("keep", 0.7),
        Some("pipeline") => (DEBUG, 0.8),
        _ => ("requested", 0.7),
    }
}

/// Jev wants the first candidate of the draft split and asks for nothing else.
pub fn first_split(name: &str) -> (&'static str, f64) {
    match name {
        "c1.advice" => ("split", 0.8),
        other => content(other),
    }
}

/// Jev wants the first two candidates of the draft split.
pub fn two_split(name: &str) -> (&'static str, f64) {
    match name {
        "c1.advice" | "c2.advice" => ("split", 0.8),
        other => content(other),
    }
}

/// Jev abstains on every question.
pub fn abstaining(_: &str) -> (&'static str, f64) {
    ("ABSTAIN", 0.9)
}

/// The head of a candidate set of one planning level.
pub enum Head {
    /// A Scope candidate set, and the source reference its goals cite.
    Scopes(ScopeSet, Value),
    Slices(SliceSet),
}

impl Head {
    /// A candidate set that accepts a first draft. Jev plays no part up to here.
    pub async fn begun(client: &mut Mcp, repo: &Path, level: Level) -> Self {
        match level {
            Level::Scopes => {
                let head = ScopeSet::begun(client, repo).await;
                let goal_ref = head.input_refs(client).await.remove(0);
                Self::Scopes(head, goal_ref)
            }
            Level::Slices => Self::Slices(SliceSet {
                context: target(client, repo, level).await.context,
            }),
        }
    }

    pub fn level(&self) -> Level {
        match self {
            Self::Scopes(..) => Level::Scopes,
            Self::Slices(_) => Level::Slices,
        }
    }

    pub fn route(&self) -> &'static str {
        self.level().save_route()
    }

    /// The candidate set as the head shows it: its id, revision and status.
    pub fn set(&self) -> &Value {
        match self {
            Self::Scopes(head, _) => &head.context["candidate_set"],
            Self::Slices(head) => &head.context["candidate_set"],
        }
    }

    /// The head a successful save or review left.
    pub async fn after(&self, client: &mut Mcp, reply: &Value) -> Self {
        match self {
            Self::Scopes(_, goal_ref) => Self::Scopes(
                ScopeSet::after(client, reply.clone()).await,
                goal_ref.clone(),
            ),
            Self::Slices(_) => Self::Slices(SliceSet::after(client, reply.clone()).await),
        }
    }

    /// The candidates of the saved draft as the store holds them, in draft order.
    pub async fn candidates(&self, client: &mut Mcp) -> Vec<Value> {
        match self {
            Self::Scopes(head, _) => head.candidates(client).await.1,
            Self::Slices(head) => head.context["draft"]["nodes"].as_array().unwrap().clone(),
        }
    }

    /// The save call for a first draft of this many new candidates.
    pub fn first(&self, count: usize) -> Value {
        match self {
            Self::Scopes(head, goal_ref) => {
                let candidates = (1..=count).map(scope_candidate).collect();
                head.save(scope_draft(goal_ref, candidates))
            }
            Self::Slices(head) => head.save(slice_draft((1..=count).map(work_node).collect())),
        }
    }

    /// The save call for the saved draft stated again, with the candidates at these
    /// 1-based positions changed and the others as they are.
    pub async fn again(&self, client: &mut Mcp, changed: &[usize]) -> Value {
        let outcome = json!(format!("Another outcome {}", Uuid::new_v4().simple()));
        let rationale = json!("Follows the advice");
        match self {
            Self::Scopes(head, goal_ref) => {
                let mut draft = scope_resave(&head.details(client).await, goal_ref);
                for position in changed {
                    draft["candidates"][position - 1]["outcome"] = outcome.clone();
                    draft["candidates"][position - 1]["change_rationale"] = rationale.clone();
                }
                head.save(draft)
            }
            Self::Slices(head) => {
                let mut nodes = self.candidates(client).await;
                nodes = nodes.iter().map(retained).collect();
                for position in changed {
                    nodes[position - 1]["outcome"] = outcome.clone();
                    nodes[position - 1]["change_rationale"] = rationale.clone();
                }
                head.save(slice_draft(nodes))
            }
        }
    }

    /// The review call at this head with this verdict; a Scope review accepts every
    /// one of `candidates`. `departures` is the review's `advice_departures`, when
    /// it states the list at all.
    pub fn review(&self, candidates: &[Value], verdict: &str, departures: Option<Value>) -> Value {
        let mut params = match self {
            Self::Scopes(head, _) => {
                let mut params = head.accept(candidates);
                params["review"]["verdict"] = json!(verdict);
                params
            }
            Self::Slices(head) => {
                let mut params = head.save(Value::Null);
                params.as_object_mut().unwrap().remove("draft");
                params["kind"] = json!("review");
                params["review"] = json!({"verdict":verdict,"summary":"Bounded and ready",
                    "findings":[]});
                params
            }
        };
        if let Some(departures) = departures {
            params["review"]["advice_departures"] = departures;
        }
        params
    }
}

/// One departure as a review states it.
pub fn departure(candidate: &Value, point: &str, reason: &str) -> Value {
    json!({"candidate_id":candidate["id"],"point":point,"reason":reason})
}

/// The kept answers of one candidate set as the operator sees them, in the order
/// they were given and, within one call, in draft order.
pub async fn answers(stand: &Stand, set: &Value) -> Vec<Value> {
    let set = Uuid::parse_str(set["id"].as_str().unwrap()).unwrap();
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(a) FROM advice_candidate_answers a \
         JOIN advice_journal j ON j.id=a.journal_id \
         WHERE a.tenant_id=$1 AND a.candidate_set_id=$2 \
         ORDER BY j.created_at, j.id, (a.answer->>'candidate')::int",
    )
    .bind(stand.tenant)
    .bind(set)
    .fetch_all(&stand.pool)
    .await
    .unwrap()
}

/// How many reviews and receipts the candidate set of a head has in the store.
pub async fn stored(stand: &Stand, head: &Head) -> (i64, i64) {
    let set = Uuid::parse_str(head.set()["id"].as_str().unwrap()).unwrap();
    let (reviews, receipts, key) = match head.level() {
        Level::Scopes => (
            "scope_candidate_reviews",
            "scope_candidate_receipts",
            "candidate_set_id",
        ),
        Level::Slices => (
            "slice_candidate_reviews",
            "native_planning_receipts",
            "entity_id",
        ),
    };
    sqlx::query_as(&format!(
        "SELECT (SELECT count(*) FROM {reviews} WHERE tenant_id=$1 AND candidate_set_id=$2), \
                (SELECT count(*) FROM {receipts} WHERE tenant_id=$1 AND {key}=$2)"
    ))
    .bind(stand.tenant)
    .bind(set)
    .fetch_one(&stand.pool)
    .await
    .unwrap()
}
