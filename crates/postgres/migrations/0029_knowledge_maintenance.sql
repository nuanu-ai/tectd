-- DK-4 durable maintenance signals, work leases, handoffs, and registered consumers.
CREATE TABLE knowledge_maintenance_signals (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_id uuid,
  unit_id uuid NOT NULL,
  unit_revision bigint NOT NULL CHECK (unit_revision >= 1),
  reason text NOT NULL CHECK (reason IN (
    'review_due','source_changed','dependency_changed','application_failed','operator_requested')),
  basis jsonb,
  basis_digest text,
  actor_principal_id uuid NOT NULL,
  actor_session_id uuid,
  observed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  FOREIGN KEY (tenant_id,workspace_id,unit_id)
    REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id),
  UNIQUE (tenant_id,workspace_id,id),
  CHECK ((NOT payload_erased AND basis IS NOT NULL AND basis_digest IS NOT NULL)
    OR (payload_erased AND basis IS NULL AND basis_digest IS NULL))
);
CREATE UNIQUE INDEX knowledge_maintenance_signal_basis_unique
  ON knowledge_maintenance_signals(tenant_id,workspace_id,unit_id,unit_revision,reason,basis_digest)
  WHERE NOT payload_erased;

CREATE TABLE knowledge_maintenance_tasks (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  signal_id uuid NOT NULL,
  revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
  state text NOT NULL DEFAULT 'pending' CHECK (state IN (
    'pending','leased','needs_review','linked','resolved','obsolete','exhausted')),
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
  failure_code text CHECK (failure_code IS NULL OR failure_code IN (
    'lease_expired','storage_unavailable','transport_unavailable','invalid_configuration',
    'internal_invariant','capacity_exceeded','needs_context')),
  next_retry_at timestamptz,
  lease_token uuid,
  lease_expires_at timestamptz,
  change_id uuid,
  run_id uuid,
  current_review jsonb,
  affected_consumers jsonb,
  terminal_evidence jsonb,
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  FOREIGN KEY (tenant_id,workspace_id,signal_id)
    REFERENCES knowledge_maintenance_signals(tenant_id,workspace_id,id),
  FOREIGN KEY (tenant_id,workspace_id,change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES knowledge_change_runs(tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,signal_id),
  CHECK ((state='leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
    OR (state<>'leased' AND lease_token IS NULL AND lease_expires_at IS NULL)),
  CHECK (state<>'exhausted' OR failure_code IS NOT NULL),
  CHECK (failure_code<>'needs_context' OR state='needs_review'),
  CHECK ((state IN ('linked','resolved') AND change_id IS NOT NULL AND run_id IS NOT NULL)
    OR (state NOT IN ('linked','resolved'))),
  CHECK (state<>'resolved' OR terminal_evidence IS NOT NULL),
  CHECK ((NOT payload_erased)
    OR (state='obsolete' AND current_review IS NULL AND affected_consumers IS NULL
      AND terminal_evidence IS NULL AND failure_code IS NULL))
);
CREATE INDEX knowledge_maintenance_tasks_claim_idx
  ON knowledge_maintenance_tasks(tenant_id,workspace_id,state,next_retry_at,id);

CREATE TABLE knowledge_maintenance_command_receipts (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  operation text NOT NULL CHECK (operation IN ('observe','begin_change')),
  request_id uuid NOT NULL,
  actor_principal_id uuid NOT NULL,
  actor_session_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  request_payload jsonb,
  result_payload jsonb,
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  FOREIGN KEY (tenant_id,workspace_id,unit_id)
    REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,operation,request_id),
  CHECK ((NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL)
    OR (payload_erased AND request_payload IS NULL AND result_payload IS NULL))
);

CREATE TABLE knowledge_maintenance_consumers (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  unit_revision bigint NOT NULL CHECK (unit_revision >= 1),
  consumer_ref text NOT NULL,
  required boolean NOT NULL,
  relation_name text NOT NULL CHECK (relation_name IN ('pipeline_knowledge_manifests')),
  row_id uuid NOT NULL,
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  FOREIGN KEY (tenant_id,workspace_id,unit_id)
    REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,unit_id,unit_revision,consumer_ref,relation_name,row_id)
);
CREATE INDEX knowledge_maintenance_consumers_unit_idx
  ON knowledge_maintenance_consumers(tenant_id,workspace_id,unit_id,unit_revision,active);

DO $policies$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'knowledge_maintenance_signals','knowledge_maintenance_tasks',
    'knowledge_maintenance_command_receipts','knowledge_maintenance_consumers'
  ] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',table_name||'_tenant',table_name,table_name,table_name);
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE %I FROM PUBLIC',table_name);
  END LOOP;
END
$policies$;

ALTER TABLE knowledge_owned_copies DROP CONSTRAINT knowledge_owned_copies_relation_check;
ALTER TABLE knowledge_owned_copies ADD CONSTRAINT knowledge_owned_copies_relation_check CHECK (
  relation_name IN (
    'knowledge_changes','knowledge_command_receipts','knowledge_lifecycle_changes',
    'knowledge_change_runs','knowledge_change_operations','knowledge_change_outputs',
    'knowledge_change_attempts','knowledge_change_inputs','knowledge_lifecycle_command_receipts',
    'pipeline_knowledge_manifests','slice_pipeline_runs','slice_pipeline_phase_attempts',
    'slice_pipeline_phase_outputs','slice_pipeline_inputs','slice_pipeline_receipts',
    'slice_results','slice_planning_inputs','slice_planning_snapshots',
    'slice_candidate_drafts','slice_candidate_reviews','native_planning_receipts',
    'knowledge_search_resources','knowledge_search_embedding_jobs','knowledge_search_vectors',
    'knowledge_maintenance_signals','knowledge_maintenance_tasks',
    'knowledge_maintenance_command_receipts','knowledge_maintenance_consumers'
  ));
