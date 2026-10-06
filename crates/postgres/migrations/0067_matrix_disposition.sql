-- An explicit decision is separate from optional advice; no planning effect.
ALTER TABLE advisory_matrix_advice
    ADD CONSTRAINT advisory_matrix_advice_disposition_binding_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, advice_id);

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
         matrix_task_revision, advice_id)
        REFERENCES advisory_matrix_advice
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, advice_id),
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


ALTER TABLE advisory_matrix_disposition ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_matrix_disposition FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_matrix_disposition_tenant_scope ON advisory_matrix_disposition
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_matrix_disposition'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_matrix_disposition'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_disposition FROM PUBLIC;

-- The runtime role cannot lock private identity tables. Recheck the agent's
-- authorization inside the INSERT under the same database transaction, then
-- hold these rows against revocation or membership removal until commit.
CREATE FUNCTION matrix_disposition_require_active_owner() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $active_owner$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'matrix disposition tenant does not match session'
            USING ERRCODE = '42501';
    END IF;

    SELECT true INTO authorized
    FROM public.agent_sessions AS s
    JOIN public.hosts AS h
        ON h.tenant_id = s.tenant_id AND h.id = s.host_id
    JOIN public.principals AS p
        ON p.tenant_id = h.tenant_id AND p.id = h.principal_id
    JOIN public.memberships AS m
        ON m.tenant_id = s.tenant_id AND m.workspace_id = s.workspace_id
        AND m.principal_id = p.id
    WHERE s.tenant_id = NEW.tenant_id
        AND s.workspace_id = NEW.workspace_id
        AND s.id = NEW.session_id
        AND h.principal_id = NEW.actor_id
        AND NOT s.revoked AND NOT h.revoked AND p.role = 'owner'
    FOR SHARE OF s, h, p, m;

    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix disposition requires an active workspace owner session'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$active_owner$;

CREATE TRIGGER advisory_matrix_disposition_active_owner
    BEFORE INSERT ON advisory_matrix_disposition
    FOR EACH ROW EXECUTE FUNCTION matrix_disposition_require_active_owner();

REVOKE ALL PRIVILEGES ON FUNCTION matrix_disposition_require_active_owner() FROM PUBLIC;
