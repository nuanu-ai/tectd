CREATE TABLE scope_candidate_sets (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    program_id uuid NOT NULL,
    origin_request_id uuid NOT NULL,
    origin_input text NOT NULL,
    origin_payload jsonb NOT NULL,
    origin_result jsonb,
    revision bigint NOT NULL DEFAULT 1,
    status text NOT NULL DEFAULT 'draft',
    boundary text NOT NULL,
    current_snapshot_id uuid,
    input_cursor bigint NOT NULL DEFAULT 0,
    latest_input bigint NOT NULL DEFAULT 1,
    max_input_bytes bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    CONSTRAINT scope_candidate_sets_revision_check CHECK (revision >= 1),
    CONSTRAINT scope_candidate_sets_status_check CHECK (
        status IN ('draft', 'review_required', 'ready', 'blocked')
    ),
    CONSTRAINT scope_candidate_sets_boundary_check CHECK (boundary IN ('finite', 'ongoing')),
    CONSTRAINT scope_candidate_sets_origin_result_check CHECK (
        origin_result IS NULL OR pg_catalog.jsonb_typeof(origin_result)='object'
    ),
    CONSTRAINT scope_candidate_sets_input_check CHECK (
        pg_catalog.btrim(origin_input) <> '' AND input_cursor >= 0
        AND latest_input >= 1 AND input_cursor <= latest_input AND max_input_bytes >= 0
    ),
    CONSTRAINT scope_candidate_sets_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id),
    CONSTRAINT scope_candidate_sets_program_fk FOREIGN KEY (tenant_id, workspace_id, program_id)
        REFERENCES programs (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_sets_scope_unique UNIQUE (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_sets_program_unique UNIQUE (tenant_id, workspace_id, program_id),
    CONSTRAINT scope_candidate_sets_request_unique UNIQUE (tenant_id, workspace_id, origin_request_id)
);

CREATE TABLE scope_candidate_inputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    sequence bigint NOT NULL,
    request_id uuid NOT NULL,
    session_id uuid NOT NULL,
    input text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    CONSTRAINT scope_candidate_inputs_text_check CHECK (pg_catalog.btrim(input) <> ''),
    CONSTRAINT scope_candidate_inputs_sequence_check CHECK (sequence >= 1),
    CONSTRAINT scope_candidate_inputs_set_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_inputs_session_fk
        FOREIGN KEY (tenant_id, workspace_id, session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_inputs_sequence_unique
        UNIQUE (tenant_id, workspace_id, candidate_set_id, sequence),
    CONSTRAINT scope_candidate_inputs_request_unique
        UNIQUE (tenant_id, workspace_id, candidate_set_id, request_id)
);

CREATE TABLE scope_candidate_contents (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    digest text NOT NULL,
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, digest),
    CONSTRAINT scope_candidate_contents_digest_check CHECK (digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT scope_candidate_contents_workspace_fk FOREIGN KEY (tenant_id, workspace_id)
        REFERENCES workspaces (tenant_id, id)
);

CREATE TABLE scope_candidate_snapshots (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    sequence bigint NOT NULL,
    program_revision bigint NOT NULL,
    program_latest_input bigint NOT NULL,
    planning_latest_input bigint NOT NULL,
    program_body_digest text NOT NULL,
    selected_worktree_ids uuid[] NOT NULL,
    selected_sources_digest text NOT NULL,
    method_id text NOT NULL,
    method_revision text NOT NULL,
    method_digest text NOT NULL,
    method_body text NOT NULL,
    method_origin_refs jsonb NOT NULL,
    registry_revision text NOT NULL,
    registry_digest text NOT NULL,
    rules jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    CONSTRAINT scope_candidate_snapshots_position_check CHECK (
        sequence >= 1 AND program_revision >= 1 AND program_latest_input >= 1
        AND planning_latest_input >= 1
    ),
    CONSTRAINT scope_candidate_snapshots_digest_check CHECK (
        program_body_digest ~ '^[0-9a-f]{64}$'
        AND selected_sources_digest ~ '^[0-9a-f]{64}$'
        AND method_digest ~ '^[0-9a-f]{64}$'
        AND registry_digest ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT scope_candidate_snapshots_rules_check CHECK (
        pg_catalog.jsonb_typeof(rules) = 'array'
        AND pg_catalog.jsonb_typeof(method_origin_refs) = 'array'
    ),
    CONSTRAINT scope_candidate_snapshots_set_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_snapshots_content_fk
        FOREIGN KEY (tenant_id, workspace_id, program_body_digest)
        REFERENCES scope_candidate_contents (tenant_id, workspace_id, digest),
    CONSTRAINT scope_candidate_snapshots_scope_unique
        UNIQUE (tenant_id, workspace_id, candidate_set_id, id),
    CONSTRAINT scope_candidate_snapshots_sequence_unique
        UNIQUE (tenant_id, workspace_id, candidate_set_id, sequence)
);

ALTER TABLE scope_candidate_sets
    ADD CONSTRAINT scope_candidate_sets_snapshot_fk
    FOREIGN KEY (tenant_id, workspace_id, id, current_snapshot_id)
    REFERENCES scope_candidate_snapshots (tenant_id, workspace_id, candidate_set_id, id);

CREATE TABLE scope_candidate_source_refs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    snapshot_id uuid NOT NULL,
    kind text NOT NULL,
    input_sequence bigint,
    program_field text,
    body_digest text NOT NULL,
    label text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    CONSTRAINT scope_candidate_source_refs_kind_check CHECK (
        (kind='program_success' AND input_sequence IS NULL AND program_field='success')
        OR (kind='program_field' AND input_sequence IS NULL AND program_field IN
            ('name','intent','basis','boundaries','constraints'))
        OR (kind='planning_input' AND input_sequence >= 1 AND program_field IS NULL)
    ),
    CONSTRAINT scope_candidate_source_refs_snapshot_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id, snapshot_id)
        REFERENCES scope_candidate_snapshots (tenant_id, workspace_id, candidate_set_id, id),
    CONSTRAINT scope_candidate_source_refs_content_fk
        FOREIGN KEY (tenant_id, workspace_id, body_digest)
        REFERENCES scope_candidate_contents (tenant_id, workspace_id, digest),
    CONSTRAINT scope_candidate_source_refs_scope_unique
        UNIQUE (tenant_id, workspace_id, candidate_set_id, id),
    CONSTRAINT scope_candidate_source_refs_kind_unique
        UNIQUE NULLS NOT DISTINCT
        (tenant_id, workspace_id, candidate_set_id, snapshot_id, kind, input_sequence, program_field)
);

CREATE TABLE scope_candidate_drafts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    set_revision bigint NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, set_revision),
    CONSTRAINT scope_candidate_drafts_revision_check CHECK (set_revision >= 2),
    CONSTRAINT scope_candidate_drafts_payload_check CHECK (pg_catalog.jsonb_typeof(payload)='object'),
    CONSTRAINT scope_candidate_drafts_set_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id)
);

CREATE TABLE scope_candidate_reviews (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    set_revision bigint NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, set_revision),
    CONSTRAINT scope_candidate_reviews_revision_check CHECK (set_revision >= 3),
    CONSTRAINT scope_candidate_reviews_payload_check CHECK (pg_catalog.jsonb_typeof(payload)='object'),
    CONSTRAINT scope_candidate_reviews_set_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id)
);

CREATE TABLE scope_candidate_receipts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    operation text NOT NULL,
    request_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_revision bigint NOT NULL,
    result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (
        pg_catalog.date_trunc('month', created_at AT TIME ZONE 'UTC')::date
    ) STORED NOT NULL,
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, operation, request_id),
    CONSTRAINT scope_candidate_receipts_operation_check CHECK (
        operation IN ('save_draft','save_review','record_input','refresh')
    ),
    CONSTRAINT scope_candidate_receipts_revision_check CHECK (result_revision >= 1),
    CONSTRAINT scope_candidate_receipts_set_fk
        FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id)
);

CREATE INDEX scope_candidate_sets_epoch_idx
    ON scope_candidate_sets (tenant_id, workspace_id, epoch_month, created_at, id);
CREATE INDEX scope_candidate_inputs_epoch_idx
    ON scope_candidate_inputs (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, sequence);
CREATE INDEX scope_candidate_contents_epoch_idx
    ON scope_candidate_contents (tenant_id, workspace_id, epoch_month, created_at, digest);
CREATE INDEX scope_candidate_snapshots_epoch_idx
    ON scope_candidate_snapshots (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, sequence);
CREATE INDEX scope_candidate_source_refs_epoch_idx
    ON scope_candidate_source_refs (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, id);
CREATE INDEX scope_candidate_drafts_epoch_idx
    ON scope_candidate_drafts (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, set_revision);
CREATE INDEX scope_candidate_reviews_epoch_idx
    ON scope_candidate_reviews (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, set_revision);
CREATE INDEX scope_candidate_receipts_epoch_idx
    ON scope_candidate_receipts (tenant_id, workspace_id, epoch_month, created_at, candidate_set_id, request_id);

ALTER TABLE scope_candidate_sets ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_sets FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_inputs ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_inputs FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_contents ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_contents FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_snapshots FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_source_refs ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_source_refs FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_drafts ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_drafts FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_reviews FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_receipts ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_receipts FORCE ROW LEVEL SECURITY;

CREATE POLICY scope_candidate_sets_tenant_scope ON scope_candidate_sets
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_sets'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_sets'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_inputs_tenant_scope ON scope_candidate_inputs
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_contents_tenant_scope ON scope_candidate_contents
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_contents'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_contents'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_snapshots_tenant_scope ON scope_candidate_snapshots
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_snapshots'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_snapshots'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_source_refs_tenant_scope ON scope_candidate_source_refs
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_source_refs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_source_refs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_drafts_tenant_scope ON scope_candidate_drafts
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_drafts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_drafts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_reviews_tenant_scope ON scope_candidate_reviews
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_reviews'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_reviews'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_receipts_tenant_scope ON scope_candidate_receipts
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_candidate_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

CREATE TRIGGER scope_candidate_sets_created_at_immutable
    BEFORE UPDATE ON scope_candidate_sets FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_inputs_created_at_immutable
    BEFORE UPDATE ON scope_candidate_inputs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_contents_created_at_immutable
    BEFORE UPDATE ON scope_candidate_contents FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_snapshots_created_at_immutable
    BEFORE UPDATE ON scope_candidate_snapshots FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_source_refs_created_at_immutable
    BEFORE UPDATE ON scope_candidate_source_refs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_drafts_created_at_immutable
    BEFORE UPDATE ON scope_candidate_drafts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_reviews_created_at_immutable
    BEFORE UPDATE ON scope_candidate_reviews FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER scope_candidate_receipts_created_at_immutable
    BEFORE UPDATE ON scope_candidate_receipts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE scope_candidate_sets, scope_candidate_inputs,
    scope_candidate_contents, scope_candidate_snapshots, scope_candidate_source_refs,
    scope_candidate_drafts, scope_candidate_reviews, scope_candidate_receipts FROM PUBLIC;
