-- Jev advice inside commands. advice_config holds one setting row per workspace;
-- no row means Jev is disabled. advice_journal holds one row per advice point that
-- fired while Jev was enabled. Existing tables are untouched.
CREATE TABLE advice_config (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    enabled boolean NOT NULL,
    daily_call_limit bigint,
    revision bigint NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    updated_by_session uuid NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id),
    CONSTRAINT advice_config_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advice_config_session_fk
        FOREIGN KEY (tenant_id, workspace_id, updated_by_session)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT advice_config_revision_check CHECK (revision >= 1),
    CONSTRAINT advice_config_limit_check CHECK (
        daily_call_limit IS NULL OR daily_call_limit > 0
    ),
    -- Enabling has no default ceiling: the caller states it.
    CONSTRAINT advice_config_enabled_limit_check CHECK (
        NOT enabled OR daily_call_limit IS NOT NULL
    )
);

CREATE TABLE advice_journal (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    session_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    point text NOT NULL,
    route text NOT NULL,
    -- Idempotency request id of the agent's command in which the point fired.
    request_id uuid,
    program_id uuid,
    candidate_set_id uuid,
    scope_id uuid,
    slice_id uuid,
    phase text,
    called boolean NOT NULL,
    outcome text,
    reason text,
    send_certainty text NOT NULL,
    -- Unknown values stay NULL; zero is never a stand-in.
    model text,
    request_bytes bigint,
    response_bytes bigint,
    input_tokens bigint,
    output_tokens bigint,
    latency_ms bigint,
    request_body bytea,
    response_body bytea,
    -- Exactly the advice field the agent received for this row.
    advice jsonb,
    CONSTRAINT advice_journal_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advice_journal_session_fk FOREIGN KEY (tenant_id, workspace_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT advice_journal_point_check CHECK (point ~ '^[a-z0-9_]{1,64}$'),
    CONSTRAINT advice_journal_route_check CHECK (route ~ '^[a-z0-9_.]{1,128}$'),
    CONSTRAINT advice_journal_request_id_check CHECK (
        request_id IS NULL OR request_id <> '00000000-0000-0000-0000-000000000000'::uuid
    ),
    CONSTRAINT advice_journal_phase_check CHECK (
        phase IS NULL OR (pg_catalog.btrim(phase) <> '' AND pg_catalog.octet_length(phase) <= 256)
    ),
    CONSTRAINT advice_journal_outcome_check CHECK (
        outcome IS NULL OR outcome IN ('advised', 'abstained', 'unavailable')
    ),
    CONSTRAINT advice_journal_reason_check CHECK (
        reason IS NULL OR reason IN (
            'provider_unconfigured', 'daily_limit_reached', 'timeout', 'transport_error',
            'http_status', 'invalid_response', 'request_too_large', 'advice_too_large',
            'mode_not_set', 'program_unavailable'
        )
    ),
    CONSTRAINT advice_journal_send_certainty_check CHECK (
        send_certainty IN ('not_sent', 'sent', 'sent_unknown')
    ),
    CONSTRAINT advice_journal_called_check CHECK (called = (send_certainty <> 'not_sent')),
    -- A reason belongs to unavailable and to nothing else.
    CONSTRAINT advice_journal_reason_outcome_check CHECK (
        (outcome IS NOT DISTINCT FROM 'unavailable') = (reason IS NOT NULL)
    ),
    -- Only a request that left and has no recorded answer has an empty outcome.
    CONSTRAINT advice_journal_pending_check CHECK (
        outcome IS NOT NULL OR send_certainty = 'sent_unknown'
    ),
    -- An answer exists only for a request that was sent and answered.
    CONSTRAINT advice_journal_answer_check CHECK (
        outcome IS NULL OR outcome = 'unavailable' OR send_certainty = 'sent'
    ),
    CONSTRAINT advice_journal_advice_check CHECK (
        (advice IS NULL) = (outcome IS NULL)
        AND (advice IS NULL OR pg_catalog.jsonb_typeof(advice) = 'object')
    ),
    CONSTRAINT advice_journal_measure_check CHECK (
        (request_bytes IS NULL OR request_bytes >= 0)
        AND (response_bytes IS NULL OR response_bytes >= 0)
        AND (input_tokens IS NULL OR input_tokens >= 0)
        AND (output_tokens IS NULL OR output_tokens >= 0)
        AND (latency_ms IS NULL OR latency_ms >= 0)
    )
);

CREATE INDEX advice_journal_workspace_time_idx
    ON advice_journal (tenant_id, workspace_id, created_at DESC, id DESC);
CREATE INDEX advice_journal_request_idx
    ON advice_journal (tenant_id, workspace_id, request_id)
    WHERE request_id IS NOT NULL;

ALTER TABLE advice_config ENABLE ROW LEVEL SECURITY;
ALTER TABLE advice_config FORCE ROW LEVEL SECURITY;
ALTER TABLE advice_journal ENABLE ROW LEVEL SECURITY;
ALTER TABLE advice_journal FORCE ROW LEVEL SECURITY;

CREATE POLICY advice_config_tenant_scope ON advice_config
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.advice_config'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.advice_config'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE POLICY advice_journal_tenant_scope ON advice_journal
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.advice_journal'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class WHERE oid='public.advice_journal'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE TRIGGER advice_journal_created_at_immutable
    BEFORE UPDATE ON advice_journal
    FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE advice_config, advice_journal FROM PUBLIC;
