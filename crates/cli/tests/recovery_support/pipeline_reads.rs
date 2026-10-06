//! Explicit pipeline fixture reads; compact replies and complete representations stay separate.
use super::Mcp;
use serde_json::{Value, json};
use uuid::Uuid;

mod bytes;
mod metadata;
mod outputs;
#[allow(unused_imports)]
pub use metadata::{ResolvedPipelineMetadata, resolve_pipeline_metadata};
#[allow(unused_imports)]
pub use outputs::read_pipeline_output;
#[cfg(test)]
mod tests;
#[allow(unused_imports)]
pub use bytes::{ReadProvenance, ResolvedRead};
pub type FixtureResult<T> = Result<T, String>;
const ROUTE: &str = "slice.pipeline.context";

fn require(condition: bool, message: &str) -> FixtureResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

pub struct ResolvedPipeline {
    pub raw_payload: Value,
    pub compact_context: Value,
    pub snapshot: ResolvedRead,
    pub phase_contract: Option<ResolvedRead>,
    pub details: ResolvedRead,
}
impl ResolvedPipeline {
    pub fn run(&self) -> &Value {
        &self.compact_context["run"]
    }
    pub fn definition(&self) -> &Value {
        &self.snapshot.value["definition"]
    }
    pub fn current_phase(&self) -> FixtureResult<&Value> {
        self.phase_contract
            .as_ref()
            .map(|read| &read.value["phase"])
            .ok_or_else(|| "resolved pipeline has no current phase contract".into())
    }
    pub fn details_data(&self) -> &Value {
        &self.details.value["data"]
    }
    /// Explicit historical/noncurrent phase query; the current phase stays separate.
    pub async fn read_phase_contract(
        &self,
        client: &mut Mcp,
        phase_id: &str,
    ) -> FixtureResult<ResolvedRead> {
        let stored = self.definition()["phases"]
            .as_array()
            .ok_or("complete phases missing")?
            .iter()
            .filter(|phase| phase["id"] == phase_id)
            .collect::<Vec<_>>();
        require(
            stored.len() == 1,
            "explicit phase missing or ambiguous in complete snapshot",
        )?;
        let arguments = json!({"route":ROUTE,"params":{"run_id":self.run()["id"],
            "view":"phase_contract","definition_digest":self.run()["definition_digest"],"phase_id":phase_id}});
        let read = bytes::read_query(client, &arguments).await?;
        require(
            read.value["phase"] == *stored[0],
            "explicit phase differs from stored complete snapshot",
        )?;
        Ok(read)
    }
}

fn compact_context(raw: &Value) -> FixtureResult<Value> {
    let contexts = std::iter::once(raw)
        .chain(
            ["created", "replay", "context"]
                .iter()
                .filter_map(|key| raw.get(key)),
        )
        .filter(|value| value["delivery_scope"] == "snapshot_reference")
        .collect::<Vec<_>>();
    require(
        contexts.len() == 1,
        "missing or ambiguous compact pipeline context",
    )?;
    let context = contexts[0];
    let run = &context["run"];
    let id = Uuid::parse_str(run["id"].as_str().ok_or("run ID missing")?)
        .map_err(|_| "invalid run UUID")?;
    require(!id.is_nil(), "nil run UUID")?;
    require(
        matches!(
            run["status"].as_str(),
            Some("active" | "waiting_input" | "blocked" | "completed" | "escalated" | "superseded")
        ),
        "invalid pipeline run status",
    )?;
    require(
        run["revision"]
            .as_i64()
            .is_some_and(|revision| revision > 0),
        "invalid run revision",
    )?;
    for (field, identity) in [
        ("kind", "definition_kind"),
        ("version", "definition_version"),
        ("digest", "definition_digest"),
    ] {
        require(
            context["definition"][field].is_string()
                && context["definition"][field] == run[identity],
            "compact definition identity differs from run",
        )?;
    }
    require(
        context["definition"].get("phases").is_none(),
        "origin must remain compact",
    )?;
    require(raw["actions"].is_array(), "actual outer actions missing")?;
    Ok(context.clone())
}

fn destination(raw: &Value, run: &Value, view: &str) -> FixtureResult<Value> {
    let matches = raw["actions"]
        .as_array()
        .ok_or("outer actions missing")?
        .iter()
        .filter(|action| {
            action["tool"] == "query"
                && action["arguments"]["route"] == ROUTE
                && action["arguments"]["params"]["view"] == view
                && (view != "details" || action["arguments"]["params"]["section"] == "all")
        })
        .collect::<Vec<_>>();
    require(
        matches.len() == 1,
        "missing or ambiguous actual pipeline destination",
    )?;
    let action = matches[0];
    require(
        action["kind"] == "ready_call",
        "pipeline destination must be Ready",
    )?;
    let params = &action["arguments"]["params"];
    require(params["run_id"] == run["id"], "destination run pin changed")?;
    match view {
        "snapshot" | "phase_contract" => {
            require(
                params["definition_digest"] == run["definition_digest"],
                "destination definition pin changed",
            )?;
            if view == "phase_contract" {
                require(
                    params["phase_id"] == run["current_phase_id"],
                    "destination phase pin changed",
                )?;
            }
        }
        "details" => {
            require(
                params["run_revision"] == run["revision"] && params["section"] == "all",
                "destination details pin changed",
            )?;
        }
        _ => return Err("unsupported pipeline destination".into()),
    }
    Ok(action.clone())
}

fn verify_snapshot(context: &Value, value: &Value) -> FixtureResult<()> {
    let run = &context["run"];
    require(
        value["run_id"] == run["id"] && value["definition_digest"] == run["definition_digest"],
        "snapshot pins differ from origin",
    )?;
    for (field, identity) in [
        ("kind", "definition_kind"),
        ("version", "definition_version"),
        ("digest", "definition_digest"),
    ] {
        require(
            value["definition"][field] == run[identity],
            "snapshot definition identity changed",
        )?;
    }
    let phases = value["definition"]["phases"]
        .as_array()
        .ok_or("complete snapshot phases missing")?;
    require(
        context["counts"]["phases"].as_u64() == Some(phases.len() as u64),
        "snapshot lost phases",
    )?;
    Ok(())
}
fn verify_phase(context: &Value, snapshot: &Value, value: &Value) -> FixtureResult<()> {
    let run = &context["run"];
    require(
        value["run_id"] == run["id"] && value["definition_digest"] == run["definition_digest"],
        "phase contract pins differ from origin",
    )?;
    let matches = snapshot["definition"]["phases"]
        .as_array()
        .ok_or("complete phases missing")?
        .iter()
        .filter(|phase| phase["id"] == run["current_phase_id"])
        .collect::<Vec<_>>();
    require(
        matches.len() == 1,
        "missing or ambiguous stored current phase",
    )?;
    require(
        value["phase"] == *matches[0],
        "phase contract differs from complete stored snapshot",
    )
}
fn verify_details(context: &Value, value: &Value) -> FixtureResult<()> {
    let run = &context["run"];
    require(
        value["run_id"] == run["id"]
            && value["run_revision"] == run["revision"]
            && value["section"] == "all",
        "details pins differ from origin",
    )?;
    let data = value["data"].as_object().ok_or("details data missing")?;
    require(
        !["run", "definition", "actions", "delivery_scope"]
            .iter()
            .any(|key| data.contains_key(*key)),
        "details attempts to overwrite lifecycle identity",
    )
}

fn current_phase_action(raw_payload: &Value, run: &Value) -> FixtureResult<Option<Value>> {
    let action = if run["current_phase_id"].is_string() {
        require(
            !matches!(run["status"].as_str(), Some("completed" | "escalated")),
            "terminal run retains a current phase",
        )?;
        require(
            run["current_phase_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
            "empty current phase ID",
        )?;
        Some(destination(raw_payload, run, "phase_contract")?)
    } else {
        require(
            run.get("current_phase_id").is_some_and(Value::is_null)
                && matches!(run["status"].as_str(), Some("completed" | "escalated")),
            "nonterminal run missing current phase",
        )?;
        require(
            !raw_payload["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| {
                    action["tool"] == "query"
                        && action["arguments"]["route"] == ROUTE
                        && action["arguments"]["params"]["view"] == "phase_contract"
                }),
            "null-phase run advertises a phase contract",
        )?;
        None
    };
    Ok(action)
}

/// Reads only the origin's actual advertised destinations. It never rewrites the origin.
/// Completed/escalated runs with null current_phase_id have snapshot/details only.
pub async fn resolve_pipeline(
    client: &mut Mcp,
    raw_payload: Value,
) -> FixtureResult<ResolvedPipeline> {
    let compact_context = compact_context(&raw_payload)?;
    let run = &compact_context["run"];
    let snapshot_action = destination(&raw_payload, run, "snapshot")?;
    let details_action = destination(&raw_payload, run, "details")?;
    let phase_action = current_phase_action(&raw_payload, run)?;
    let snapshot = bytes::read(client, &snapshot_action).await?;
    verify_snapshot(&compact_context, &snapshot.value)?;
    let details = bytes::read(client, &details_action).await?;
    verify_details(&compact_context, &details.value)?;
    let phase_contract = if let Some(action) = phase_action {
        let phase = bytes::read(client, &action).await?;
        verify_phase(&compact_context, &snapshot.value, &phase.value)?;
        Some(phase)
    } else {
        None
    };
    Ok(ResolvedPipeline {
        raw_payload,
        compact_context,
        snapshot,
        phase_contract,
        details,
    })
}
