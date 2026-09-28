-- The native session owns its advisory preference; existing sessions inherit
-- the same durable default as newly opened sessions.
ALTER TABLE public.agent_sessions
    ADD COLUMN advisory_preference text NOT NULL DEFAULT 'use_workspace',
    ADD COLUMN advisory_preference_revision bigint NOT NULL DEFAULT 0,
    ADD CONSTRAINT agent_sessions_advisory_preference_check
        CHECK (advisory_preference IN ('use_workspace', 'skip')),
    ADD CONSTRAINT agent_sessions_advisory_preference_revision_check
        CHECK (advisory_preference_revision >= 0);

CREATE TABLE public.session_advisory_preference_history (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    session_id uuid NOT NULL,
    revision bigint NOT NULL,
    previous_revision bigint,
    preference text NOT NULL,
    changed_by_principal_id uuid NOT NULL,
    changed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, session_id, revision),
    CONSTRAINT session_advisory_preference_history_revision_check CHECK (
        (revision = 0 AND previous_revision IS NULL)
        OR (revision > 0 AND previous_revision = revision - 1)
    ),
    CONSTRAINT session_advisory_preference_history_value_check CHECK (
        preference IN ('use_workspace', 'skip')
    ),
    CONSTRAINT session_advisory_preference_history_session_fk FOREIGN KEY
        (tenant_id, workspace_id, session_id)
        REFERENCES public.agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT session_advisory_preference_history_principal_fk FOREIGN KEY
        (tenant_id, changed_by_principal_id)
        REFERENCES public.principals (tenant_id, id),
    CONSTRAINT session_advisory_preference_history_predecessor_fk FOREIGN KEY
        (tenant_id, workspace_id, session_id, previous_revision)
        REFERENCES public.session_advisory_preference_history
            (tenant_id, workspace_id, session_id, revision)
);

ALTER TABLE public.session_advisory_preference_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.session_advisory_preference_history FORCE ROW LEVEL SECURITY;
CREATE POLICY session_advisory_preference_history_tenant_scope
    ON public.session_advisory_preference_history
    USING (tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid);
REVOKE ALL PRIVILEGES ON public.session_advisory_preference_history FROM PUBLIC;
