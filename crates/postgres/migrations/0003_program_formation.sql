ALTER TABLE agent_sessions
    ADD CONSTRAINT agent_sessions_workspace_id_unique
    UNIQUE (tenant_id, workspace_id, id);

CREATE TABLE programs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    status text NOT NULL,
    revision bigint NOT NULL,
    name text,
    intent text,
    basis text,
    boundaries text,
    constraints text,
    success text,
    working_notes text,
    pending_question text,
    current_step text NOT NULL,
    input_cursor bigint NOT NULL,
    latest_input bigint NOT NULL,
    max_input_bytes bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT programs_status_check CHECK (status IN ('draft', 'open')),
    CONSTRAINT programs_revision_check CHECK (revision >= 1),
    CONSTRAINT programs_step_check CHECK (
        current_step IN ('compose', 'waiting_input', 'ready')
    ),
    CONSTRAINT programs_input_position_check CHECK (
        input_cursor >= 0 AND latest_input >= 1 AND input_cursor <= latest_input
    ),
    CONSTRAINT programs_max_input_bytes_check CHECK (max_input_bytes >= 0),
    CONSTRAINT programs_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT programs_scope_id_unique UNIQUE (tenant_id, workspace_id, id)
);

CREATE TABLE program_inputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    program_id uuid NOT NULL,
    sequence bigint NOT NULL,
    request_id uuid NOT NULL,
    session_id uuid NOT NULL,
    input text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT program_inputs_id_check CHECK (
        id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT program_inputs_sequence_check CHECK (sequence >= 1),
    CONSTRAINT program_inputs_request_id_check CHECK (
        request_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT program_inputs_text_check CHECK (pg_catalog.btrim(input) <> ''),
    CONSTRAINT program_inputs_program_fk
        FOREIGN KEY (tenant_id, workspace_id, program_id)
        REFERENCES programs (tenant_id, workspace_id, id),
    CONSTRAINT program_inputs_session_fk
        FOREIGN KEY (tenant_id, workspace_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT program_inputs_sequence_unique
        UNIQUE (tenant_id, workspace_id, program_id, sequence),
    CONSTRAINT program_inputs_request_unique
        UNIQUE (tenant_id, workspace_id, program_id, request_id)
);

CREATE INDEX programs_workspace_order_idx
    ON programs (tenant_id, workspace_id, (current_step = 'ready'), id);
CREATE UNIQUE INDEX program_inputs_workspace_origin_request_unique
    ON program_inputs (tenant_id, workspace_id, request_id)
    WHERE sequence = 1;

ALTER TABLE programs ENABLE ROW LEVEL SECURITY;
ALTER TABLE programs FORCE ROW LEVEL SECURITY;
ALTER TABLE program_inputs ENABLE ROW LEVEL SECURITY;
ALTER TABLE program_inputs FORCE ROW LEVEL SECURITY;

CREATE POLICY programs_tenant_scope ON programs
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.programs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.programs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY program_inputs_tenant_scope ON program_inputs
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.program_inputs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.program_inputs'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

REVOKE ALL PRIVILEGES ON TABLE programs, program_inputs FROM PUBLIC;
