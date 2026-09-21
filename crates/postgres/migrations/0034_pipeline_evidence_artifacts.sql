-- Immutable, backend-owned evidence artifact metadata and bytes. v0.6 phase
-- artifacts remain untouched; this table is an additive evidence surface.
CREATE TABLE pipeline_evidence_artifacts (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  artifact_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision >= 1),
  digest text NOT NULL CHECK (digest ~ '^[0-9a-fA-F]{64}$'),
  size bigint NOT NULL CHECK (size >= 0),
  format text NOT NULL CHECK (btrim(format) <> '' AND octet_length(format) <= 128),
  provenance text NOT NULL CHECK (btrim(provenance) <> '' AND octet_length(provenance) <= 4096),
  target text NOT NULL CHECK (btrim(target) <> '' AND octet_length(target) <= 4096),
  readiness text NOT NULL CHECK (readiness IN ('uploading','ready','rejected')),
  body text NOT NULL DEFAULT '',
  request_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,artifact_id,revision),
  UNIQUE (tenant_id,workspace_id,request_id),
  CONSTRAINT pipeline_evidence_artifact_body_shape CHECK (
    (readiness='uploading' AND body='') OR (readiness IN ('ready','rejected'))
  )
);
ALTER TABLE slice_pipeline_phase_outputs
  ADD COLUMN evidence_artifacts jsonb NOT NULL DEFAULT '[]'::jsonb,
  ADD CONSTRAINT slice_pipeline_output_evidence_artifacts_shape CHECK (pg_catalog.jsonb_typeof(evidence_artifacts)='array');
CREATE INDEX pipeline_evidence_artifacts_lookup
  ON pipeline_evidence_artifacts(tenant_id,workspace_id,artifact_id,revision);
ALTER TABLE pipeline_evidence_artifacts ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_evidence_artifacts FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_evidence_artifacts_tenant ON pipeline_evidence_artifacts
  USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_evidence_artifacts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
  WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_evidence_artifacts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_evidence_artifacts FROM PUBLIC;
CREATE TRIGGER pipeline_evidence_artifacts_created_at_immutable BEFORE UPDATE
  ON pipeline_evidence_artifacts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
