-- Jev's advice binds. advice_candidate_answers holds what Jev answered about each
-- candidate a draft save created or changed: one row per candidate, revision and
-- advice point, written when the journal row of that call is completed as advised.
-- A ready review is held to the rows that ask for a change, for the candidates that
-- still stand in the draft under the same revision, until a review states a departure:
-- its reason and time are then written on the row. Existing tables are untouched.
CREATE TABLE advice_candidate_answers (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    -- The journal row of the call that gave this answer.
    journal_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    point text NOT NULL,
    -- The candidate set of the saved draft and the planning level it belongs to.
    candidate_set_id uuid NOT NULL,
    level text NOT NULL,
    -- The candidate under the revision the answer is about.
    candidate_id uuid NOT NULL,
    candidate_revision bigint NOT NULL,
    asks_change boolean NOT NULL,
    -- What Jev chose and its probabilities; no text of the draft.
    answer jsonb NOT NULL,
    departure_reason text,
    departed_at timestamptz,
    CONSTRAINT advice_candidate_answers_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT advice_candidate_answers_journal_fk FOREIGN KEY (journal_id)
        REFERENCES advice_journal (id),
    CONSTRAINT advice_candidate_answers_point_check CHECK (point IN ('sizing', 'pipeline')),
    CONSTRAINT advice_candidate_answers_level_check CHECK (
        level IN ('program_to_scopes', 'scope_to_slices')
    ),
    CONSTRAINT advice_candidate_answers_revision_check CHECK (candidate_revision >= 1),
    CONSTRAINT advice_candidate_answers_answer_check CHECK (
        pg_catalog.jsonb_typeof(answer) = 'object'
    ),
    -- A departure is a reason and its time together, and only advice that asks for
    -- a change can be departed from.
    CONSTRAINT advice_candidate_answers_departure_check CHECK (
        (departure_reason IS NULL) = (departed_at IS NULL)
        AND (
            departure_reason IS NULL
            OR (asks_change AND pg_catalog.btrim(departure_reason) <> '')
        )
    ),
    -- Jev is asked about a candidate under one revision once per point.
    CONSTRAINT advice_candidate_answers_candidate_key UNIQUE (
        tenant_id, workspace_id, candidate_set_id, candidate_id, candidate_revision, point
    )
);

ALTER TABLE advice_candidate_answers ENABLE ROW LEVEL SECURITY;
ALTER TABLE advice_candidate_answers FORCE ROW LEVEL SECURITY;

CREATE POLICY advice_candidate_answers_tenant_scope ON advice_candidate_answers
    USING (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.advice_candidate_answers'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        CURRENT_USER = pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='public.advice_candidate_answers'::regclass)
        )
        OR tenant_id = NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid
    );

CREATE TRIGGER advice_candidate_answers_created_at_immutable
    BEFORE UPDATE ON advice_candidate_answers
    FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE advice_candidate_answers FROM PUBLIC;
