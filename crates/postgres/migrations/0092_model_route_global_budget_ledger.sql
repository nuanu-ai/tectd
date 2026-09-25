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

CREATE OR REPLACE FUNCTION advisory_budget_global_reservation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE p public.advisory_budget_policies%ROWTYPE;
DECLARE u record;
DECLARE attempt_retries bigint;
DECLARE held_elapsed bigint;
BEGIN
    IF TG_OP <> 'INSERT' OR NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'global budget reservation is not authorized' USING ERRCODE='42501';
    END IF;
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'global budget reservation requires read committed' USING ERRCODE='23514';
    END IF;
    SELECT * INTO p FROM public.advisory_budget_policies WHERE
      (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.policy_id) FOR UPDATE;
    IF NOT FOUND OR p.version<>NEW.policy_version OR p.digest<>NEW.policy_digest THEN
        RAISE EXCEPTION 'global budget policy mismatch' USING ERRCODE='23514';
    END IF;
    SELECT * INTO u FROM public.advisory_budget_policy_usage_totals(
        NEW.tenant_id,NEW.workspace_id,NEW.policy_id,NEW.policy_version,NEW.policy_digest);
    IF TG_TABLE_NAME='advisory_budget_reservations' THEN
        attempt_retries := NEW.reserved_retry_dispatches;
        held_elapsed := NEW.remaining_elapsed_ms;
    ELSE
        attempt_retries := 0;
        held_elapsed := NEW.reserved_elapsed_ms;
    END IF;
    IF u.pending<>0 OR u.invalid<>0 OR u.calls+1>p.provider_calls
       OR u.request_bytes+NEW.request_utf8_bytes>p.request_utf8_bytes
       OR u.retries+attempt_retries>p.retry_dispatches
       OR u.input_tokens>=p.input_tokens OR u.output_tokens>=p.output_tokens
       OR u.elapsed_ms>=p.elapsed_monotonic_ms
       OR NEW.reserved_input_tokens>p.input_tokens-u.input_tokens
       OR NEW.reserved_output_tokens>p.output_tokens-u.output_tokens
       OR held_elapsed>p.elapsed_monotonic_ms-u.elapsed_ms THEN
        RAISE EXCEPTION 'global budget exhausted before dispatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END; $guard$;
CREATE TRIGGER zz_model_route_budget_global_reservation_guard
    BEFORE INSERT ON model_route_budget_reservations FOR EACH ROW
    EXECUTE FUNCTION advisory_budget_global_reservation_guard();

CREATE OR REPLACE FUNCTION advisory_budget_global_consumption_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE p public.advisory_budget_policies%ROWTYPE;
DECLARE u record;
DECLARE elapsed bigint;
BEGIN
    IF TG_OP <> 'INSERT' OR NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'global budget consumption is not authorized' USING ERRCODE='42501';
    END IF;
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'global budget consumption requires read committed' USING ERRCODE='23514';
    END IF;
    SELECT * INTO p FROM public.advisory_budget_policies WHERE
      (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.policy_id) FOR UPDATE;
    IF NOT FOUND OR p.version<>NEW.policy_version OR p.digest<>NEW.policy_digest THEN
        RAISE EXCEPTION 'global budget consumption policy mismatch' USING ERRCODE='23514';
    END IF;
    SELECT * INTO u FROM public.advisory_budget_policy_usage_totals(
        NEW.tenant_id,NEW.workspace_id,NEW.policy_id,NEW.policy_version,NEW.policy_digest);
    IF TG_TABLE_NAME='advisory_budget_consumptions' THEN
        elapsed := NEW.monotonic_elapsed_ms;
    ELSE
        elapsed := NEW.elapsed_monotonic_ms;
    END IF;
    IF u.pending<>1 THEN
        RAISE EXCEPTION 'global budget has a different pending attempt' USING ERRCODE='23514';
    END IF;
    IF NOT NEW.exhausted_after_response AND
       (NEW.unknown_usage OR u.invalid<>0 OR NEW.input_tokens IS NULL
        OR NEW.output_tokens IS NULL OR elapsed IS NULL
        OR u.input_tokens+NEW.input_tokens>p.input_tokens
        OR u.output_tokens+NEW.output_tokens>p.output_tokens
        OR u.elapsed_ms+elapsed>p.elapsed_monotonic_ms) THEN
        RAISE EXCEPTION 'global budget exhaustion verdict is false' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END; $guard$;
CREATE TRIGGER zz_model_route_budget_global_consumption_guard
    BEFORE INSERT ON model_route_budget_consumptions FOR EACH ROW
    EXECUTE FUNCTION advisory_budget_global_consumption_guard();
