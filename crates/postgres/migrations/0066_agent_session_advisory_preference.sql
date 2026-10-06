-- The native session owns its advisory preference; existing sessions inherit
-- the same durable default as newly opened sessions.
ALTER TABLE public.agent_sessions
    ADD COLUMN advisory_preference text NOT NULL DEFAULT 'use_workspace',
    ADD COLUMN advisory_preference_revision bigint NOT NULL DEFAULT 0,
    ADD CONSTRAINT agent_sessions_advisory_preference_check
        CHECK (advisory_preference IN ('use_workspace', 'skip')),
    ADD CONSTRAINT agent_sessions_advisory_preference_revision_check
        CHECK (advisory_preference_revision >= 0);
