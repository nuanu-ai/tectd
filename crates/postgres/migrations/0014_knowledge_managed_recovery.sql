-- Operator-only receipt proving one suppression checkpoint was applied to one
-- qualified restored database identity. Contains opaque recovery metadata only.
CREATE TABLE knowledge_suppression_recoveries (
  database_lineage_id uuid NOT NULL,
  erasure_sequence bigint NOT NULL CHECK (erasure_sequence >= 0),
  manifest_digest text NOT NULL,
  qualified_system_identifier text NOT NULL,
  qualified_database_oid oid NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (
    database_lineage_id,erasure_sequence,manifest_digest,
    qualified_system_identifier,qualified_database_oid
  )
);
REVOKE ALL PRIVILEGES ON TABLE knowledge_suppression_recoveries FROM PUBLIC;
