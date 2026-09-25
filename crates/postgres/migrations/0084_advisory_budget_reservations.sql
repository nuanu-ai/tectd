-- REQ-BUDGET-001 pre-send reservations. An absent row never grants send.
CREATE TABLE advisory_budget_reservations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    policy_version bigint NOT NULL CHECK (policy_version > 0),
    policy_digest text NOT NULL CHECK (policy_digest ~ '^[0-9a-f]{64}$'),
    policy_effective_from_unix_ms bigint NOT NULL,
    policy_effective_until_unix_ms bigint NOT NULL,
    request_sha256 text NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    reserved_calls bigint NOT NULL DEFAULT 1 CHECK (reserved_calls = 1),
    reserved_retry_dispatches bigint NOT NULL CHECK (reserved_retry_dispatches IN (0,1)),
    remaining_elapsed_ms bigint NOT NULL CHECK (remaining_elapsed_ms > 0),
    reserved_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    FOREIGN KEY (tenant_id,workspace_id,policy_id)
        REFERENCES advisory_budget_policies (tenant_id,workspace_id,id)
);
CREATE INDEX advisory_budget_reservations_opportunity_idx
    ON advisory_budget_reservations (tenant_id,workspace_id,opportunity_id);

CREATE FUNCTION advisory_budget_reservation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE dispatch public.advisory_dispatch%ROWTYPE;
DECLARE policy public.advisory_budget_policies%ROWTYPE;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'budget reservation is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'budget reservation tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT * INTO dispatch FROM public.advisory_dispatch
      WHERE (tenant_id,workspace_id,opportunity_id,id)=
            (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id);
    SELECT * INTO policy FROM public.advisory_budget_policies
      WHERE (tenant_id,workspace_id,id)=
            (NEW.tenant_id,NEW.workspace_id,NEW.policy_id);
    IF dispatch.id IS NULL OR policy.id IS NULL
       OR dispatch.state <> 'authorized'
       OR NEW.request_utf8_bytes <> pg_catalog.octet_length(dispatch.request_payload)
       OR NEW.request_sha256 <> dispatch.payload_digest
       OR NEW.request_sha256 <> pg_catalog.encode(pg_catalog.sha256(dispatch.request_payload),'hex')
       OR NEW.policy_version <> policy.version OR NEW.policy_digest <> policy.digest
       OR NEW.policy_effective_from_unix_ms <> policy.effective_from_unix_ms
       OR NEW.policy_effective_until_unix_ms <> policy.effective_until_unix_ms
       OR (dispatch.attempt_number=1 AND NEW.reserved_retry_dispatches <> 0)
       OR (dispatch.attempt_number<>1 AND NEW.reserved_retry_dispatches <> 1)
       OR NEW.remaining_elapsed_ms > policy.elapsed_monotonic_ms THEN
        RAISE EXCEPTION 'budget reservation binding mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END; $guard$;
CREATE TRIGGER advisory_budget_reservation_guard_trigger
    BEFORE INSERT OR UPDATE OR DELETE ON advisory_budget_reservations
    FOR EACH ROW EXECUTE FUNCTION advisory_budget_reservation_guard();

ALTER TABLE advisory_budget_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_budget_reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_budget_reservation_tenant_scope ON advisory_budget_reservations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_reservations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_budget_reservations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_budget_reservation_guard() FROM PUBLIC;
