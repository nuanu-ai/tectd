-- Backend-owned acknowledgement of an immutable pipeline manifest delivery.
-- One receipt exists per run context epoch; refreshes reuse its pinned digest.
CREATE TABLE pipeline_delivery_receipts (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  delivery_id uuid NOT NULL,
  run_id uuid NOT NULL,
  context_epoch bigint NOT NULL CHECK (context_epoch >= 1),
  manifest_digest text NOT NULL CHECK (pg_catalog.btrim(manifest_digest) <> ''),
  delivered_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,delivery_id),
  CONSTRAINT pipeline_delivery_receipts_run_fk
    FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_delivery_receipts_epoch_unique
    UNIQUE (tenant_id,workspace_id,run_id,context_epoch)
);

CREATE INDEX pipeline_delivery_receipts_run_idx
  ON pipeline_delivery_receipts(tenant_id,workspace_id,run_id,context_epoch);
ALTER TABLE pipeline_delivery_receipts ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_delivery_receipts FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_delivery_receipts_tenant ON pipeline_delivery_receipts
  USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_delivery_receipts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
  WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_delivery_receipts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_delivery_receipts FROM PUBLIC;
CREATE TRIGGER pipeline_delivery_receipts_created_at_immutable BEFORE UPDATE
  ON pipeline_delivery_receipts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
