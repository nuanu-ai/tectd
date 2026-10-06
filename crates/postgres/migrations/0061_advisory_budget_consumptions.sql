CREATE TABLE advisory_budget_consumptions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL,
    request_sha256 text NOT NULL,
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    response_sha256 text CHECK (response_sha256 ~ '^[0-9a-f]{64}$'),
    raw_response_ref text,
    send_certainty text NOT NULL CHECK (send_certainty IN ('sent','sent_unknown','not_sent')),
    outcome text NOT NULL CHECK (outcome IN ('provider_response','provider_failure')),
    input_tokens bigint CHECK (input_tokens >= 0),
    output_tokens bigint CHECK (output_tokens >= 0),
    input_tokens_known boolean NOT NULL,
    output_tokens_known boolean NOT NULL,
    monotonic_elapsed_ms bigint CHECK (monotonic_elapsed_ms >= 0),
    elapsed_known boolean NOT NULL,
    calls bigint NOT NULL CHECK (calls = 1),
    retry_dispatches bigint NOT NULL CHECK (retry_dispatches IN (0,1)),
    unknown_usage boolean NOT NULL,
    exhausted_after_response boolean NOT NULL,
    sealed_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,dispatch_id)
        REFERENCES advisory_budget_reservations (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    CHECK (input_tokens_known = (input_tokens IS NOT NULL)),
    CHECK (output_tokens_known = (output_tokens IS NOT NULL)),
    CHECK (elapsed_known = (monotonic_elapsed_ms IS NOT NULL)),
    CHECK (unknown_usage = (NOT input_tokens_known OR NOT output_tokens_known OR NOT elapsed_known))
);
CREATE INDEX advisory_budget_consumptions_opportunity_idx
    ON advisory_budget_consumptions (tenant_id,workspace_id,opportunity_id);

CREATE FUNCTION advisory_budget_consumption_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE dispatch public.advisory_dispatch%ROWTYPE;
DECLARE reservation public.advisory_budget_reservations%ROWTYPE;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'budget consumption is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'budget consumption tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT * INTO dispatch FROM public.advisory_dispatch WHERE
      (tenant_id,workspace_id,opportunity_id,id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id);
    SELECT * INTO reservation FROM public.advisory_budget_reservations WHERE
      (tenant_id,workspace_id,dispatch_id)=
      (NEW.tenant_id,NEW.workspace_id,NEW.dispatch_id);
    IF dispatch.id IS NULL OR reservation.dispatch_id IS NULL
       OR dispatch.state <> 'sealed' OR dispatch.sealed_at IS NULL
       OR NEW.policy_id <> reservation.policy_id
       OR NEW.policy_version <> reservation.policy_version
       OR NEW.policy_digest <> reservation.policy_digest
       OR NEW.request_sha256 <> reservation.request_sha256
       OR NEW.request_utf8_bytes <> reservation.request_utf8_bytes
       OR (NEW.input_tokens IS NOT NULL AND NEW.input_tokens > reservation.reserved_input_tokens
           AND NOT NEW.exhausted_after_response)
       OR (NEW.output_tokens IS NOT NULL AND NEW.output_tokens > reservation.reserved_output_tokens
           AND NOT NEW.exhausted_after_response)
       OR NEW.calls <> reservation.reserved_calls
       OR NEW.retry_dispatches <> reservation.reserved_retry_dispatches
       OR NEW.send_certainty <> dispatch.send_certainty
       OR NEW.outcome <> dispatch.outcome
       OR (dispatch.response_payload IS NULL AND NEW.response_sha256 IS NOT NULL)
       OR (dispatch.response_payload IS NOT NULL AND NEW.response_sha256 IS DISTINCT FROM
           pg_catalog.encode(pg_catalog.sha256(dispatch.response_payload),'hex'))
       OR NEW.raw_response_ref IS DISTINCT FROM dispatch.raw_response_ref
       OR NEW.input_tokens IS DISTINCT FROM dispatch.input_tokens
       OR NEW.output_tokens IS DISTINCT FROM dispatch.output_tokens
       OR NEW.monotonic_elapsed_ms IS DISTINCT FROM dispatch.latency_ms
       OR NEW.sealed_at IS DISTINCT FROM dispatch.sealed_at THEN
        RAISE EXCEPTION 'budget consumption seal mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END; $guard$;
CREATE TRIGGER advisory_budget_consumption_guard_trigger
    BEFORE INSERT OR UPDATE OR DELETE ON advisory_budget_consumptions
    FOR EACH ROW EXECUTE FUNCTION advisory_budget_consumption_guard();

ALTER TABLE advisory_budget_consumptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_budget_consumptions FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_budget_consumption_tenant_scope ON advisory_budget_consumptions
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_budget_consumptions FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_budget_consumption_guard() FROM PUBLIC;


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
