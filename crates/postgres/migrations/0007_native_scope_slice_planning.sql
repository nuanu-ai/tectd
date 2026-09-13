-- Native Scope/Slice planning. Pipeline execution is deliberately absent.
CREATE TABLE native_scopes (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    source_candidate_set_id uuid NOT NULL,
    source_candidate_set_revision bigint NOT NULL CHECK (source_candidate_set_revision >= 1),
    source_snapshot_id uuid NOT NULL,
    source_candidate_id uuid NOT NULL,
    source_candidate_revision bigint NOT NULL CHECK (source_candidate_revision >= 1),
    boundary text NOT NULL CHECK (boundary IN ('finite','ongoing')),
    title text NOT NULL CHECK (pg_catalog.btrim(title) <> ''),
    outcome text NOT NULL CHECK (pg_catalog.btrim(outcome) <> ''),
    includes jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(includes)='array'),
    excludes jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(excludes)='array'),
    slice_candidate_set_id uuid,
    origin_request_id uuid NOT NULL,
    origin_payload jsonb NOT NULL,
    origin_result jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT native_scopes_workspace_fk FOREIGN KEY (tenant_id,workspace_id) REFERENCES workspaces(tenant_id,id),
    CONSTRAINT native_scopes_source_set_fk FOREIGN KEY (tenant_id,workspace_id,source_candidate_set_id) REFERENCES scope_candidate_sets(tenant_id,workspace_id,id),
    CONSTRAINT native_scopes_scope_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT native_scopes_source_candidate_unique UNIQUE (tenant_id,workspace_id,source_candidate_id),
    CONSTRAINT native_scopes_request_unique UNIQUE (tenant_id,workspace_id,origin_request_id)
);

CREATE TABLE slice_candidate_sets (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    scope_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    status text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','review_required','ready','blocked')),
    current_snapshot_id uuid,
    input_cursor bigint NOT NULL DEFAULT 0 CHECK (input_cursor >= 0),
    latest_input bigint NOT NULL DEFAULT 0 CHECK (latest_input >= 0),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_candidate_sets_scope_fk FOREIGN KEY (tenant_id,workspace_id,scope_id) REFERENCES native_scopes(tenant_id,workspace_id,id),
    CONSTRAINT slice_candidate_sets_scope_unique UNIQUE (tenant_id,workspace_id,scope_id),
    CONSTRAINT slice_candidate_sets_set_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT slice_candidate_sets_cursor_check CHECK (input_cursor <= latest_input)
);

CREATE TABLE slice_planning_inputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    sequence bigint NOT NULL CHECK (sequence >= 1),
    request_id uuid,
    session_id uuid NOT NULL,
    source_result_id uuid,
    input text NOT NULL CHECK (pg_catalog.btrim(input) <> ''),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_planning_inputs_set_fk FOREIGN KEY (tenant_id,workspace_id,candidate_set_id) REFERENCES slice_candidate_sets(tenant_id,workspace_id,id),
    CONSTRAINT slice_planning_inputs_session_fk FOREIGN KEY (tenant_id,workspace_id,session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id),
    CONSTRAINT slice_planning_inputs_sequence_unique UNIQUE (tenant_id,workspace_id,candidate_set_id,sequence)
);
CREATE UNIQUE INDEX slice_planning_inputs_request_unique
    ON slice_planning_inputs(tenant_id,workspace_id,candidate_set_id,request_id)
    WHERE request_id IS NOT NULL;

CREATE TABLE slice_planning_snapshots (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    sequence bigint NOT NULL CHECK (sequence >= 1),
    scope_revision bigint NOT NULL CHECK (scope_revision >= 1),
    source_candidate_set_revision bigint NOT NULL CHECK (source_candidate_set_revision >= 1),
    source_snapshot_id uuid NOT NULL,
    planning_latest_input bigint NOT NULL CHECK (planning_latest_input >= 0),
    method jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(method)='object'),
    registry_revision text NOT NULL CHECK (pg_catalog.btrim(registry_revision)<>''),
    registry_digest text NOT NULL CHECK (pg_catalog.btrim(registry_digest)<>''),
    rules jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(rules)='array'),
    catalogue jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(catalogue)='object'),
    result_ids uuid[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_planning_snapshots_set_fk FOREIGN KEY (tenant_id,workspace_id,candidate_set_id) REFERENCES slice_candidate_sets(tenant_id,workspace_id,id),
    CONSTRAINT slice_planning_snapshots_snapshot_unique UNIQUE (tenant_id,workspace_id,candidate_set_id,id),
    CONSTRAINT slice_planning_snapshots_sequence_unique UNIQUE (tenant_id,workspace_id,candidate_set_id,sequence)
);

ALTER TABLE slice_candidate_sets ADD CONSTRAINT slice_candidate_sets_snapshot_fk
    FOREIGN KEY (tenant_id,workspace_id,id,current_snapshot_id)
    REFERENCES slice_planning_snapshots(tenant_id,workspace_id,candidate_set_id,id);
ALTER TABLE native_scopes ADD CONSTRAINT native_scopes_slice_set_fk
    FOREIGN KEY (tenant_id,workspace_id,slice_candidate_set_id)
    REFERENCES slice_candidate_sets(tenant_id,workspace_id,id);

CREATE TABLE slice_candidate_drafts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    set_revision bigint NOT NULL CHECK (set_revision >= 2),
    payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(payload)='object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,set_revision),
    CONSTRAINT slice_candidate_drafts_set_fk FOREIGN KEY (tenant_id,workspace_id,candidate_set_id) REFERENCES slice_candidate_sets(tenant_id,workspace_id,id)
);

CREATE TABLE slice_candidate_reviews (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    set_revision bigint NOT NULL CHECK (set_revision >= 3),
    payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(payload)='object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,set_revision),
    CONSTRAINT slice_candidate_reviews_set_fk FOREIGN KEY (tenant_id,workspace_id,candidate_set_id) REFERENCES slice_candidate_sets(tenant_id,workspace_id,id)
);

CREATE TABLE native_slices (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    scope_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    candidate_id uuid NOT NULL,
    candidate_revision bigint NOT NULL CHECK (candidate_revision >= 1),
    opening_snapshot_id uuid NOT NULL,
    title text NOT NULL CHECK (pg_catalog.btrim(title)<>''),
    outcome text NOT NULL CHECK (pg_catalog.btrim(outcome)<>''),
    pipeline text NOT NULL CHECK (pipeline IN (
        'slice.lightweight-tdd-development','slice.full-design-to-execution','slice.debug-root-cause',
        'slice.operational-preparation','slice.operational-execution','slice.research-to-durable-knowledge',
        'slice.custom-procedure-capture')),
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open','completed','blocked')),
    origin_request_id uuid NOT NULL,
    origin_payload jsonb NOT NULL,
    origin_result jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT native_slices_scope_fk FOREIGN KEY (tenant_id,workspace_id,scope_id) REFERENCES native_scopes(tenant_id,workspace_id,id),
    CONSTRAINT native_slices_slice_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT native_slices_candidate_unique UNIQUE (tenant_id,workspace_id,scope_id,candidate_id),
    CONSTRAINT native_slices_request_unique UNIQUE (tenant_id,workspace_id,origin_request_id)
);

-- Scope/set/snapshot ownership is checked transactionally. The direct Scope FK
-- and immutable snapshot row retain the durable ownership boundary.

CREATE TABLE slice_results (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    scope_id uuid NOT NULL,
    slice_id uuid NOT NULL,
    slice_revision bigint NOT NULL CHECK (slice_revision >= 1),
    revision bigint NOT NULL CHECK (revision >= 1),
    outcome text NOT NULL CHECK (outcome IN ('completed','blocked')),
    summary text NOT NULL CHECK (pg_catalog.btrim(summary)<>''),
    evidence jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(evidence)='array' AND pg_catalog.jsonb_array_length(evidence)>0),
    scope_impact text NOT NULL CHECK (pg_catalog.btrim(scope_impact)<>''),
    remaining_work text NOT NULL CHECK (pg_catalog.btrim(remaining_work)<>''),
    provenance text NOT NULL DEFAULT 'externally_reported' CHECK (provenance='externally_reported'),
    request_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_results_scope_fk FOREIGN KEY (tenant_id,workspace_id,scope_id) REFERENCES native_scopes(tenant_id,workspace_id,id),
    CONSTRAINT slice_results_slice_fk FOREIGN KEY (tenant_id,workspace_id,slice_id) REFERENCES native_slices(tenant_id,workspace_id,id),
    CONSTRAINT slice_results_revision_unique UNIQUE (tenant_id,workspace_id,slice_id,revision),
    CONSTRAINT slice_results_request_unique UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT slice_results_result_unique UNIQUE (tenant_id,workspace_id,id)
);
ALTER TABLE slice_planning_inputs ADD CONSTRAINT slice_planning_inputs_result_fk
    FOREIGN KEY (tenant_id,workspace_id,source_result_id) REFERENCES slice_results(tenant_id,workspace_id,id);

CREATE TABLE native_planning_receipts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    entity_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('save_slice_draft','review_slice_set','record_slice_input','refresh_slice_set')),
    request_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,entity_id,operation,request_id)
);

CREATE INDEX native_scopes_epoch_idx ON native_scopes(tenant_id,workspace_id,epoch_month,created_at,id);
CREATE INDEX slice_candidate_sets_epoch_idx ON slice_candidate_sets(tenant_id,workspace_id,epoch_month,created_at,id);
CREATE INDEX slice_planning_inputs_epoch_idx ON slice_planning_inputs(tenant_id,workspace_id,epoch_month,created_at,candidate_set_id,sequence);
CREATE INDEX slice_planning_snapshots_epoch_idx ON slice_planning_snapshots(tenant_id,workspace_id,epoch_month,created_at,candidate_set_id,sequence);
CREATE INDEX slice_candidate_drafts_epoch_idx ON slice_candidate_drafts(tenant_id,workspace_id,epoch_month,created_at,candidate_set_id,set_revision);
CREATE INDEX slice_candidate_reviews_epoch_idx ON slice_candidate_reviews(tenant_id,workspace_id,epoch_month,created_at,candidate_set_id,set_revision);
CREATE INDEX native_slices_epoch_idx ON native_slices(tenant_id,workspace_id,epoch_month,created_at,id);
CREATE INDEX slice_results_epoch_idx ON slice_results(tenant_id,workspace_id,epoch_month,created_at,slice_id,revision);
CREATE INDEX native_planning_receipts_epoch_idx ON native_planning_receipts(tenant_id,workspace_id,epoch_month,created_at,entity_id);

ALTER TABLE native_scopes ENABLE ROW LEVEL SECURITY; ALTER TABLE native_scopes FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_candidate_sets ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_candidate_sets FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_planning_inputs ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_planning_inputs FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_planning_snapshots ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_planning_snapshots FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_candidate_drafts ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_candidate_drafts FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_candidate_reviews ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_candidate_reviews FORCE ROW LEVEL SECURITY;
ALTER TABLE native_slices ENABLE ROW LEVEL SECURITY; ALTER TABLE native_slices FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_results ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_results FORCE ROW LEVEL SECURITY;
ALTER TABLE native_planning_receipts ENABLE ROW LEVEL SECURITY; ALTER TABLE native_planning_receipts FORCE ROW LEVEL SECURITY;

CREATE POLICY native_scopes_tenant_scope ON native_scopes USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_scopes'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_scopes'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_candidate_sets_tenant_scope ON slice_candidate_sets USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_sets'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_sets'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_planning_inputs_tenant_scope ON slice_planning_inputs USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_planning_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_planning_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_planning_snapshots_tenant_scope ON slice_planning_snapshots USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_planning_snapshots'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_planning_snapshots'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_candidate_drafts_tenant_scope ON slice_candidate_drafts USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_drafts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_drafts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_candidate_reviews_tenant_scope ON slice_candidate_reviews USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_reviews'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_candidate_reviews'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY native_slices_tenant_scope ON native_slices USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_slices'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_slices'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_results_tenant_scope ON slice_results USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_results'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_results'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY native_planning_receipts_tenant_scope ON native_planning_receipts USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_planning_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='native_planning_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

CREATE TRIGGER native_scopes_created_at_immutable BEFORE UPDATE ON native_scopes FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_candidate_sets_created_at_immutable BEFORE UPDATE ON slice_candidate_sets FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_planning_inputs_created_at_immutable BEFORE UPDATE ON slice_planning_inputs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_planning_snapshots_created_at_immutable BEFORE UPDATE ON slice_planning_snapshots FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_candidate_drafts_created_at_immutable BEFORE UPDATE ON slice_candidate_drafts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_candidate_reviews_created_at_immutable BEFORE UPDATE ON slice_candidate_reviews FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER native_slices_created_at_immutable BEFORE UPDATE ON native_slices FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_results_created_at_immutable BEFORE UPDATE ON slice_results FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER native_planning_receipts_created_at_immutable BEFORE UPDATE ON native_planning_receipts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE native_scopes,slice_candidate_sets,slice_planning_inputs,
    slice_planning_snapshots,slice_candidate_drafts,slice_candidate_reviews,native_slices,
    slice_results,native_planning_receipts FROM PUBLIC;
