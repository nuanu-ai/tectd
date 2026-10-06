-- Extend the same policy-wide serialization and accounting to Model Route.
CREATE OR REPLACE FUNCTION advisory_budget_policy_usage_totals(
    p_tenant uuid, p_workspace uuid, p_policy uuid, p_version bigint, p_digest text
) RETURNS TABLE (calls bigint,request_bytes bigint,retries bigint,input_tokens bigint,
                 output_tokens bigint,elapsed_ms bigint,pending bigint,invalid bigint)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,public AS $usage$
    SELECT COUNT(*)::bigint,COALESCE(SUM(a.request_bytes),0)::bigint,
           COALESCE(SUM(a.retries),0)::bigint,COALESCE(SUM(a.input_tokens),0)::bigint,
           COALESCE(SUM(a.output_tokens),0)::bigint,COALESCE(SUM(a.elapsed_ms),0)::bigint,
           COUNT(*) FILTER (WHERE NOT a.consumed)::bigint,
           COUNT(*) FILTER (WHERE a.unknown_usage OR a.exhausted)::bigint
    FROM (
        SELECT r.request_utf8_bytes AS request_bytes,r.reserved_retry_dispatches AS retries,
               c.input_tokens,c.output_tokens,c.monotonic_elapsed_ms AS elapsed_ms,
               c.dispatch_id IS NOT NULL AS consumed,
               COALESCE(c.unknown_usage,false) AS unknown_usage,
               COALESCE(c.exhausted_after_response,false) AS exhausted
        FROM public.advisory_budget_reservations r
        LEFT JOIN public.advisory_budget_consumptions c
          ON (c.tenant_id,c.workspace_id,c.dispatch_id)=(r.tenant_id,r.workspace_id,r.dispatch_id)
        WHERE (r.tenant_id,r.workspace_id,r.policy_id,r.policy_version,r.policy_digest)=
              (p_tenant,p_workspace,p_policy,p_version,p_digest)
        UNION ALL
        SELECT r.request_utf8_bytes,0,c.input_tokens,c.output_tokens,c.elapsed_monotonic_ms,
               c.review_id IS NOT NULL,COALESCE(c.unknown_usage,false),
               COALESCE(c.exhausted_after_response,false)
        FROM public.scope_anti_bloat_budget_reservations r
        LEFT JOIN public.scope_anti_bloat_budget_consumptions c
          ON (c.tenant_id,c.workspace_id,c.review_id)=(r.tenant_id,r.workspace_id,r.review_id)
        WHERE (r.tenant_id,r.workspace_id,r.policy_id,r.policy_version,r.policy_digest)=
              (p_tenant,p_workspace,p_policy,p_version,p_digest)
        UNION ALL
        SELECT r.request_utf8_bytes,0,c.input_tokens,c.output_tokens,c.elapsed_monotonic_ms,
               c.attempt_id IS NOT NULL,COALESCE(c.unknown_usage,false),
               COALESCE(c.exhausted_after_response,false)
        FROM public.model_route_budget_reservations r
        LEFT JOIN public.model_route_budget_consumptions c
          ON (c.tenant_id,c.workspace_id,c.attempt_id)=(r.tenant_id,r.workspace_id,r.attempt_id)
        WHERE (r.tenant_id,r.workspace_id,r.policy_id,r.policy_version,r.policy_digest)=
              (p_tenant,p_workspace,p_policy,p_version,p_digest)
    ) a
$usage$;


CREATE TRIGGER zz_anti_bloat_budget_global_reservation_guard
    BEFORE INSERT ON scope_anti_bloat_budget_reservations FOR EACH ROW
    EXECUTE FUNCTION advisory_budget_global_reservation_guard();
CREATE TRIGGER zz_anti_bloat_budget_global_consumption_guard
    BEFORE INSERT ON scope_anti_bloat_budget_consumptions FOR EACH ROW
    EXECUTE FUNCTION advisory_budget_global_consumption_guard();
