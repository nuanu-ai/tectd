use super::*;
use std::collections::BTreeSet;

pub struct ResolvedPipelineMetadata {
    pub raw_payload: Value,
    pub compact_context: Value,
    pub snapshot: ResolvedRead,
    pub history: ResolvedRead,
    pub phase_contract: Option<ResolvedRead>,
}
impl ResolvedPipelineMetadata {
    pub fn run(&self) -> &Value {
        &self.compact_context["run"]
    }
    pub fn definition(&self) -> &Value {
        &self.snapshot.value["definition"]
    }
    pub fn history_data(&self) -> &Value {
        &self.history.value["data"]
    }
}

/// Retains the compact origin and reads explicit History without output bodies.
pub async fn resolve_pipeline_metadata(
    client: &mut Mcp,
    raw_payload: Value,
) -> FixtureResult<ResolvedPipelineMetadata> {
    let compact_context = compact_context(&raw_payload)?;
    let run = &compact_context["run"];
    let snapshot_action = destination(&raw_payload, run, "snapshot")?;
    let phase_action = current_phase_action(&raw_payload, run)?;
    let snapshot = bytes::read(client, &snapshot_action).await?;
    verify_snapshot(&compact_context, &snapshot.value)?;
    let arguments = json!({"route":ROUTE,"params":{"run_id":run["id"],"view":"details",
        "run_revision":run["revision"],"section":"history"}});
    let history = bytes::read_query(client, &arguments).await?;
    require(
        history.value["run_id"] == run["id"]
            && history.value["run_revision"] == run["revision"]
            && history.value["section"] == "history",
        "History pins differ from origin",
    )?;
    let keys = history.value["data"]
        .as_object()
        .ok_or("History data missing")?
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    require(
        keys == BTreeSet::from([
            "attempts",
            "checkpoints",
            "inquiry",
            "result",
            "source_checkpoint",
            "qualification_reason",
            "delivered_phases",
            "delivery_receipt",
        ]),
        "History fields changed",
    )?;
    let phase_contract = if let Some(action) = phase_action {
        let phase = bytes::read(client, &action).await?;
        verify_phase(&compact_context, &snapshot.value, &phase.value)?;
        Some(phase)
    } else {
        None
    };
    Ok(ResolvedPipelineMetadata {
        raw_payload,
        compact_context,
        snapshot,
        history,
        phase_contract,
    })
}
