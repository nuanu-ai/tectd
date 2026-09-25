-- REQ-BUDGET-001: policy installation is explicit, tenant-scoped and immutable.
-- No row is seeded, so a missing policy cannot authorize provider dispatch.
CREATE TABLE advisory_budget_policies (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL,
    version bigint NOT NULL CHECK (version > 0),
    digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
    effective_from_unix_ms bigint NOT NULL CHECK (effective_from_unix_ms >= 0),
    effective_until_unix_ms bigint NOT NULL,
    provider_calls bigint NOT NULL CHECK (provider_calls > 0),
    input_tokens bigint NOT NULL CHECK (input_tokens > 0),
    output_tokens bigint NOT NULL CHECK (output_tokens > 0),
    request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0),
    elapsed_monotonic_ms bigint NOT NULL CHECK (elapsed_monotonic_ms > 0),
    retry_dispatches bigint NOT NULL CHECK (retry_dispatches > 0),
    approved_by uuid NOT NULL,
    approval_signature text NOT NULL CHECK (approval_signature ~ '^[0-9a-fA-F]{128}$'),
    installed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, id),
    UNIQUE (tenant_id, workspace_id, version),
    UNIQUE (tenant_id, workspace_id, digest),
    FOREIGN KEY (tenant_id, workspace_id) REFERENCES workspaces (tenant_id, id),
    FOREIGN KEY (tenant_id, approved_by) REFERENCES principals (tenant_id, id),
    CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    CHECK (approved_by <> '00000000-0000-0000-0000-000000000000'::uuid),
    CHECK (effective_until_unix_ms > effective_from_unix_ms)
);

CREATE FUNCTION advisory_budget_policy_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'advisory budget policy is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'budget policy tenant mismatch' USING ERRCODE='42501';
    END IF;
    IF NEW.digest IS DISTINCT FROM pg_catalog.encode(pg_catalog.sha256(
        pg_catalog.convert_to(pg_catalog.format(
            'jev-budget-policy/v1%s%s%s%s%s%s%s%s%s%s',
            E'\n' || NEW.id::text || E'\n',
            NEW.version::text || E'\n',
            NEW.effective_from_unix_ms::text || E'\n',
            NEW.effective_until_unix_ms::text || E'\n',
            NEW.provider_calls::text || E'\n',
            NEW.input_tokens::text || E'\n',
            NEW.output_tokens::text || E'\n',
            NEW.request_utf8_bytes::text || E'\n',
            NEW.elapsed_monotonic_ms::text || E'\n',
            NEW.retry_dispatches::text || E'\n'
        ), 'UTF8')), 'hex') THEN
        RAISE EXCEPTION 'budget policy digest mismatch' USING ERRCODE='23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.principals p
        JOIN public.memberships m ON (m.tenant_id,m.principal_id)=(p.tenant_id,p.id)
        WHERE p.tenant_id=NEW.tenant_id AND p.id=NEW.approved_by
          AND p.role='owner' AND m.workspace_id=NEW.workspace_id
    ) THEN
        RAISE EXCEPTION 'budget policy requires workspace owner approval'
            USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER advisory_budget_policy_guard_trigger
    BEFORE INSERT OR UPDATE OR DELETE ON advisory_budget_policies
    FOR EACH ROW EXECUTE FUNCTION advisory_budget_policy_guard();

ALTER TABLE advisory_budget_policies ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_budget_policies FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_budget_policy_tenant_scope ON advisory_budget_policies
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_policies'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_budget_policies'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_budget_policies FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_budget_policy_guard() FROM PUBLIC;
