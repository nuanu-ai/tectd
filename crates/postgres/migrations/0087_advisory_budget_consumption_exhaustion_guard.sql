-- Do not trust a caller-supplied "not exhausted" verdict. Recompute it from
-- the sealed measurements and the frozen policy, including unknown usage.
CREATE FUNCTION advisory_budget_consumption_exhaustion_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE
    reservation public.advisory_budget_reservations%ROWTYPE;
    input_limit bigint;
    output_limit bigint;
    elapsed_limit bigint;
    prior_input numeric;
    prior_output numeric;
    prior_elapsed numeric;
    prior_exhausted boolean;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'budget consumption is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'budget consumption tenant mismatch' USING ERRCODE='42501';
    END IF;
    -- This lock serializes sibling consumption even for direct SQL writers.
    PERFORM 1 FROM public.advisory_opportunity WHERE
      (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
      FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'budget consumption opportunity missing' USING ERRCODE='23514';
    END IF;
    SELECT * INTO reservation FROM public.advisory_budget_reservations WHERE
      (tenant_id,workspace_id,dispatch_id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.dispatch_id);
    IF NOT FOUND OR reservation.opportunity_id <> NEW.opportunity_id THEN
        RAISE EXCEPTION 'budget consumption reservation mismatch' USING ERRCODE='23514';
    END IF;
    SELECT p.input_tokens,p.output_tokens,p.elapsed_monotonic_ms
      INTO input_limit,output_limit,elapsed_limit
      FROM public.advisory_budget_policies p WHERE
      (p.tenant_id,p.workspace_id,p.id,p.version,p.digest)=
      (NEW.tenant_id,NEW.workspace_id,reservation.policy_id,
       reservation.policy_version,reservation.policy_digest);
    IF NOT FOUND THEN
        RAISE EXCEPTION 'budget consumption policy missing' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE(SUM(c.input_tokens::numeric),0),
           COALESCE(SUM(c.output_tokens::numeric),0),
           COALESCE(SUM(c.monotonic_elapsed_ms::numeric),0),
           COALESCE(BOOL_OR(c.exhausted_after_response),false)
      INTO prior_input,prior_output,prior_elapsed,prior_exhausted
      FROM public.advisory_budget_consumptions c WHERE
      (c.tenant_id,c.workspace_id,c.opportunity_id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id);
    IF NOT NEW.exhausted_after_response AND
       (NEW.unknown_usage OR prior_exhausted
        OR NEW.input_tokens IS NULL OR NEW.output_tokens IS NULL
        OR NEW.monotonic_elapsed_ms IS NULL
        OR NEW.input_tokens > reservation.reserved_input_tokens
        OR NEW.output_tokens > reservation.reserved_output_tokens
        OR NEW.monotonic_elapsed_ms > reservation.remaining_elapsed_ms
        OR prior_input + NEW.input_tokens::numeric > input_limit::numeric
        OR prior_output + NEW.output_tokens::numeric > output_limit::numeric
        OR prior_elapsed + NEW.monotonic_elapsed_ms::numeric > elapsed_limit::numeric) THEN
        RAISE EXCEPTION 'budget consumption exhaustion verdict is false'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER z_advisory_budget_consumption_exhaustion_guard
    BEFORE INSERT ON advisory_budget_consumptions FOR EACH ROW
    EXECUTE FUNCTION advisory_budget_consumption_exhaustion_guard();
REVOKE ALL PRIVILEGES ON FUNCTION advisory_budget_consumption_exhaustion_guard() FROM PUBLIC;
