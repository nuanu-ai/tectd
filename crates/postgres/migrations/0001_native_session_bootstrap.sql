CREATE TABLE tenants (
    id uuid PRIMARY KEY,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp()
);

CREATE TABLE principals (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    role text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT principals_role_check CHECK (role = 'owner'),
    CONSTRAINT principals_tenant_fk FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT principals_tenant_id_unique UNIQUE (tenant_id, id)
);
CREATE UNIQUE INDEX principals_one_owner_per_tenant
    ON principals (tenant_id) WHERE role = 'owner';

CREATE TABLE hosts (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    credential_digest text NOT NULL UNIQUE,
    revoked boolean NOT NULL DEFAULT false,
    allowed_source_roots jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT hosts_digest_check CHECK (credential_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT hosts_roots_array_check CHECK (pg_catalog.jsonb_typeof(allowed_source_roots) = 'array'),
    CONSTRAINT hosts_principal_fk FOREIGN KEY (tenant_id, principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT hosts_tenant_id_unique UNIQUE (tenant_id, id)
);

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    key text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT workspaces_key_check CHECK (
        pg_catalog.length(key) BETWEEN 1 AND 128
        AND key ~ '^[A-Za-z0-9][A-Za-z0-9._-]*$'
    ),
    CONSTRAINT workspaces_tenant_fk FOREIGN KEY (tenant_id) REFERENCES tenants (id),
    CONSTRAINT workspaces_tenant_id_unique UNIQUE (tenant_id, id),
    CONSTRAINT workspaces_tenant_key_unique UNIQUE (tenant_id, key)
);

CREATE TABLE memberships (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, principal_id),
    CONSTRAINT memberships_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT memberships_principal_fk FOREIGN KEY (tenant_id, principal_id)
        REFERENCES principals (tenant_id, id)
);

CREATE TABLE agent_sessions (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    host_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    native_session_id text NOT NULL,
    revoked boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT agent_sessions_native_id_check CHECK (
        native_session_id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        AND native_session_id <> '00000000-0000-0000-0000-000000000000'
    ),
    CONSTRAINT agent_sessions_host_fk FOREIGN KEY (tenant_id, host_id)
        REFERENCES hosts (tenant_id, id),
    CONSTRAINT agent_sessions_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT agent_sessions_tenant_id_unique UNIQUE (tenant_id, id),
    CONSTRAINT agent_sessions_native_unique UNIQUE (host_id, native_session_id)
);

CREATE TABLE workspace_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    kind text NOT NULL,
    entity_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT workspace_events_kind_check CHECK (kind IN ('workspace_opened', 'session_opened')),
    CONSTRAINT workspace_events_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT workspace_events_creation_unique UNIQUE (kind, entity_id)
);

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
ALTER TABLE principals ENABLE ROW LEVEL SECURITY;
ALTER TABLE principals FORCE ROW LEVEL SECURITY;
ALTER TABLE hosts ENABLE ROW LEVEL SECURITY;
ALTER TABLE hosts FORCE ROW LEVEL SECURITY;
ALTER TABLE workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspaces FORCE ROW LEVEL SECURITY;
ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;
ALTER TABLE agent_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_sessions FORCE ROW LEVEL SECURITY;
ALTER TABLE workspace_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_events FORCE ROW LEVEL SECURITY;

CREATE POLICY tenants_tenant_scope ON tenants
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.tenants'::regclass)
        )
        OR id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.tenants'::regclass)
        )
        OR id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY principals_tenant_scope ON principals
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.principals'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.principals'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY hosts_tenant_scope ON hosts
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.hosts'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.hosts'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY workspaces_tenant_scope ON workspaces
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.workspaces'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.workspaces'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY memberships_tenant_scope ON memberships
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.memberships'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.memberships'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY agent_sessions_tenant_scope ON agent_sessions
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.agent_sessions'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.agent_sessions'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY workspace_events_tenant_scope ON workspace_events
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.workspace_events'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.workspace_events'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE FUNCTION public.tect_authenticate_host(
    p_host_id uuid,
    p_credential_digest text,
    p_for_write boolean
)
RETURNS TABLE (tenant_id uuid, principal_id uuid, allowed_source_roots jsonb)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
    IF p_for_write THEN
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, h.allowed_source_roots
        FROM public.hosts AS h
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked
        FOR SHARE OF h;
    ELSE
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, h.allowed_source_roots
        FROM public.hosts AS h
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked;
    END IF;
END
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_authenticate_host(uuid, text, boolean) FROM PUBLIC;
REVOKE ALL PRIVILEGES ON TABLE tenants, principals, hosts FROM PUBLIC;
REVOKE ALL PRIVILEGES ON TABLE workspaces, memberships, agent_sessions, workspace_events FROM PUBLIC;
