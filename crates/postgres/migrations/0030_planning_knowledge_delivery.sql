-- DK-4 immutable durable-knowledge delivery for Program, Scope, and Slice candidates.
CREATE TABLE planning_knowledge_manifests (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  stage text NOT NULL CHECK (stage IN ('program','scope','slice_candidates')),
  owner_id uuid NOT NULL,
  owner_revision bigint NOT NULL CHECK (owner_revision >= 1),
  input_revision bigint NOT NULL CHECK (input_revision >= 0),
  request_id uuid NOT NULL,
  program_id uuid,
  scope_id uuid,
  policy_id text NOT NULL,
  policy_version text NOT NULL,
  task_context_digest text,
  task_context jsonb,
  workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
  needs jsonb,
  selected jsonb,
  unresolved_needs jsonb,
  digest text,
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,stage,owner_id,request_id),
  CHECK ((NOT payload_erased AND task_context_digest IS NOT NULL AND task_context IS NOT NULL
      AND needs IS NOT NULL AND selected IS NOT NULL AND unresolved_needs IS NOT NULL AND digest IS NOT NULL)
    OR (payload_erased AND task_context_digest IS NULL AND task_context IS NULL
      AND needs IS NULL AND selected IS NULL AND unresolved_needs IS NULL AND digest IS NULL))
);
CREATE INDEX planning_knowledge_manifests_owner_idx
  ON planning_knowledge_manifests(tenant_id,workspace_id,stage,owner_id,created_at DESC,id DESC);

CREATE TABLE program_knowledge_refresh_receipts (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  program_id uuid NOT NULL,
  request_id uuid NOT NULL,
  request_payload jsonb,
  result_revision bigint NOT NULL CHECK (result_revision >= 2),
  manifest_id uuid,
  result_payload jsonb,
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,program_id,request_id),
  FOREIGN KEY (tenant_id,workspace_id,program_id)
    REFERENCES programs(tenant_id,workspace_id,id),
  FOREIGN KEY (tenant_id,workspace_id,manifest_id)
    REFERENCES planning_knowledge_manifests(tenant_id,workspace_id,id),
  CHECK ((payload_erased AND request_payload IS NULL AND manifest_id IS NULL AND result_payload IS NULL)
    OR (NOT payload_erased AND request_payload IS NOT NULL AND manifest_id IS NOT NULL AND result_payload IS NOT NULL))
);
CREATE INDEX program_knowledge_refresh_receipts_epoch_idx
  ON program_knowledge_refresh_receipts(tenant_id,workspace_id,created_at,program_id,request_id);

CREATE TABLE planning_knowledge_consumptions (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  manifest_id uuid NOT NULL,
  relation_name text NOT NULL CHECK (relation_name IN (
    'programs','scope_candidate_drafts','scope_candidate_reviews','native_scopes',
    'slice_candidate_drafts','slice_candidate_reviews')),
  row_id uuid NOT NULL,
  row_revision bigint NOT NULL CHECK (row_revision >= 1),
  redacted boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  FOREIGN KEY (tenant_id,workspace_id,manifest_id)
    REFERENCES planning_knowledge_manifests(tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,manifest_id,relation_name,row_id,row_revision)
);

ALTER TABLE programs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false;
ALTER TABLE native_scopes ADD COLUMN payload_erased boolean NOT NULL DEFAULT false;
ALTER TABLE scope_candidate_drafts
  ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN payload DROP NOT NULL;
ALTER TABLE scope_candidate_reviews
  ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN payload DROP NOT NULL;
ALTER TABLE scope_candidate_drafts ADD CONSTRAINT scope_candidate_drafts_erased_shape CHECK (
  (payload_erased AND payload IS NULL) OR (NOT payload_erased AND payload IS NOT NULL));
ALTER TABLE scope_candidate_reviews ADD CONSTRAINT scope_candidate_reviews_erased_shape CHECK (
  (payload_erased AND payload IS NULL) OR (NOT payload_erased AND payload IS NOT NULL));
ALTER TABLE scope_candidate_receipts
  ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN request_payload DROP NOT NULL,
  ALTER COLUMN result_payload DROP NOT NULL;
ALTER TABLE scope_candidate_receipts ADD CONSTRAINT scope_candidate_receipts_erased_shape CHECK (
  (payload_erased AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL));

DO $policies$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY['planning_knowledge_manifests','program_knowledge_refresh_receipts','planning_knowledge_consumptions'] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',table_name||'_tenant',table_name,table_name,table_name);
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE %I FROM PUBLIC',table_name);
    EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at()',table_name||'_created_at_immutable',table_name);
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
    'scope_candidate_drafts','scope_candidate_reviews','scope_candidate_receipts',
    'slice_candidate_drafts','slice_candidate_reviews','native_planning_receipts',
    'knowledge_search_resources','knowledge_search_embedding_jobs','knowledge_search_vectors',
    'knowledge_maintenance_signals','knowledge_maintenance_tasks',
    'knowledge_maintenance_command_receipts','knowledge_maintenance_consumers',
    'planning_knowledge_manifests','program_knowledge_refresh_receipts','planning_knowledge_consumptions','programs','native_scopes'
  ));

ALTER TABLE knowledge_maintenance_consumers DROP CONSTRAINT knowledge_maintenance_consumers_relation_name_check;
ALTER TABLE knowledge_maintenance_consumers ADD CONSTRAINT knowledge_maintenance_consumers_relation_name_check
  CHECK (relation_name IN ('pipeline_knowledge_manifests','planning_knowledge_manifests'));
