-- A verification is an immutable, independently authenticated observation of
-- one accepted Matrix task revision. It grants no authority to edit the task.
ALTER TABLE matrix_task_revisions
    ADD CONSTRAINT matrix_task_revisions_input_binding_unique
    UNIQUE (tenant_id, workspace_id, task_id, revision, input_digest);

CREATE TABLE matrix_verifications (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    task_id uuid NOT NULL,
    task_revision bigint NOT NULL,
    input_digest text NOT NULL,
    schema text NOT NULL,
    owner_principal_id uuid NOT NULL,
    verifier_principal_id uuid NOT NULL,
    verifier_session_id uuid NOT NULL,
    verification_reason text NOT NULL,
    policy_version text NOT NULL,
    record_digest text NOT NULL,
    verified_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, id),
    CONSTRAINT matrix_verifications_digest_unique
        UNIQUE (tenant_id, workspace_id, task_id, task_revision, record_digest),
    CONSTRAINT matrix_verifications_revision_fk FOREIGN KEY
        (tenant_id, workspace_id, task_id, task_revision, input_digest)
        REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision, input_digest),
    CONSTRAINT matrix_verifications_owner_fk FOREIGN KEY (tenant_id, owner_principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT matrix_verifications_verifier_fk FOREIGN KEY (tenant_id, verifier_principal_id)
        REFERENCES principals (tenant_id, id),
    CONSTRAINT matrix_verifications_session_fk FOREIGN KEY
        (tenant_id, workspace_id, verifier_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT matrix_verifications_schema_check
        CHECK (schema = 'tect.matrix-verification/1'),
    CONSTRAINT matrix_verifications_reason_check
        CHECK (verification_reason = 'matrix_facts_verified'),
    CONSTRAINT matrix_verifications_principals_distinct_check
        CHECK (owner_principal_id <> verifier_principal_id),
    CONSTRAINT matrix_verifications_digest_check CHECK (
        input_digest ~ '^[0-9a-f]{64}$' AND record_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT matrix_verifications_policy_check CHECK (
        pg_catalog.length(pg_catalog.btrim(policy_version)) BETWEEN 1 AND 256)
);

CREATE TABLE matrix_verification_bindings (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    verification_id uuid NOT NULL,
    fact_path text NOT NULL,
    value_digest text NOT NULL,
    evidence_ref text NOT NULL,
    content_digest text NOT NULL,
    source text NOT NULL,
    subject text NOT NULL,
    observed_at bigint NOT NULL,
    expires_at bigint NOT NULL,
    validation_outcome text NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, verification_id, fact_path),
    CONSTRAINT matrix_verification_bindings_record_fk FOREIGN KEY
        (tenant_id, workspace_id, verification_id)
        REFERENCES matrix_verifications (tenant_id, workspace_id, id),
    CONSTRAINT matrix_verification_bindings_digest_check CHECK (
        value_digest ~ '^[0-9a-f]{64}$' AND content_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT matrix_verification_bindings_text_check CHECK (
        pg_catalog.length(pg_catalog.btrim(fact_path)) BETWEEN 1 AND 1024
        AND pg_catalog.length(pg_catalog.btrim(evidence_ref)) BETWEEN 1 AND 4096
        AND pg_catalog.length(pg_catalog.btrim(source)) BETWEEN 1 AND 256
        AND pg_catalog.length(pg_catalog.btrim(subject)) BETWEEN 1 AND 256),
    CONSTRAINT matrix_verification_bindings_time_check
        CHECK (expires_at > observed_at),
    CONSTRAINT matrix_verification_bindings_outcome_check
        CHECK (validation_outcome = 'accepted')
);

-- Hold the current head and identity rows through the insert. The application
-- has already locked the task head before invoking its evidence validator.
CREATE FUNCTION matrix_verifications_require_active_verifier() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $active_verifier$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'matrix verification tenant does not match session'
            USING ERRCODE = '42501';
    END IF;

    SELECT true INTO authorized
    FROM public.matrix_tasks AS t
    JOIN public.matrix_task_revisions AS r
      ON r.tenant_id=t.tenant_id AND r.workspace_id=t.workspace_id
      AND r.task_id=t.id AND r.revision=NEW.task_revision
    JOIN public.agent_sessions AS s
      ON s.tenant_id=t.tenant_id AND s.workspace_id=t.workspace_id
      AND s.id=NEW.verifier_session_id
    JOIN public.hosts AS h
      ON h.tenant_id=s.tenant_id AND h.id=s.host_id
    JOIN public.principals AS p
      ON p.tenant_id=h.tenant_id AND p.id=h.principal_id
    JOIN public.memberships AS m
      ON m.tenant_id=t.tenant_id AND m.workspace_id=t.workspace_id
      AND m.principal_id=p.id
    WHERE t.tenant_id=NEW.tenant_id AND t.workspace_id=NEW.workspace_id
      AND t.id=NEW.task_id AND t.current_revision=NEW.task_revision
      AND r.input_digest=NEW.input_digest
      AND r.recorded_by_principal_id=NEW.owner_principal_id
      AND p.id=NEW.verifier_principal_id AND p.role='verifier'
      AND p.id<>NEW.owner_principal_id
      AND NOT s.revoked AND NOT h.revoked
    FOR SHARE OF t, r, s, h, p, m;

    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix verification requires current revision and active independent verifier'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$active_verifier$;

CREATE TRIGGER matrix_verifications_active_verifier
    BEFORE INSERT ON matrix_verifications
    FOR EACH ROW EXECUTE FUNCTION matrix_verifications_require_active_verifier();

CREATE FUNCTION matrix_verification_deny_mutation() RETURNS trigger
LANGUAGE plpgsql AS $immutable$
BEGIN
    RAISE EXCEPTION 'matrix verification records are immutable' USING ERRCODE = '42501';
END
$immutable$;

CREATE TRIGGER matrix_verifications_immutable
    BEFORE UPDATE OR DELETE ON matrix_verifications
    FOR EACH ROW EXECUTE FUNCTION matrix_verification_deny_mutation();
CREATE TRIGGER matrix_verification_bindings_immutable
    BEFORE UPDATE OR DELETE ON matrix_verification_bindings
    FOR EACH ROW EXECUTE FUNCTION matrix_verification_deny_mutation();

REVOKE ALL PRIVILEGES ON FUNCTION matrix_verifications_require_active_verifier() FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION matrix_verification_deny_mutation() FROM PUBLIC;

CREATE INDEX matrix_verifications_current_lookup_idx ON matrix_verifications
    (tenant_id, workspace_id, task_id, task_revision, input_digest, verified_at DESC);

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['matrix_verifications', 'matrix_verification_bindings'] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', relation_name);
        EXECUTE pg_catalog.format(
            'CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',
            relation_name||'_tenant_scope', relation_name, relation_name, relation_name);
    END LOOP;
END
$policy$;

REVOKE ALL PRIVILEGES ON TABLE matrix_verifications, matrix_verification_bindings FROM PUBLIC;
