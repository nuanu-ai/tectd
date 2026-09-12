ALTER TABLE hosts
    ADD COLUMN allowed_setup_roots jsonb NOT NULL DEFAULT '[]'::jsonb,
    ADD CONSTRAINT hosts_setup_roots_array_check
        CHECK (pg_catalog.jsonb_typeof(allowed_setup_roots) = 'array');

DROP FUNCTION public.tect_authenticate_host(uuid, text, boolean);

CREATE FUNCTION public.tect_authenticate_host(
    p_host_id uuid,
    p_credential_digest text,
    p_for_write boolean
)
RETURNS TABLE (
    tenant_id uuid,
    principal_id uuid,
    allowed_source_roots jsonb,
    allowed_setup_roots jsonb
)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $function$
BEGIN
    IF p_for_write THEN
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, h.allowed_source_roots, h.allowed_setup_roots
        FROM public.hosts AS h
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked
        FOR SHARE OF h;
    ELSE
        RETURN QUERY
        SELECT h.tenant_id, h.principal_id, h.allowed_source_roots, h.allowed_setup_roots
        FROM public.hosts AS h
        WHERE h.id = p_host_id
          AND h.credential_digest = p_credential_digest
          AND NOT h.revoked;
    END IF;
END
$function$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_authenticate_host(uuid, text, boolean) FROM PUBLIC;

CREATE TABLE setup_session_directories (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    session_id uuid NOT NULL,
    task_directory text NOT NULL,
    device bigint NOT NULL,
    inode bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, host_id, session_id),
    CONSTRAINT setup_session_directories_path_check CHECK (
        pg_catalog.octet_length(task_directory) BETWEEN 1 AND 4096
        AND task_directory ~ '^/([^/]+(/[^/]+)*)?$'
        AND task_directory !~ '(^|/)\.{1,2}(/|$)'
    ),
    CONSTRAINT setup_session_directories_identity_check CHECK (device >= 0 AND inode >= 0),
    CONSTRAINT setup_session_directories_workspace_fk
        FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT setup_session_directories_host_fk
        FOREIGN KEY (tenant_id, host_id)
        REFERENCES hosts (tenant_id, id),
    CONSTRAINT setup_session_directories_session_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, host_id, id)
);

CREATE TABLE workspace_setups (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    task_directory text NOT NULL,
    device bigint NOT NULL,
    inode bigint NOT NULL,
    status text NOT NULL,
    revision bigint NOT NULL,
    content text,
    working_notes text,
    pending_question text,
    current_step text NOT NULL,
    input_cursor bigint NOT NULL,
    latest_input bigint NOT NULL,
    max_input_bytes bigint NOT NULL,
    applied_from_revision bigint,
    applied_sha256 text,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT workspace_setups_id_check CHECK (
        id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT workspace_setups_path_check CHECK (
        pg_catalog.octet_length(task_directory) BETWEEN 1 AND 4096
        AND task_directory ~ '^/([^/]+(/[^/]+)*)?$'
        AND task_directory !~ '(^|/)\.{1,2}(/|$)'
    ),
    CONSTRAINT workspace_setups_identity_check CHECK (device >= 0 AND inode >= 0),
    CONSTRAINT workspace_setups_revision_check CHECK (revision >= 1),
    CONSTRAINT workspace_setups_input_position_check CHECK (
        input_cursor >= 0 AND latest_input >= 1 AND input_cursor <= latest_input
    ),
    CONSTRAINT workspace_setups_max_input_bytes_check CHECK (max_input_bytes >= 0),
    CONSTRAINT workspace_setups_question_check CHECK (
        pending_question IS NULL OR pg_catalog.btrim(pending_question) <> ''
    ),
    CONSTRAINT workspace_setups_state_check CHECK (
        (
            status = 'draft'
            AND current_step IN ('compose', 'waiting_input', 'ready_to_apply')
            AND applied_from_revision IS NULL
            AND applied_sha256 IS NULL
        )
        OR (
            status = 'applied'
            AND current_step = 'complete'
            AND applied_from_revision >= 1
            AND revision = applied_from_revision + 1
            AND applied_sha256 ~ '^[0-9a-f]{64}$'
            AND content IS NOT NULL
            AND pg_catalog.btrim(content) <> ''
        )
    ),
    CONSTRAINT workspace_setups_step_check CHECK (
        current_step <> 'waiting_input' OR pending_question IS NOT NULL
    ),
    CONSTRAINT workspace_setups_ready_check CHECK (
        current_step <> 'ready_to_apply'
        OR (
            content IS NOT NULL
            AND pg_catalog.btrim(content) <> ''
            AND pending_question IS NULL
            AND input_cursor = latest_input
        )
    ),
    CONSTRAINT workspace_setups_workspace_fk
        FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT workspace_setups_host_fk
        FOREIGN KEY (tenant_id, host_id)
        REFERENCES hosts (tenant_id, id),
    CONSTRAINT workspace_setups_scope_id_unique
        UNIQUE (tenant_id, workspace_id, host_id, id),
    CONSTRAINT workspace_setups_directory_unique
        UNIQUE (tenant_id, workspace_id, host_id, task_directory)
);

CREATE TABLE workspace_setup_inputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    host_id uuid NOT NULL,
    setup_id uuid NOT NULL,
    sequence bigint NOT NULL,
    request_id uuid NOT NULL,
    session_id uuid NOT NULL,
    input text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT workspace_setup_inputs_id_check CHECK (
        id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT workspace_setup_inputs_sequence_check CHECK (sequence >= 1),
    CONSTRAINT workspace_setup_inputs_request_id_check CHECK (
        request_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT workspace_setup_inputs_text_check CHECK (pg_catalog.btrim(input) <> ''),
    CONSTRAINT workspace_setup_inputs_setup_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, setup_id)
        REFERENCES workspace_setups (tenant_id, workspace_id, host_id, id),
    CONSTRAINT workspace_setup_inputs_session_fk
        FOREIGN KEY (tenant_id, workspace_id, host_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, host_id, id),
    CONSTRAINT workspace_setup_inputs_sequence_unique
        UNIQUE (tenant_id, setup_id, sequence),
    CONSTRAINT workspace_setup_inputs_request_unique
        UNIQUE (tenant_id, setup_id, request_id)
);

ALTER TABLE setup_session_directories ENABLE ROW LEVEL SECURITY;
ALTER TABLE setup_session_directories FORCE ROW LEVEL SECURITY;
ALTER TABLE workspace_setups ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_setups FORCE ROW LEVEL SECURITY;
ALTER TABLE workspace_setup_inputs ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_setup_inputs FORCE ROW LEVEL SECURITY;

CREATE POLICY setup_session_directories_tenant_scope ON setup_session_directories
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.setup_session_directories'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.setup_session_directories'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY workspace_setups_tenant_scope ON workspace_setups
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.workspace_setups'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.workspace_setups'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY workspace_setup_inputs_tenant_scope ON workspace_setup_inputs
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.workspace_setup_inputs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.workspace_setup_inputs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

REVOKE ALL PRIVILEGES ON TABLE
    setup_session_directories, workspace_setups, workspace_setup_inputs
FROM PUBLIC;
