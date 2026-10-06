-- Anti-Bloat has its own one-use review fence; advisory dispatch budget rows
-- cannot reference it. Both rows are immutable and bound to the exact review.
CREATE TABLE scope_anti_bloat_budget_reservations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    review_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL CHECK (policy_digest ~ '^[0-9a-f]{64}$'),
    request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    reserved_input_tokens bigint NOT NULL CHECK (reserved_input_tokens > 0),
    reserved_output_tokens bigint NOT NULL CHECK (reserved_output_tokens > 0),
    reserved_elapsed_ms bigint NOT NULL CHECK (reserved_elapsed_ms > 0),
    reserved_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,review_id),
    FOREIGN KEY (tenant_id,workspace_id,review_id)
        REFERENCES scope_anti_bloat_reviews (tenant_id,workspace_id,review_id),
    FOREIGN KEY (opportunity_id) REFERENCES advisory_opportunity (id),
    FOREIGN KEY (tenant_id,workspace_id,policy_id)
        REFERENCES advisory_budget_policies (tenant_id,workspace_id,id)
);
CREATE INDEX scope_anti_bloat_budget_reservation_opportunity_idx
    ON scope_anti_bloat_budget_reservations (tenant_id,workspace_id,opportunity_id);

CREATE TABLE scope_anti_bloat_budget_consumptions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    review_id uuid NOT NULL,
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
    PRIMARY KEY (tenant_id,workspace_id,review_id),
    FOREIGN KEY (tenant_id,workspace_id,review_id)
        REFERENCES scope_anti_bloat_budget_reservations (tenant_id,workspace_id,review_id),
    CHECK (unknown_usage = (input_tokens IS NULL OR output_tokens IS NULL OR elapsed_monotonic_ms IS NULL)),
    CHECK (NOT transport_failed OR (response_sha256 IS NULL AND unknown_usage AND exhausted_after_response))
);

CREATE FUNCTION scope_anti_bloat_budget_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE review public.scope_anti_bloat_reviews%ROWTYPE;
DECLARE policy public.advisory_budget_policies%ROWTYPE;
DECLARE reservation public.scope_anti_bloat_budget_reservations%ROWTYPE;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'anti-bloat budget audit is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'anti-bloat budget tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT * INTO review FROM public.scope_anti_bloat_reviews WHERE
        (tenant_id,workspace_id,review_id)=(NEW.tenant_id,NEW.workspace_id,NEW.review_id);
    IF TG_TABLE_NAME='scope_anti_bloat_budget_reservations' THEN
        SELECT * INTO policy FROM public.advisory_budget_policies WHERE
            (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.policy_id);
        IF review.review_id IS NULL OR policy.id IS NULL OR review.state<>'sending'
            OR NOT EXISTS (SELECT 1 FROM public.scope_anti_bloat_bindings b WHERE
                (b.tenant_id,b.workspace_id,b.candidate_set_id,b.candidate_set_revision,b.opportunity_id)=
                (review.tenant_id,review.workspace_id,review.candidate_set_id,review.candidate_set_revision,NEW.opportunity_id))
            OR NEW.policy_version<>policy.version OR NEW.policy_digest<>policy.digest
            OR NEW.request_sha256<>review.request_sha256
            OR NEW.request_utf8_bytes<>pg_catalog.octet_length(review.request_bytes)
            OR NEW.request_sha256<>pg_catalog.encode(pg_catalog.sha256(review.request_bytes),'hex')
            OR NEW.reserved_input_tokens>policy.input_tokens
            OR NEW.reserved_output_tokens>policy.output_tokens
            OR NEW.reserved_elapsed_ms>policy.elapsed_monotonic_ms
            OR policy.provider_calls<1
            OR NEW.request_utf8_bytes>policy.request_utf8_bytes
            OR (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint
                NOT BETWEEN policy.effective_from_unix_ms AND policy.effective_until_unix_ms-1
        THEN
            RAISE EXCEPTION 'anti-bloat reservation binding mismatch' USING ERRCODE='23514';
        END IF;
    ELSE
        SELECT * INTO reservation FROM public.scope_anti_bloat_budget_reservations WHERE
            (tenant_id,workspace_id,review_id)=(NEW.tenant_id,NEW.workspace_id,NEW.review_id);
        IF review.review_id IS NULL OR reservation.review_id IS NULL
            OR NEW.policy_id<>reservation.policy_id
            OR NEW.policy_version<>reservation.policy_version
            OR NEW.policy_digest<>reservation.policy_digest
            OR NEW.request_sha256<>reservation.request_sha256
            OR (NOT NEW.transport_failed AND (review.response_sealed_at IS NULL
                OR NEW.response_sha256<>review.response_sha256))
            OR (NEW.transport_failed AND (review.state<>'send_unknown' OR review.raw_response IS NOT NULL))
            OR NEW.exhausted_after_response IS DISTINCT FROM
                (NEW.unknown_usage OR COALESCE(NEW.input_tokens>reservation.reserved_input_tokens,false)
                 OR COALESCE(NEW.output_tokens>reservation.reserved_output_tokens,false)
                 OR COALESCE(NEW.elapsed_monotonic_ms>reservation.reserved_elapsed_ms,false))
        THEN
            RAISE EXCEPTION 'anti-bloat consumption seal mismatch' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER scope_anti_bloat_budget_reservation_guard
    BEFORE INSERT OR UPDATE OR DELETE ON scope_anti_bloat_budget_reservations
    FOR EACH ROW EXECUTE FUNCTION scope_anti_bloat_budget_guard();
CREATE TRIGGER scope_anti_bloat_budget_consumption_guard
    BEFORE INSERT OR UPDATE OR DELETE ON scope_anti_bloat_budget_consumptions
    FOR EACH ROW EXECUTE FUNCTION scope_anti_bloat_budget_guard();

ALTER TABLE scope_anti_bloat_budget_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_anti_bloat_budget_reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY scope_anti_bloat_budget_reservation_tenant_scope ON scope_anti_bloat_budget_reservations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
ALTER TABLE scope_anti_bloat_budget_consumptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_anti_bloat_budget_consumptions FORCE ROW LEVEL SECURITY;
CREATE POLICY scope_anti_bloat_budget_consumption_tenant_scope ON scope_anti_bloat_budget_consumptions
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_budget_consumptions'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_budget_reservations,scope_anti_bloat_budget_consumptions FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION scope_anti_bloat_budget_guard() FROM PUBLIC;
