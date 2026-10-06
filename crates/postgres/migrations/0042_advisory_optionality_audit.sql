-- Slice 00 Packet A: optional Jev configuration and accountable advisory audit.
-- Provider transport, application orchestration, routes and audit projection are
-- delivered by later packets. This migration only establishes the durable spine.

CREATE TABLE advisory_workspace_config (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 0,
    mode text NOT NULL DEFAULT 'disabled',
    provider_profile_ref text,
    model_configuration jsonb,
    updated_by_principal_id uuid NOT NULL,
    updated_by_session_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id),
    CONSTRAINT advisory_workspace_config_revision_check CHECK (revision >= 0),
    CONSTRAINT advisory_workspace_config_mode_check CHECK (mode IN ('disabled', 'optional')),
    CONSTRAINT advisory_workspace_config_provider_check CHECK (
        (provider_profile_ref IS NULL AND model_configuration IS NULL)
        OR (provider_profile_ref IS NOT NULL AND model_configuration IS NOT NULL
            AND pg_catalog.btrim(provider_profile_ref) <> ''
            AND pg_catalog.btrim(provider_profile_ref) = provider_profile_ref
            AND pg_catalog.length(provider_profile_ref) <= 256)
    ),
    CONSTRAINT advisory_workspace_config_model_check CHECK (
        model_configuration IS NULL OR (
            pg_catalog.jsonb_typeof(model_configuration) = 'object'
            AND model_configuration ? 'model'
            AND pg_catalog.jsonb_typeof(model_configuration->'model') = 'string'
            AND pg_catalog.btrim(model_configuration->>'model') <> ''
            AND pg_catalog.btrim(model_configuration->>'model') = model_configuration->>'model'
            AND pg_catalog.length(model_configuration->>'model') <= 256
            AND model_configuration - 'model' = '{}'::jsonb
        )
    ),
    CONSTRAINT advisory_workspace_config_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advisory_workspace_config_principal_fk
        FOREIGN KEY (tenant_id, updated_by_principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT advisory_workspace_config_session_fk
        FOREIGN KEY (tenant_id, workspace_id, updated_by_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id)
);

CREATE TABLE advisory_workspace_config_history (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    revision bigint NOT NULL,
    previous_revision bigint,
    mode text NOT NULL,
    provider_profile_ref text,
    model_configuration jsonb,
    changed_by_principal_id uuid NOT NULL,
    changed_by_session_id uuid NOT NULL,
    changed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, revision),
    CONSTRAINT advisory_workspace_config_history_revision_check CHECK (
        (revision = 0 AND previous_revision IS NULL)
        OR (revision > 0 AND previous_revision = revision - 1)
    ),
    CONSTRAINT advisory_workspace_config_history_mode_check CHECK (mode IN ('disabled', 'optional')),
    CONSTRAINT advisory_workspace_config_history_provider_check CHECK (
        (provider_profile_ref IS NULL AND model_configuration IS NULL)
        OR (provider_profile_ref IS NOT NULL AND model_configuration IS NOT NULL
            AND pg_catalog.btrim(provider_profile_ref) <> ''
            AND pg_catalog.btrim(provider_profile_ref) = provider_profile_ref
            AND pg_catalog.length(provider_profile_ref) <= 256)
    ),
    CONSTRAINT advisory_workspace_config_history_model_check CHECK (
        model_configuration IS NULL OR (
            pg_catalog.jsonb_typeof(model_configuration) = 'object'
            AND model_configuration ? 'model'
            AND pg_catalog.jsonb_typeof(model_configuration->'model') = 'string'
            AND pg_catalog.btrim(model_configuration->>'model') <> ''
            AND pg_catalog.btrim(model_configuration->>'model') = model_configuration->>'model'
            AND pg_catalog.length(model_configuration->>'model') <= 256
            AND model_configuration - 'model' = '{}'::jsonb
        )
    ),
    CONSTRAINT advisory_workspace_config_history_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advisory_workspace_config_history_principal_fk
        FOREIGN KEY (tenant_id, changed_by_principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT advisory_workspace_config_history_session_fk
        FOREIGN KEY (tenant_id, workspace_id, changed_by_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT advisory_workspace_config_history_predecessor_fk
        FOREIGN KEY (tenant_id, workspace_id, previous_revision)
        REFERENCES advisory_workspace_config_history (tenant_id, workspace_id, revision)
);

ALTER TABLE advisory_workspace_config
    ADD CONSTRAINT advisory_workspace_config_history_fk
    FOREIGN KEY (tenant_id, workspace_id, revision)
    REFERENCES advisory_workspace_config_history (tenant_id, workspace_id, revision);

CREATE TABLE advisory_opportunity (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    scope_id uuid,
    work_item_kind text NOT NULL,
    work_item_id uuid,
    session_id uuid NOT NULL,
    authorized_actor_id uuid NOT NULL,
    source_revision text,
    run_id uuid,
    phase text,
    step text,
    capability text NOT NULL,
    decision_point text NOT NULL,
    config_revision bigint NOT NULL,
    session_preference text NOT NULL,
    request_preference text NOT NULL,
    policy_version text NOT NULL,
    request_key text NOT NULL,
    material_digest text NOT NULL,
    deterministic_baseline_ref text,
    eligible_material_ref text,
    state text NOT NULL,
    primary_reason text NOT NULL,
    parent_opportunity_id uuid,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT advisory_opportunity_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advisory_opportunity_session_fk FOREIGN KEY (tenant_id, workspace_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT advisory_opportunity_actor_fk FOREIGN KEY (tenant_id, authorized_actor_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT advisory_opportunity_scope_fk FOREIGN KEY (tenant_id, workspace_id, scope_id)
        REFERENCES native_scopes (tenant_id, workspace_id, id),
    CONSTRAINT advisory_opportunity_capability_check CHECK (capability IN (
        'scope_decomposition', 'engineering_profile', 'pipeline_recommendation',
        'anti_bloat', 'model_routing'
    )),
    CONSTRAINT advisory_opportunity_context_check CHECK (
        pg_catalog.btrim(work_item_kind) <> ''
        AND (source_revision IS NULL OR pg_catalog.btrim(source_revision) <> '')
        AND (phase IS NULL OR pg_catalog.btrim(phase) <> '')
        AND (step IS NULL OR pg_catalog.btrim(step) <> '')
    ),
    CONSTRAINT advisory_opportunity_decision_check CHECK (
        decision_point = 'scope.decomposition.before_selection'
        AND pg_catalog.btrim(policy_version) <> ''
        AND pg_catalog.btrim(request_key) <> ''
        AND config_revision >= 0
    ),
    CONSTRAINT advisory_opportunity_digest_check CHECK (material_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_opportunity_decision_capability_check CHECK (
        capability = 'scope_decomposition'
        AND decision_point = 'scope.decomposition.before_selection'
    ),
    CONSTRAINT advisory_opportunity_preference_check CHECK (
        session_preference IN ('use_workspace', 'skip')
        AND request_preference IN ('use_workspace', 'skip')
    ),
    CONSTRAINT advisory_opportunity_state_check CHECK (state IN (
        'prepared', 'no_call', 'awaiting_response', 'advised',
        'invalidated', 'failed', 'unresolved'
    )),
    CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled', 'session_skip', 'request_skip',
        'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
        'configuration_changed', 'dispatch_authorized', 'provider_response',
        'provider_failure', 'send_unknown'
    )),
    CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state = 'no_call' AND primary_reason IN (
            'workspace_disabled', 'session_skip', 'request_skip',
            'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured'
        ))
        OR (state = 'prepared' AND primary_reason = 'dispatch_authorized')
        OR (state = 'awaiting_response' AND primary_reason IN ('dispatch_authorized', 'send_unknown'))
        OR (state = 'advised' AND primary_reason = 'provider_response')
        OR (state = 'invalidated' AND primary_reason = 'configuration_changed')
        OR (state = 'failed' AND primary_reason = 'provider_failure')
        OR (state = 'unresolved' AND primary_reason = 'send_unknown')
    ),
    CONSTRAINT advisory_opportunity_tenant_id_unique UNIQUE (tenant_id, workspace_id, id),
    CONSTRAINT advisory_opportunity_request_unique UNIQUE (tenant_id, workspace_id, request_key),
    CONSTRAINT advisory_opportunity_parent_fk
        FOREIGN KEY (tenant_id, workspace_id, parent_opportunity_id)
        REFERENCES advisory_opportunity (tenant_id, workspace_id, id)
);

CREATE TABLE advisory_dispatch (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    attempt_number integer NOT NULL,
    predecessor_dispatch_id uuid,
    provider text NOT NULL,
    model text NOT NULL,
    configuration_snapshot jsonb NOT NULL,
    configuration_digest text NOT NULL,
    material_digest text NOT NULL,
    payload_digest text NOT NULL,
    request_payload bytea NOT NULL,
    response_payload bytea,
    input_tokens bigint,
    output_tokens bigint,
    latency_ms bigint,
    state text NOT NULL,
    send_certainty text NOT NULL,
    outcome text,
    retry_basis text NOT NULL,
    raw_response_ref text,
    authorized_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    send_started_at timestamptz,
    sealed_at timestamptz,
    CONSTRAINT advisory_dispatch_opportunity_fk
        FOREIGN KEY (tenant_id, workspace_id, opportunity_id)
        REFERENCES advisory_opportunity (tenant_id, workspace_id, id),
    CONSTRAINT advisory_dispatch_provider_check CHECK (
        pg_catalog.btrim(provider) <> '' AND pg_catalog.btrim(model) <> ''
        AND pg_catalog.jsonb_typeof(configuration_snapshot) = 'object'
    ),
    CONSTRAINT advisory_dispatch_digest_check CHECK (
        configuration_digest ~ '^[0-9a-f]{64}$'
        AND material_digest ~ '^[0-9a-f]{64}$'
        AND payload_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT advisory_dispatch_measurement_check CHECK (
        (input_tokens IS NULL OR input_tokens >= 0)
        AND (output_tokens IS NULL OR output_tokens >= 0)
        AND (latency_ms IS NULL OR latency_ms >= 0)
    ),
    CONSTRAINT advisory_dispatch_state_check CHECK (
        state IN ('authorized', 'sending', 'sealed', 'cancelled')
    ),
    CONSTRAINT advisory_dispatch_certainty_check CHECK (
        send_certainty IN ('not_sent', 'sent', 'sent_unknown')
    ),
    CONSTRAINT advisory_dispatch_outcome_check CHECK (
        outcome IS NULL OR outcome IN ('provider_response', 'provider_failure')
    ),
    CONSTRAINT advisory_dispatch_retry_basis_check CHECK (retry_basis IN (
        'initial', 'proven_not_sent', 'known_retryable_response',
        'verified_provider_idempotency'
    )),
    CONSTRAINT advisory_dispatch_attempt_check CHECK (
        (attempt_number = 1 AND predecessor_dispatch_id IS NULL AND retry_basis = 'initial')
        OR (attempt_number > 1 AND predecessor_dispatch_id IS NOT NULL
            AND retry_basis = 'proven_not_sent')
    ),
    CONSTRAINT advisory_dispatch_lifecycle_check CHECK (
        (state = 'authorized' AND send_certainty = 'not_sent'
            AND send_started_at IS NULL AND sealed_at IS NULL AND outcome IS NULL)
        OR (state = 'sending' AND send_certainty = 'sent_unknown'
            AND send_started_at IS NOT NULL AND sealed_at IS NULL AND outcome IS NULL)
        OR (state = 'sealed' AND send_started_at IS NOT NULL
            AND sealed_at IS NOT NULL AND outcome IS NOT NULL)
        OR (state = 'cancelled' AND send_certainty = 'not_sent'
            AND send_started_at IS NULL AND sealed_at IS NOT NULL AND outcome IS NULL)
    ),
    CONSTRAINT advisory_dispatch_response_certainty_check CHECK (
        outcome <> 'provider_response' OR send_certainty = 'sent'
    ),
    CONSTRAINT advisory_dispatch_lineage_unique
        UNIQUE (tenant_id, workspace_id, opportunity_id, id),
    CONSTRAINT advisory_dispatch_attempt_unique
        UNIQUE (tenant_id, workspace_id, opportunity_id, attempt_number),
    CONSTRAINT advisory_dispatch_predecessor_fk
        FOREIGN KEY (tenant_id, workspace_id, opportunity_id, predecessor_dispatch_id)
        REFERENCES advisory_dispatch (tenant_id, workspace_id, opportunity_id, id)
);

CREATE INDEX advisory_opportunity_workspace_audit_idx
    ON advisory_opportunity (tenant_id, workspace_id, created_at DESC, id DESC);
CREATE INDEX advisory_opportunity_scope_audit_idx
    ON advisory_opportunity (tenant_id, workspace_id, scope_id, created_at DESC, id DESC)
    WHERE scope_id IS NOT NULL;
CREATE INDEX advisory_dispatch_unresolved_idx
    ON advisory_dispatch (tenant_id, workspace_id, authorized_at, id)
    WHERE state = 'sending' AND send_certainty = 'sent_unknown';
CREATE UNIQUE INDEX advisory_dispatch_retry_child_unique
    ON advisory_dispatch (tenant_id, workspace_id, opportunity_id, predecessor_dispatch_id)
    WHERE predecessor_dispatch_id IS NOT NULL;

ALTER TABLE advisory_workspace_config ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_workspace_config FORCE ROW LEVEL SECURITY;
ALTER TABLE advisory_workspace_config_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_workspace_config_history FORCE ROW LEVEL SECURITY;
ALTER TABLE advisory_opportunity ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_opportunity FORCE ROW LEVEL SECURITY;
ALTER TABLE advisory_dispatch ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_dispatch FORCE ROW LEVEL SECURITY;

CREATE POLICY advisory_workspace_config_tenant_scope ON advisory_workspace_config
    USING (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_workspace_config'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_workspace_config'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY advisory_workspace_config_history_tenant_scope ON advisory_workspace_config_history
    USING (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_workspace_config_history'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_workspace_config_history'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY advisory_opportunity_tenant_scope ON advisory_opportunity
    USING (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_opportunity'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_opportunity'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY advisory_dispatch_tenant_scope ON advisory_dispatch
    USING (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_dispatch'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER = pg_catalog.pg_get_userbyid(
        (SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_dispatch'::regclass)
    ) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

CREATE TRIGGER advisory_workspace_config_created_at_immutable
    BEFORE UPDATE ON advisory_workspace_config
    FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER advisory_opportunity_created_at_immutable
    BEFORE UPDATE ON advisory_opportunity
    FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE advisory_workspace_config,
    advisory_workspace_config_history, advisory_opportunity, advisory_dispatch FROM PUBLIC;
