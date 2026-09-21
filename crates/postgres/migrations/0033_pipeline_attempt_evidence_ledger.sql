-- Backend-owned attempt receipt/evidence ledger.  The legacy request payload
-- remains intact for v0.6 replay/decode compatibility, while these columns
-- contain only references resolved from authoritative rows in this transaction.
ALTER TABLE slice_pipeline_phase_attempts
  ADD COLUMN evidence_refs jsonb NOT NULL DEFAULT '[]'::jsonb,
  ADD COLUMN knowledge_manifest_id uuid,
  ADD COLUMN knowledge_manifest_digest text,
  ADD COLUMN knowledge_workspace_generation bigint,
  ADD COLUMN stale_dependency boolean NOT NULL DEFAULT false,
  ADD COLUMN stale_reason text,
  ADD CONSTRAINT slice_pipeline_attempt_evidence_refs_shape CHECK (
    pg_catalog.jsonb_typeof(evidence_refs)='array'
    AND pg_catalog.octet_length(evidence_refs::text) <= 262144
  ),
  ADD CONSTRAINT slice_pipeline_attempt_knowledge_binding_shape CHECK (
    (knowledge_manifest_id IS NULL AND knowledge_manifest_digest IS NULL
      AND knowledge_workspace_generation IS NULL)
    OR (knowledge_manifest_id IS NOT NULL
      AND pg_catalog.btrim(knowledge_manifest_digest) <> ''
      AND knowledge_workspace_generation >= 0)
  ),
  ADD CONSTRAINT slice_pipeline_attempt_stale_shape CHECK (
    (stale_dependency AND pg_catalog.btrim(stale_reason) <> '')
    OR (NOT stale_dependency AND stale_reason IS NULL)
  ),
  ADD CONSTRAINT slice_pipeline_attempt_manifest_fk
    FOREIGN KEY (tenant_id,workspace_id,knowledge_manifest_id)
    REFERENCES pipeline_knowledge_manifests(tenant_id,workspace_id,id);

CREATE INDEX slice_pipeline_attempt_evidence_idx
  ON slice_pipeline_phase_attempts(tenant_id,workspace_id,run_id,phase_id,attempt);
