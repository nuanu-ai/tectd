-- A model-route send spends the same immutable workspace policy as the other
-- adviser routes. These rows are append-only and bound to the frozen attempt.
CREATE TABLE model_route_budget_reservations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL CHECK (policy_digest ~ '^[0-9a-f]{64}$'),
    request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    reserved_input_tokens bigint NOT NULL CHECK (reserved_input_tokens > 0),
    reserved_output_tokens bigint NOT NULL CHECK (reserved_output_tokens > 0),
    reserved_elapsed_ms bigint NOT NULL CHECK (reserved_elapsed_ms > 0),
    reserved_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,attempt_id),
    FOREIGN KEY (tenant_id,workspace_id,attempt_id)
        REFERENCES model_route_advisory_attempts (tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,workspace_id,policy_id)
        REFERENCES advisory_budget_policies (tenant_id,workspace_id,id)
);
CREATE TABLE model_route_budget_consumptions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL CHECK (policy_digest ~ '^[0-9a-f]{64}$'),
    request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    response_sha256 text CHECK (response_sha256 ~ '^[0-9a-f]{64}$'),
    input_tokens bigint CHECK (input_tokens >= 0),
    output_tokens bigint CHECK (output_tokens >= 0),
    elapsed_monotonic_ms bigint CHECK (elapsed_monotonic_ms >= 0),
    unknown_usage boolean NOT NULL,
    exhausted_after_response boolean NOT NULL,
    transport_failed boolean NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,attempt_id),
    FOREIGN KEY (tenant_id,workspace_id,attempt_id)
        REFERENCES model_route_budget_reservations (tenant_id,workspace_id,attempt_id),
    CHECK (unknown_usage = (input_tokens IS NULL OR output_tokens IS NULL OR elapsed_monotonic_ms IS NULL)),
    CHECK (NOT transport_failed OR (response_sha256 IS NULL AND unknown_usage AND exhausted_after_response))
);

CREATE FUNCTION model_route_budget_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE a public.model_route_advisory_attempts%ROWTYPE;
DECLARE p public.advisory_budget_policies%ROWTYPE;
DECLARE r public.model_route_budget_reservations%ROWTYPE;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'model-route budget audit is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'model-route budget tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT * INTO a FROM public.model_route_advisory_attempts WHERE
      (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.attempt_id);
    IF TG_TABLE_NAME='model_route_budget_reservations' THEN
        SELECT * INTO p FROM public.advisory_budget_policies WHERE
          (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.policy_id);
        IF a.id IS NULL OR p.id IS NULL OR a.state<>'send_unknown'
            OR NEW.policy_version<>p.version OR NEW.policy_digest<>p.digest
            OR NEW.request_sha256<>a.request_sha256
            OR NEW.request_utf8_bytes<>pg_catalog.octet_length(a.request_payload)
            OR NEW.request_sha256<>pg_catalog.encode(pg_catalog.sha256(a.request_payload),'hex')
            OR NEW.reserved_input_tokens>p.input_tokens
            OR NEW.reserved_output_tokens>p.output_tokens
            OR NEW.reserved_elapsed_ms>p.elapsed_monotonic_ms
            OR NEW.request_utf8_bytes>p.request_utf8_bytes
            OR (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint
               NOT BETWEEN p.effective_from_unix_ms AND p.effective_until_unix_ms-1
        THEN
            RAISE EXCEPTION 'model-route reservation binding mismatch' USING ERRCODE='23514';
        END IF;
    ELSE
        SELECT * INTO r FROM public.model_route_budget_reservations WHERE
          (tenant_id,workspace_id,attempt_id)=(NEW.tenant_id,NEW.workspace_id,NEW.attempt_id);
        IF a.id IS NULL OR r.attempt_id IS NULL
            OR NEW.policy_id<>r.policy_id OR NEW.policy_version<>r.policy_version
            OR NEW.policy_digest<>r.policy_digest OR NEW.request_sha256<>r.request_sha256
            OR (NOT NEW.transport_failed AND (a.raw_sealed_at IS NULL
                OR NEW.response_sha256<>a.response_sha256))
            OR (NEW.transport_failed AND (a.state<>'send_unknown' OR a.response_payload IS NOT NULL))
            OR NEW.exhausted_after_response IS DISTINCT FROM
               (NEW.unknown_usage OR COALESCE(NEW.input_tokens>r.reserved_input_tokens,false)
                OR COALESCE(NEW.output_tokens>r.reserved_output_tokens,false)
                OR COALESCE(NEW.elapsed_monotonic_ms>r.reserved_elapsed_ms,false))
        THEN
            RAISE EXCEPTION 'model-route consumption seal mismatch' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER model_route_budget_reservation_guard
    BEFORE INSERT OR UPDATE OR DELETE ON model_route_budget_reservations
    FOR EACH ROW EXECUTE FUNCTION model_route_budget_guard();
CREATE TRIGGER model_route_budget_consumption_guard
    BEFORE INSERT OR UPDATE OR DELETE ON model_route_budget_consumptions
    FOR EACH ROW EXECUTE FUNCTION model_route_budget_guard();

-- A parsed ranking and a visible recommendation require recorded, known usage.
CREATE FUNCTION model_route_budget_visibility_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
BEGIN
    IF TG_TABLE_NAME='model_route_advisory_attempts' THEN
        IF NEW.state='parsed' AND NOT EXISTS (
            SELECT 1 FROM public.model_route_budget_consumptions c WHERE
              (c.tenant_id,c.workspace_id,c.attempt_id)=(NEW.tenant_id,NEW.workspace_id,NEW.id)
              AND NOT c.unknown_usage AND NOT c.exhausted_after_response
        ) THEN RAISE EXCEPTION 'model-route budget consumption missing or exhausted' USING ERRCODE='23514'; END IF;
    ELSIF NEW.outcome_kind='recommended' AND NOT EXISTS (
        SELECT 1 FROM public.model_route_advisory_attempts a
        JOIN public.model_route_budget_consumptions c ON
          (c.tenant_id,c.workspace_id,c.attempt_id)=(a.tenant_id,a.workspace_id,a.id)
        WHERE (a.tenant_id,a.workspace_id,a.preparation_request_key)=
              (NEW.tenant_id,NEW.workspace_id,NEW.preparation_request_key)
          AND a.state='parsed' AND NOT c.unknown_usage AND NOT c.exhausted_after_response
    ) THEN RAISE EXCEPTION 'model-route recommendation lacks budget proof' USING ERRCODE='23514'; END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER zz_model_route_budget_parse_guard BEFORE UPDATE ON model_route_advisory_attempts
    FOR EACH ROW EXECUTE FUNCTION model_route_budget_visibility_guard();
CREATE TRIGGER model_route_budget_decision_guard BEFORE INSERT ON model_route_decisions
    FOR EACH ROW EXECUTE FUNCTION model_route_budget_visibility_guard();

ALTER TABLE model_route_budget_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE model_route_budget_reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY model_route_budget_reservation_tenant_scope ON model_route_budget_reservations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
ALTER TABLE model_route_budget_consumptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE model_route_budget_consumptions FORCE ROW LEVEL SECURITY;
CREATE POLICY model_route_budget_consumption_tenant_scope ON model_route_budget_consumptions
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE model_route_budget_reservations,model_route_budget_consumptions FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION model_route_budget_guard() FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION model_route_budget_visibility_guard() FROM PUBLIC;
