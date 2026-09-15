use super::*;
use std::collections::BTreeSet;

pub(super) async fn validate_specification_finding_lineage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    output: &PipelinePhaseOutputDraft,
    prior_phase_ids: &[String],
    reconciliation_phase_id: &str,
) -> Result<()> {
    let report = output
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "engineering-review.json")
        .and_then(|artifact| serde_json::from_str::<serde_json::Value>(&artifact.body).ok())
        .ok_or(Error::InvalidArguments)?;
    let declared = |field: &str| -> Result<BTreeSet<String>> {
        report
            .get(field)
            .and_then(serde_json::Value::as_array)
            .ok_or(Error::InvalidArguments)?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or(Error::InvalidArguments)
            })
            .collect()
    };
    let prior_declared = declared("prior_finding_ids")?;
    let resolved_declared = declared("resolved_finding_ids")?;
    let mut historical = BTreeSet::new();
    for phase_id in prior_phase_ids {
        let artifact_sets: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT artifacts FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 AND NOT payload_erased ORDER BY revision",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(run)
        .bind(phase_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        for artifacts in artifact_sets {
            let Some(body) = artifacts
                .as_array()
                .and_then(|items| {
                    items.iter().find(|item| {
                        item.get("name").and_then(serde_json::Value::as_str)
                            == Some("engineering-review.json")
                    })
                })
                .and_then(|item| item.get("body"))
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let prior_report: serde_json::Value =
                serde_json::from_str(body).map_err(|_| Error::InternalInvariant)?;
            if let Some(findings) = prior_report
                .get("findings")
                .and_then(serde_json::Value::as_array)
            {
                for finding in findings {
                    let id = finding
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or(Error::InternalInvariant)?;
                    historical.insert(id.to_owned());
                }
            }
        }
    }
    let reconciliation_fields: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT o.fields FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.stale=false AND NOT o.payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(reconciliation_phase_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let fields = reconciliation_fields.ok_or(Error::Forbidden)?;
    let field_set = |field: &str| -> Result<BTreeSet<String>> {
        let encoded = fields
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::InvalidArguments)?;
        serde_json::from_str::<Vec<String>>(encoded)
            .map_err(|_| Error::InvalidArguments)?
            .into_iter()
            .map(|value| {
                if value.trim().is_empty() {
                    Err(Error::InvalidArguments)
                } else {
                    Ok(value)
                }
            })
            .collect()
    };
    let reconciliation_ids = field_set("engineering_finding_ids")?;
    let resolved_ids = field_set("resolved_engineering_finding_ids")?;
    let deferred_ids = field_set("deferred_engineering_finding_ids")?;
    let unresolved = fields
        .get("unresolved_engineering_finding_count")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(Error::InvalidArguments)?;
    let authority = fields
        .get("engineering_finding_authority")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(Error::InvalidArguments)?;
    let dispositioned = resolved_ids
        .union(&deferred_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let valid_deferred = deferred_ids.is_empty()
        || !matches!(authority.trim(), "none" | "not_applicable" | "not-required");
    if validate_finding_sets(
        &historical,
        &reconciliation_ids,
        &resolved_ids,
        &deferred_ids,
        unresolved,
        valid_deferred,
        &prior_declared,
        &resolved_declared,
    ) && dispositioned == historical
    {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_finding_sets(
    cross_cutting: &BTreeSet<String>,
    reconciliation: &BTreeSet<String>,
    resolved: &BTreeSet<String>,
    deferred: &BTreeSet<String>,
    unresolved_count: usize,
    deferred_has_authority: bool,
    readiness_prior: &BTreeSet<String>,
    readiness_resolved: &BTreeSet<String>,
) -> bool {
    let dispositioned = resolved.union(deferred).cloned().collect::<BTreeSet<_>>();
    cross_cutting == reconciliation
        && resolved.is_disjoint(deferred)
        && unresolved_count == 0
        && deferred_has_authority
        && dispositioned == *cross_cutting
        && readiness_prior == cross_cutting
        && readiness_resolved == &dispositioned
}

#[cfg(test)]
mod engineering_finding_tests {
    use super::*;

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn readiness_cannot_self_claim_a_finding_omitted_by_reconciliation() {
        assert!(!validate_finding_sets(
            &set(&["F-1"]),
            &set(&[]),
            &set(&[]),
            &set(&[]),
            0,
            true,
            &set(&["F-1"]),
            &set(&["F-1"]),
        ));
    }

    #[test]
    fn exact_reconciliation_resolution_authorizes_readiness_lineage() {
        assert!(validate_finding_sets(
            &set(&["F-1"]),
            &set(&["F-1"]),
            &set(&["F-1"]),
            &set(&[]),
            0,
            true,
            &set(&["F-1"]),
            &set(&["F-1"]),
        ));
    }
}
