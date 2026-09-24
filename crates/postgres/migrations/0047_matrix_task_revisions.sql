-- Slice 02: owner-recorded Matrix task facts. These are accepted task inputs,
-- not independently verified environmental evidence or execution decisions.
-- A caller creates the head and revision 1 in one transaction; later writes
-- insert the next revision and CAS the head's current_revision in that transaction.

CREATE TABLE matrix_tasks (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    current_revision bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, id),
    CONSTRAINT matrix_tasks_revision_check CHECK (current_revision >= 1),
    CONSTRAINT matrix_tasks_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id)
);

CREATE TABLE matrix_task_revisions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    task_id uuid NOT NULL,
    revision bigint NOT NULL,
    previous_revision bigint,
    request_id uuid NOT NULL,
    input_schema text NOT NULL,
    canonical_input jsonb NOT NULL,
    input_digest text NOT NULL,
    recorded_by_principal_id uuid NOT NULL,
    recorded_by_session_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, task_id, revision),
    CONSTRAINT matrix_task_revisions_request_unique UNIQUE (tenant_id, workspace_id, request_id),
    CONSTRAINT matrix_task_revisions_chain_check CHECK (
        (revision = 1 AND previous_revision IS NULL)
        OR (revision > 1 AND previous_revision = revision - 1)
    ),
    CONSTRAINT matrix_task_revisions_schema_check
        CHECK (input_schema = 'tect.engineering-matrix-input/1'),
    CONSTRAINT matrix_task_revisions_payload_check CHECK (
        pg_catalog.jsonb_typeof(canonical_input) = 'object'
        AND canonical_input ?& ARRAY[
            'mode', 'envelope', 'criticality', 'intent', 'urgency',
            'promised_behavior', 'promised_proof', 'affected_guarantees',
            'actual_exposure', 'demand_commitment', 'latency_commitment', 'urgent_repair'
        ]
        AND pg_catalog.jsonb_typeof(canonical_input->'envelope') = 'object'
    ),
    CONSTRAINT matrix_task_revisions_digest_check CHECK (input_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT matrix_task_revisions_task_fk FOREIGN KEY (tenant_id, workspace_id, task_id)
        REFERENCES matrix_tasks (tenant_id, workspace_id, id),
    CONSTRAINT matrix_task_revisions_predecessor_fk
        FOREIGN KEY (tenant_id, workspace_id, task_id, previous_revision)
        REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision),
    CONSTRAINT matrix_task_revisions_principal_fk
        FOREIGN KEY (tenant_id, recorded_by_principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT matrix_task_revisions_session_fk
        FOREIGN KEY (tenant_id, workspace_id, recorded_by_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id)
);

-- The owner identity must still be active when the accepted fact is recorded.
-- Row locks hold that decision against concurrent session or host revocation,
-- principal role changes, and membership removal until this write commits.
CREATE FUNCTION matrix_task_revisions_require_active_owner() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $active_owner$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'matrix task revision tenant does not match session'
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
        AND s.id = NEW.recorded_by_session_id
        AND h.principal_id = NEW.recorded_by_principal_id
        AND NOT s.revoked AND NOT h.revoked AND p.role = 'owner'
    FOR SHARE OF s, h, p, m;

    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix task revision requires an active workspace owner session'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$active_owner$;

CREATE TRIGGER matrix_task_revisions_active_owner
    BEFORE INSERT ON matrix_task_revisions
    FOR EACH ROW EXECUTE FUNCTION matrix_task_revisions_require_active_owner();

REVOKE ALL PRIVILEGES ON FUNCTION matrix_task_revisions_require_active_owner() FROM PUBLIC;

-- Deferred so the first accepted revision and its head can be inserted together.
ALTER TABLE matrix_tasks
    ADD CONSTRAINT matrix_tasks_current_revision_fk
    FOREIGN KEY (tenant_id, workspace_id, id, current_revision)
    REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision)
    DEFERRABLE INITIALLY DEFERRED;

-- The deferred FK proves the selected revision exists at commit; this guard
-- also prevents a head from jumping ahead or returning to an older revision.
CREATE FUNCTION matrix_tasks_enforce_revision_step() RETURNS trigger
LANGUAGE plpgsql AS $revision_step$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.current_revision <> 1 THEN
            RAISE EXCEPTION 'matrix task head must start at revision 1'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.current_revision <> OLD.current_revision + 1 THEN
        RAISE EXCEPTION 'matrix task head must advance exactly one revision'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$revision_step$;

CREATE TRIGGER matrix_tasks_revision_step
    BEFORE INSERT OR UPDATE OF current_revision ON matrix_tasks
    FOR EACH ROW EXECUTE FUNCTION matrix_tasks_enforce_revision_step();

REVOKE ALL PRIVILEGES ON FUNCTION matrix_tasks_enforce_revision_step() FROM PUBLIC;

CREATE INDEX matrix_task_revisions_recent_idx ON matrix_task_revisions
    (tenant_id, workspace_id, recorded_at DESC, task_id, revision DESC);

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['matrix_tasks', 'matrix_task_revisions'] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format(
            'CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',
            relation_name||'_tenant_scope', relation_name, relation_name, relation_name);
    END LOOP;
END
$policy$;

REVOKE ALL PRIVILEGES ON TABLE matrix_tasks, matrix_task_revisions FROM PUBLIC;
