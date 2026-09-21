-- Explicit legacy-to-successor pipeline run migration receipts.
-- The predecessor run remains an immutable definition/history record; only its
-- lifecycle status and revision advance atomically with the successor insert.
ALTER TABLE slice_pipeline_runs
    DROP CONSTRAINT slice_pipeline_runs_slice_unique,
    DROP CONSTRAINT slice_pipeline_runs_status_check,
    ADD CONSTRAINT slice_pipeline_runs_status_check
        CHECK (status IN ('active','waiting_input','blocked','completed','escalated','superseded'));

CREATE INDEX slice_pipeline_runs_slice_idx
    ON slice_pipeline_runs(tenant_id,workspace_id,slice_id,created_at,id);

CREATE TABLE slice_pipeline_run_migrations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    migration_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    request_payload jsonb NOT NULL,
    predecessor_run_id uuid NOT NULL,
    successor_run_id uuid NOT NULL,
    predecessor_revision bigint NOT NULL CHECK (predecessor_revision >= 1),
    expected_revision bigint NOT NULL CHECK (expected_revision >= 1),
    predecessor_definition_version text NOT NULL CHECK (pg_catalog.btrim(predecessor_definition_version) <> ''),
    predecessor_definition_digest text NOT NULL CHECK (pg_catalog.btrim(predecessor_definition_digest) <> ''),
    successor_definition_version text NOT NULL CHECK (pg_catalog.btrim(successor_definition_version) <> ''),
    successor_definition_digest text NOT NULL CHECK (pg_catalog.btrim(successor_definition_digest) <> ''),
    mappings jsonb NOT NULL,
    mappings_digest text NOT NULL CHECK (pg_catalog.btrim(mappings_digest) <> ''),
    result_payload jsonb NOT NULL,
    status text NOT NULL DEFAULT 'committed',
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,migration_id),
    CONSTRAINT slice_pipeline_run_migrations_key_unique UNIQUE (tenant_id,workspace_id,idempotency_key),
    CONSTRAINT slice_pipeline_run_migrations_predecessor_unique UNIQUE (tenant_id,workspace_id,predecessor_run_id),
    CONSTRAINT slice_pipeline_run_migrations_successor_unique UNIQUE (tenant_id,workspace_id,successor_run_id),
    CONSTRAINT slice_pipeline_run_migrations_payload_check CHECK (pg_catalog.jsonb_typeof(request_payload)='object' AND pg_catalog.jsonb_typeof(mappings)='array' AND pg_catalog.jsonb_typeof(result_payload)='object'),
    CONSTRAINT slice_pipeline_run_migrations_status_check CHECK (status IN ('committed','replayed','rejected')),
    CONSTRAINT slice_pipeline_run_migrations_predecessor_fk FOREIGN KEY (tenant_id,workspace_id,predecessor_run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_run_migrations_successor_fk FOREIGN KEY (tenant_id,workspace_id,successor_run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id)
);

CREATE INDEX slice_pipeline_run_migrations_predecessor_idx
    ON slice_pipeline_run_migrations(tenant_id,workspace_id,predecessor_run_id,created_at);

ALTER TABLE slice_pipeline_run_migrations ENABLE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_run_migrations FORCE ROW LEVEL SECURITY;
CREATE POLICY slice_pipeline_run_migrations_tenant_scope ON slice_pipeline_run_migrations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_run_migrations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_run_migrations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE TRIGGER slice_pipeline_run_migrations_created_at_immutable BEFORE UPDATE ON slice_pipeline_run_migrations FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
REVOKE ALL PRIVILEGES ON TABLE slice_pipeline_run_migrations FROM PUBLIC;
