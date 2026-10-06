#[path = "native_reads/program.rs"]
mod program;
#[path = "native_reads/program_lists.rs"]
pub mod program_lists;
#[path = "native_reads/program_queries.rs"]
pub mod program_queries;
#[allow(unused_imports)]
pub use program::ProgramFixture;

use super::{
    Mcp,
    candidate_reads::{ResolvedRead, read_ready_json},
};
use serde_json::Value;
use uuid::Uuid;

fn uuid(value: &Value) {
    Uuid::parse_str(value.as_str().expect("fixture UUID string")).expect("fixture UUID");
}
fn revision(value: &Value) {
    assert!(
        value.as_i64().is_some_and(|value| value > 0),
        "fixture revision"
    );
}
fn destination<'a>(mutation: &'a Value, field: &str, route: &str, scope_id: &Value) -> &'a Value {
    let action = &mutation["field_destinations"][field];
    assert_eq!(action["kind"], "ready_call", "destination readiness");
    assert_eq!(action["tool"], "query", "destination query");
    assert_eq!(action["arguments"]["route"], route, "destination route");
    assert_eq!(
        action["arguments"]["params"]["scope_id"], *scope_id,
        "destination scope"
    );
    if route == "slice.candidates.context" {
        assert_eq!(
            action["arguments"]["params"]["view"], "details",
            "destination details"
        );
    }
    action
}
fn planning_pins(read: &ResolvedRead, scope_id: &Value, set: &Value, snapshot: &Value) {
    assert_eq!(read.value["scope"]["id"], *scope_id, "planning scope ID");
    for key in ["id", "revision"] {
        assert_eq!(
            read.value["candidate_set"][key], set[key],
            "planning set {key}"
        );
    }
    assert_eq!(
        read.value["snapshot"]["id"], *snapshot,
        "planning snapshot ID"
    );
    if let Some(source) = &read.provenance.source {
        assert_eq!(source["scope_id"], *scope_id, "fragment scope");
        assert_eq!(source["candidate_set_id"], set["id"], "fragment set");
        assert_eq!(source["snapshot_id"], *snapshot, "fragment snapshot");
    }
}

pub struct ScopeOpenFixture {
    pub mutation: Value,
}
impl ScopeOpenFixture {
    pub fn from_mutation(mutation: Value, disposition: &str) -> Self {
        assert_eq!(mutation["disposition"], disposition, "scope disposition");
        assert!(
            mutation.get("created").is_none() && mutation.get("replay").is_none(),
            "compact scope outcome"
        );
        assert!(
            mutation["planning"].get("snapshot").is_none(),
            "compact planning snapshot"
        );
        for key in [
            "id",
            "source_candidate_set_id",
            "source_candidate_id",
            "slice_candidate_set_id",
        ] {
            uuid(&mutation["scope"][key]);
        }
        revision(&mutation["scope"]["revision"]);
        uuid(&mutation["planning"]["candidate_set"]["id"]);
        revision(&mutation["planning"]["candidate_set"]["revision"]);
        uuid(&mutation["planning"]["snapshot_id"]);
        assert_eq!(
            mutation["scope"]["slice_candidate_set_id"],
            mutation["planning"]["candidate_set"]["id"],
            "scope planning set"
        );
        Self { mutation }
    }
    pub async fn read_scope(&self, client: &mut Mcp) -> ResolvedRead {
        let action = destination(
            &self.mutation,
            "scope",
            "scope.context",
            &self.mutation["scope"]["id"],
        );
        let read = read_ready_json(client, action).await;
        for key in [
            "id",
            "revision",
            "source_candidate_set_id",
            "source_candidate_id",
            "slice_candidate_set_id",
        ] {
            assert_eq!(
                read.value[key], self.mutation["scope"][key],
                "scope read {key}"
            );
        }
        if let Some(source) = &read.provenance.source {
            assert_eq!(
                source["scope_id"], self.mutation["scope"]["id"],
                "scope fragment ID"
            );
            assert_eq!(
                source["scope_revision"], self.mutation["scope"]["revision"],
                "scope fragment revision"
            );
        }
        read
    }
    pub async fn read_planning(&self, client: &mut Mcp) -> ResolvedRead {
        let scope = &self.mutation["scope"];
        let action = destination(
            &self.mutation,
            "planning",
            "slice.candidates.context",
            &scope["id"],
        );
        let read = read_ready_json(client, action).await;
        planning_pins(
            &read,
            &scope["id"],
            &self.mutation["planning"]["candidate_set"],
            &self.mutation["planning"]["snapshot_id"],
        );
        for key in [
            "revision",
            "source_candidate_set_id",
            "source_candidate_id",
            "slice_candidate_set_id",
        ] {
            assert_eq!(read.value["scope"][key], scope[key], "planning scope {key}");
        }
        read
    }
}

pub struct SlicePlanningFixture {
    pub mutation: Value,
}
impl SlicePlanningFixture {
    pub fn from_mutation(mutation: Value) -> Self {
        assert!(
            mutation.get("scope").is_none() && mutation.get("draft").is_none(),
            "compact slice planning"
        );
        uuid(&mutation["scope_id"]);
        uuid(&mutation["candidate_set"]["id"]);
        revision(&mutation["candidate_set"]["revision"]);
        uuid(&mutation["snapshot"]["id"]);
        revision(&mutation["snapshot"]["sequence"]);
        Self { mutation }
    }
    pub async fn read_details(&self, client: &mut Mcp) -> ResolvedRead {
        let action = destination(
            &self.mutation,
            "complete_planning_context",
            "slice.candidates.context",
            &self.mutation["scope_id"],
        );
        let read = read_ready_json(client, action).await;
        planning_pins(
            &read,
            &self.mutation["scope_id"],
            &self.mutation["candidate_set"],
            &self.mutation["snapshot"]["id"],
        );
        assert_eq!(
            read.value["snapshot"]["sequence"], self.mutation["snapshot"]["sequence"],
            "planning sequence"
        );
        read
    }
}
