use super::*;
pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
) -> Result<()> {
    for statement in [
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_bindings, \
             scope_anti_bloat_reviews, scope_anti_bloat_caller_links FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE scope_anti_bloat_bindings TO {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_reviews, \
             scope_anti_bloat_caller_links TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_preservation_attestations FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_preservation_attestations TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_budget_reservations TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_budget_consumptions TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(state,request_bytes,request_sha256,request_adapter_identity,ranked_ids,raw_response,response_sha256,response_sealed_at,response_http_status,response_original_input_tokens,response_original_output_tokens,response_original_elapsed_ms,response_complete,original_transport_context,send_started_at,sealed_at) \
             ON TABLE scope_anti_bloat_reviews TO {quoted_role}"
        ),
    ] {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}
