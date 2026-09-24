-- Guarded Matrix advice is evidence about owner-authored choices. It cannot
-- select a choice. The agent's later disposition is a separate append-only fact.
-- The sentinel is outside the 64-hex digest domain, so this key makes a NULL
-- (no-choice) binding compare exactly in composite foreign keys.
ALTER TABLE advisory_opportunity
    ADD COLUMN matrix_choice_binding_key text GENERATED ALWAYS AS
        (COALESCE(matrix_choice_set_digest, 'no-choice')) STORED,
    ADD CONSTRAINT advisory_opportunity_matrix_exact_binding_unique UNIQUE
        (tenant_id, workspace_id, id, work_item_id, matrix_task_revision,
         matrix_choice_binding_key);

CREATE TABLE advisory_matrix_advice (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    task_id uuid NOT NULL,
    matrix_task_revision bigint NOT NULL,
    matrix_choice_set_digest text NOT NULL,
    advice_id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    dispatch_id uuid NOT NULL,
    kind text NOT NULL,
    ranked_choice_ids jsonb,
    reason text,
    advice_digest text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, advice_id),
    CONSTRAINT advisory_matrix_advice_opportunity_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id),
    CONSTRAINT advisory_matrix_advice_dispatch_unique UNIQUE
        (tenant_id, workspace_id, dispatch_id),
    CONSTRAINT advisory_matrix_advice_binding_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest, advice_id),
    CONSTRAINT advisory_matrix_advice_digest_check CHECK (
        matrix_choice_set_digest ~ '^[0-9a-f]{64}$'
        AND advice_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_matrix_advice_revision_check CHECK (matrix_task_revision >= 1),
    CONSTRAINT advisory_matrix_advice_kind_check CHECK (
        (kind = 'ranked' AND ranked_choice_ids IS NOT NULL
         AND pg_catalog.jsonb_typeof(ranked_choice_ids) = 'array'
         AND pg_catalog.jsonb_array_length(ranked_choice_ids) > 0)
        OR (kind IN ('abstained', 'rejected') AND ranked_choice_ids IS NULL)),
    CONSTRAINT advisory_matrix_advice_reason_check CHECK (
        reason IS NULL OR pg_catalog.btrim(reason) <> ''),
    CONSTRAINT advisory_matrix_advice_opportunity_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest)
        REFERENCES advisory_opportunity
        (tenant_id, workspace_id, id, work_item_id,
         matrix_task_revision, matrix_choice_binding_key),
    CONSTRAINT advisory_matrix_advice_dispatch_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, dispatch_id)
        REFERENCES advisory_dispatch
        (tenant_id, workspace_id, opportunity_id, id),
    CONSTRAINT advisory_matrix_advice_revision_fk FOREIGN KEY
        (tenant_id, workspace_id, task_id, matrix_task_revision,
         matrix_choice_set_digest)
        REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision, choice_set_digest)
);

CREATE TABLE advisory_matrix_disposition (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    task_id uuid NOT NULL,
    matrix_task_revision bigint NOT NULL,
    matrix_choice_set_digest text,
    matrix_choice_binding_key text GENERATED ALWAYS AS
        (COALESCE(matrix_choice_set_digest, 'no-choice')) STORED,
    disposition_id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    request_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    session_id uuid NOT NULL,
    basis text NOT NULL,
    advice_id uuid,
    outcome text NOT NULL,
    selected_choice_id text,
    blocked_reason text,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, disposition_id),
    CONSTRAINT advisory_matrix_disposition_request_unique UNIQUE
        (tenant_id, workspace_id, request_id),
    CONSTRAINT advisory_matrix_disposition_opportunity_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id),
    CONSTRAINT advisory_matrix_disposition_revision_check CHECK (matrix_task_revision >= 1),
    CONSTRAINT advisory_matrix_disposition_digest_check CHECK (
        matrix_choice_set_digest IS NULL OR
        matrix_choice_set_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_matrix_disposition_basis_check CHECK (
        (basis IN ('no_call', 'manual') AND advice_id IS NULL)
        OR (basis = 'after_advice' AND advice_id IS NOT NULL)),
    CONSTRAINT advisory_matrix_disposition_outcome_check CHECK (
        (outcome = 'selected' AND matrix_choice_set_digest IS NOT NULL
         AND selected_choice_id IS NOT NULL
         AND pg_catalog.btrim(selected_choice_id) <> ''
         AND blocked_reason IS NULL)
        OR (outcome = 'blocked' AND selected_choice_id IS NULL
            AND blocked_reason IS NOT NULL
            AND pg_catalog.btrim(blocked_reason) <> '')),
    CONSTRAINT advisory_matrix_disposition_opportunity_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_binding_key)
        REFERENCES advisory_opportunity
        (tenant_id, workspace_id, id, work_item_id,
         matrix_task_revision, matrix_choice_binding_key),
    CONSTRAINT advisory_matrix_disposition_advice_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest, advice_id)
        REFERENCES advisory_matrix_advice
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest, advice_id),
    CONSTRAINT advisory_matrix_disposition_revision_fk FOREIGN KEY
        (tenant_id, workspace_id, task_id, matrix_task_revision)
        REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision),
    CONSTRAINT advisory_matrix_disposition_actor_fk FOREIGN KEY (tenant_id, actor_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT advisory_matrix_disposition_session_fk FOREIGN KEY
        (tenant_id, workspace_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id)
);

CREATE INDEX advisory_matrix_disposition_task_lookup_idx ON advisory_matrix_disposition
    (tenant_id, workspace_id, task_id, matrix_task_revision DESC, created_at DESC);

-- The store must lock the opportunity and dispatch while checking lifecycle
-- state, and must validate ranking entries/selected_choice_id against the
-- owner choice-set payload. These semantic checks cannot be expressed by the
-- simple immutable row constraints above. It must also authenticate actor_id
-- against session_id and admit INSERT only; no advice is an auto-selection.
DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY[
        'advisory_matrix_advice', 'advisory_matrix_disposition'
    ] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format(
            'CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',
            relation_name||'_tenant_scope', relation_name, relation_name, relation_name);
    END LOOP;
END
$policy$;

REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_advice,
    advisory_matrix_disposition FROM PUBLIC;
