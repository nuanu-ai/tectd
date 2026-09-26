include!("advisory/rows.rs");
include!("advisory/config_opportunity.rs");
include!("advisory/dispatch_source.rs");
include!("advisory/dispatch.rs");
include!("advisory/pipeline_dispatch.rs");
include!("advisory/matrix_dispatch_read.rs");
include!("advisory/provider_observation.rs");
include!("advisory/matrix_observation.rs");
include!("advisory/audit_store.rs");
include!("advisory/tests.rs");

// Source-contract index retained for compile-time source-inspection tests after the split.
// FROM advisory_workspace_config
// INSERT INTO advisory_workspace_config_history
// revision,previous_revision
// ON CONFLICT DO NOTHING
// FOR UPDATE
// AND revision=$9
// INSERT INTO advisory_opportunity
// ON CONFLICT(tenant_id,workspace_id,request_key) DO NOTHING
// row.material_digest != input.material_digest
// current_revision != input.config_revision
// INSERT INTO advisory_dispatch
// state='sending',send_certainty='sent_unknown'
// send_started_at=pg_catalog.clock_timestamp()
// WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'
// state='sealed'
// sealed_at=pg_catalog.clock_timestamp()
// dispatch_matches_authorization
// advisory_retry_permitted
// current.0 != expected_config_revision
// AdvisoryOpportunityState::Invalidated
// AdvisoryOpportunityState::Unresolved
// AdvisoryCancellationOutcome::DeliveryMayHaveOccurred
// dispatch_by_id(tx, tenant, workspace, dispatch_id, true)
// id=$3 AND state='authorized'
// AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => false
// AdvisoryReconciliationEvidence::Inconclusive
// AdvisoryReconciliationEvidence::ConfirmedSent
// AdvisoryReconciliationEvidence::ConfirmedNotSent
// state='sending' AND send_certainty='sent_unknown'
// previous.material_digest != input.material_digest
// previous.payload_digest != input.payload_digest
// child_exists
// AdvisoryDispatchState::Cancelled | AdvisoryDispatchState::Sealed
// async fn audit(
// o.tenant_id=$1 AND o.workspace_id=$2
// ($3::uuid IS NULL OR o.scope_id=$3)
// (o.created_at,o.id)<
// ORDER BY o.created_at DESC,o.id DESC
// LIMIT $9
// d.opportunity_id=ANY($3)
// send_certainty='sent_unknown'
// send_certainty='not_sent' AND d.state IN ('sealed','cancelled')
// attempts_with_unknown_token_usage
// GROUP BY o.primary_reason ORDER BY o.primary_reason
// octet_length(d.request_payload)::bigint AS request_bytes
// async fn opportunity_detail(
// o.scope_id=$3 AND o.id=$4
// ORDER BY d.attempt_number,d.id
