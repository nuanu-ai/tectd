-- DK-2 typed Knowledge Change lifecycle and projections. Native erase follows in 0011.
ALTER TABLE durable_knowledge_capability
  ADD COLUMN mutation_contract_version text NOT NULL DEFAULT 'dk-2',
  ADD COLUMN database_lineage_id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
  ADD COLUMN qualified_system_identifier text,
  ADD COLUMN qualified_database_oid oid,
  ADD COLUMN qualified_at timestamptz,
  ADD COLUMN erasure_sequence bigint NOT NULL DEFAULT 0 CHECK (erasure_sequence >= 0),
  ADD COLUMN exported_erasure_sequence bigint NOT NULL DEFAULT 0 CHECK (exported_erasure_sequence >= 0),
  ADD COLUMN exported_manifest_digest text;

ALTER TABLE knowledge_changes
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1';

CREATE TABLE knowledge_lifecycle_changes (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  request_id uuid NOT NULL,
  initiator_principal_id uuid NOT NULL,
  initiator_session_id uuid NOT NULL,
  owner jsonb NOT NULL,
  intent text NOT NULL,
  desired_outcome text NOT NULL,
  sources jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(sources)='array'),
  source_pins jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(source_pins)='array'),
  operation_hints jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(operation_hints)='array'),
  completion jsonb NOT NULL,
  status text NOT NULL CHECK (status IN ('active','waiting_input','blocked','completed','escalated')),
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_lifecycle_changes_workspace_fk FOREIGN KEY (tenant_id,workspace_id)
    REFERENCES workspaces(tenant_id,id),
  CONSTRAINT knowledge_lifecycle_changes_session_fk FOREIGN KEY (tenant_id,workspace_id,initiator_session_id)
    REFERENCES agent_sessions(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_lifecycle_changes_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_lifecycle_changes_request_unique UNIQUE (tenant_id,workspace_id,request_id)
);

CREATE TABLE knowledge_change_runs (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  change_id uuid NOT NULL,
  revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
  definition jsonb NOT NULL,
  definition_version text NOT NULL,
  definition_digest text NOT NULL,
  registry jsonb NOT NULL,
  registry_version text NOT NULL,
  registry_digest text NOT NULL,
  delivery_mode text NOT NULL CHECK (delivery_mode IN ('whole','phasewise')),
  status text NOT NULL CHECK (status IN ('active','waiting_input','blocked','completed','escalated')),
  current_phase_id text,
  current_phase_ordinal integer CHECK (current_phase_ordinal BETWEEN 1 AND 12),
  baseline jsonb,
  branch_plan jsonb,
  ready_to_commit jsonb,
  publisher_receipt jsonb,
  effects_report jsonb,
  result jsonb,
  terminal_review_outcome text CHECK (terminal_review_outcome IS NULL OR terminal_review_outcome IN ('ready','no_change','rejected')),
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_change_runs_change_fk FOREIGN KEY (tenant_id,workspace_id,change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_runs_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_runs_one_per_change UNIQUE (tenant_id,workspace_id,change_id)
);

CREATE TABLE knowledge_change_operations (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  change_id uuid NOT NULL,
  client_label text NOT NULL,
  operation text NOT NULL CHECK (operation IN ('create','revise','revalidate','supersede','retract','erase')),
  unit_id uuid NOT NULL,
  expected_revision bigint CHECK (expected_revision IS NULL OR expected_revision >= 1),
  expected_lifecycle text CHECK (expected_lifecycle IS NULL OR expected_lifecycle IN ('active','retracted','superseded','erasure_pending','erased')),
  reason text NOT NULL,
  authority_basis text NOT NULL,
  dependency_operation_ids uuid[] NOT NULL DEFAULT '{}',
  knowledge_kind text,
  profile_ids text[],
  qualification_basis text,
  applied_event_id uuid,
  applied_revision bigint,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_change_operations_change_fk FOREIGN KEY (tenant_id,workspace_id,change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_operations_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_operations_label_unique UNIQUE (tenant_id,workspace_id,change_id,client_label)
);

CREATE TABLE knowledge_change_outputs (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  run_id uuid NOT NULL,
  phase_id text NOT NULL,
  phase_ordinal integer NOT NULL CHECK (phase_ordinal BETWEEN 1 AND 12),
  revision bigint NOT NULL CHECK (revision >= 1),
  digest text NOT NULL,
  output jsonb,
  payload_erased boolean NOT NULL DEFAULT false,
  owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_change_outputs_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES knowledge_change_runs(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_outputs_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_outputs_revision_unique UNIQUE (tenant_id,workspace_id,run_id,phase_id,revision),
  CONSTRAINT knowledge_change_outputs_payload_shape CHECK ((output IS NULL)=payload_erased)
);

CREATE TABLE knowledge_change_attempts (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  run_id uuid NOT NULL,
  phase_id text NOT NULL,
  phase_ordinal integer NOT NULL CHECK (phase_ordinal BETWEEN 1 AND 12),
  attempt bigint NOT NULL CHECK (attempt >= 1),
  outcome text NOT NULL CHECK (outcome IN ('completed','waiting_input','blocked')),
  transition text NOT NULL CHECK (transition IN ('continue','complete','block','escalate')),
  output_id uuid,
  output_digest text,
  actor_session_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_change_attempts_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES knowledge_change_runs(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_attempts_output_fk FOREIGN KEY (tenant_id,workspace_id,output_id)
    REFERENCES knowledge_change_outputs(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_attempts_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_attempts_number_unique UNIQUE (tenant_id,workspace_id,run_id,phase_id,attempt)
);

CREATE TABLE knowledge_change_output_bindings (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  run_id uuid NOT NULL,
  phase_id text NOT NULL,
  phase_ordinal integer NOT NULL CHECK (phase_ordinal BETWEEN 1 AND 12),
  output_id uuid NOT NULL,
  output_revision bigint NOT NULL CHECK (output_revision >= 1),
  stale boolean NOT NULL DEFAULT false,
  stale_reason text,
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,run_id,phase_id),
  CONSTRAINT knowledge_change_output_bindings_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES knowledge_change_runs(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_output_bindings_output_fk FOREIGN KEY (tenant_id,workspace_id,output_id)
    REFERENCES knowledge_change_outputs(tenant_id,workspace_id,id)
);

CREATE TABLE knowledge_change_inputs (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  run_id uuid NOT NULL,
  request_id uuid NOT NULL,
  sequence bigint NOT NULL CHECK (sequence >= 1),
  revisit_phase_id text NOT NULL,
  reason text NOT NULL,
  input text,
  digest text,
  payload_erased boolean NOT NULL DEFAULT false,
  actor_session_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_change_inputs_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id)
    REFERENCES knowledge_change_runs(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_inputs_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_change_inputs_request_unique UNIQUE (tenant_id,workspace_id,request_id),
  CONSTRAINT knowledge_change_inputs_sequence_unique UNIQUE (tenant_id,workspace_id,run_id,sequence),
  CONSTRAINT knowledge_change_inputs_payload_shape CHECK ((input IS NULL OR digest IS NULL)=payload_erased)
);

CREATE TABLE knowledge_lifecycle_command_receipts (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  operation text NOT NULL CHECK (operation IN ('begin','phase_complete','record_input','commit','settle_effects')),
  request_id uuid NOT NULL,
  actor_principal_id uuid NOT NULL,
  actor_session_id uuid NOT NULL,
  request_payload jsonb,
  result_payload jsonb,
  payload_erased boolean NOT NULL DEFAULT false,
  erased_change_id uuid,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,operation,request_id),
  CONSTRAINT knowledge_lifecycle_receipts_payload_shape CHECK ((request_payload IS NULL OR result_payload IS NULL)=payload_erased)
);

ALTER TABLE knowledge_unit_heads
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1',
  ADD COLUMN lifecycle text NOT NULL DEFAULT 'active' CHECK (lifecycle IN ('active','retracted','superseded','erasure_pending','erased')),
  ADD COLUMN access_scope text NOT NULL DEFAULT 'workspace_members' CHECK (access_scope IN ('workspace_members','owners_only')),
  ADD COLUMN last_validation_event_id uuid;
UPDATE knowledge_unit_heads
SET lifecycle=CASE WHEN active THEN 'active' ELSE 'retracted' END;

ALTER TABLE knowledge_revisions
  ALTER COLUMN constraint_payload DROP NOT NULL,
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1',
  ADD COLUMN document_payload jsonb,
  ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN access_scope text NOT NULL DEFAULT 'workspace_members' CHECK (access_scope IN ('workspace_members','owners_only')),
  ADD CONSTRAINT knowledge_revisions_contract_payload CHECK (
    (payload_erased AND constraint_payload IS NULL AND document_payload IS NULL)
    OR (NOT payload_erased AND contract_version='dk-1' AND constraint_payload IS NOT NULL AND document_payload IS NULL)
    OR (NOT payload_erased AND contract_version='dk-2' AND constraint_payload IS NULL AND document_payload IS NOT NULL)
  );

ALTER TABLE knowledge_publication_events
  DROP CONSTRAINT knowledge_publication_events_operation_check,
  DROP CONSTRAINT knowledge_events_change_fk,
  ALTER COLUMN change_id DROP NOT NULL,
  ADD COLUMN lifecycle_change_id uuid,
  ADD COLUMN operation_id uuid,
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1',
  ADD COLUMN event_payload jsonb,
  ADD CONSTRAINT knowledge_publication_events_operation_check CHECK (operation IN ('create','revise','revalidate','supersede','retract','erase')),
  ADD CONSTRAINT knowledge_events_legacy_change_fk FOREIGN KEY (tenant_id,workspace_id,change_id)
    REFERENCES knowledge_changes(tenant_id,workspace_id,id),
  ADD CONSTRAINT knowledge_events_lifecycle_change_fk FOREIGN KEY (tenant_id,workspace_id,lifecycle_change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  ADD CONSTRAINT knowledge_events_change_shape CHECK (
    (contract_version='dk-1' AND change_id IS NOT NULL AND lifecycle_change_id IS NULL)
    OR (contract_version='dk-2' AND change_id IS NULL AND lifecycle_change_id IS NOT NULL)
  );

CREATE TABLE knowledge_supersessions (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  predecessor_unit_id uuid NOT NULL,
  successor_unit_id uuid NOT NULL,
  event_id uuid NOT NULL,
  replacement_binding_ids uuid[] NOT NULL,
  full_replacement boolean NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_supersessions_event_fk FOREIGN KEY (tenant_id,workspace_id,event_id)
    REFERENCES knowledge_publication_events(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_supersessions_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT knowledge_supersessions_no_self CHECK (predecessor_unit_id<>successor_unit_id)
);

ALTER TABLE knowledge_bindings DROP CONSTRAINT knowledge_bindings_pkey;
ALTER TABLE knowledge_bindings DROP CONSTRAINT knowledge_bindings_shape;
ALTER TABLE knowledge_bindings
  ADD COLUMN id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
  ADD COLUMN program_id uuid,
  ADD COLUMN purpose text NOT NULL DEFAULT 'required' CHECK (purpose IN ('required','reference','procedure','proof_basis')),
  ADD COLUMN version_resolution text NOT NULL DEFAULT 'current_accepted' CHECK (version_resolution IN ('current_accepted','pinned_revision')),
  ADD COLUMN pinned_revision bigint CHECK (pinned_revision IS NULL OR pinned_revision >= 1),
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1',
  ADD PRIMARY KEY (tenant_id,workspace_id,id),
  ADD CONSTRAINT knowledge_bindings_shape CHECK (
    (binding_kind='workspace' AND program_id IS NULL AND scope_id IS NULL AND slice_id IS NULL AND phase_id IS NULL)
    OR (binding_kind='program' AND program_id IS NOT NULL AND scope_id IS NULL AND slice_id IS NULL AND phase_id IS NULL)
    OR (binding_kind='scope' AND program_id IS NULL AND scope_id IS NOT NULL AND slice_id IS NULL AND phase_id IS NULL)
    OR (binding_kind='slice' AND program_id IS NULL AND scope_id IS NOT NULL AND slice_id IS NOT NULL AND phase_id IS NULL)
    OR (binding_kind='slice_phase' AND program_id IS NULL AND scope_id IS NOT NULL AND slice_id IS NOT NULL AND phase_id IS NOT NULL)
  ),
  ADD CONSTRAINT knowledge_bindings_version_shape CHECK (
    (version_resolution='current_accepted' AND pinned_revision IS NULL)
    OR (version_resolution='pinned_revision' AND pinned_revision IS NOT NULL)
  );
ALTER TABLE knowledge_bindings DROP CONSTRAINT knowledge_bindings_binding_kind_check;
ALTER TABLE knowledge_bindings ADD CONSTRAINT knowledge_bindings_binding_kind_check
  CHECK (binding_kind IN ('workspace','program','scope','slice','slice_phase'));
CREATE UNIQUE INDEX knowledge_bindings_exact_unique ON knowledge_bindings(
  tenant_id,workspace_id,unit_id,revision,binding_kind,
  COALESCE(program_id,'00000000-0000-0000-0000-000000000000'::uuid),
  COALESCE(scope_id,'00000000-0000-0000-0000-000000000000'::uuid),
  COALESCE(slice_id,'00000000-0000-0000-0000-000000000000'::uuid),COALESCE(phase_id,''),purpose,version_resolution
);

CREATE TABLE knowledge_validation_events (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  unit_revision bigint NOT NULL CHECK (unit_revision >= 1),
  lifecycle_change_id uuid NOT NULL,
  operation_id uuid NOT NULL,
  sources jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(sources)='array'),
  source_pin_digest text NOT NULL,
  evidence_basis text NOT NULL,
  valid_until timestamptz,
  review_due_at timestamptz,
  actor_principal_id uuid NOT NULL,
  actor_session_id uuid NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_validation_events_revision_fk FOREIGN KEY (tenant_id,workspace_id,unit_id,unit_revision)
    REFERENCES knowledge_revisions(tenant_id,workspace_id,unit_id,revision),
  CONSTRAINT knowledge_validation_events_change_fk FOREIGN KEY (tenant_id,workspace_id,lifecycle_change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_validation_events_unique UNIQUE (tenant_id,workspace_id,id)
);

CREATE TABLE knowledge_lifecycle_effects (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  change_id uuid NOT NULL,
  publisher_receipt_id uuid NOT NULL,
  operation_id uuid,
  unit_id uuid,
  kind text NOT NULL CHECK (kind IN ('exact_delivery','invalidation','impact','search','visibility_closure','owned_copy_purge','backup_disposition')),
  status text NOT NULL CHECK (status IN ('not_applicable','not_configured','pending','ready','failed')),
  generation bigint NOT NULL CHECK (generation >= 0),
  owner_ref text NOT NULL,
  detail text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  CONSTRAINT knowledge_lifecycle_effects_change_fk FOREIGN KEY (tenant_id,workspace_id,change_id)
    REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
  CONSTRAINT knowledge_lifecycle_effects_unique UNIQUE (tenant_id,workspace_id,id)
);

CREATE TABLE knowledge_owned_copies (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  copy_kind text NOT NULL,
  relation_name text NOT NULL,
  row_id uuid NOT NULL,
  source_revision bigint,
  redacted boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  UNIQUE (tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id)
);

CREATE TABLE knowledge_suppression_ledger (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  change_id uuid NOT NULL,
  run_id uuid NOT NULL,
  request_id uuid NOT NULL,
  event_id uuid NOT NULL,
  erasure_sequence bigint NOT NULL CHECK (erasure_sequence >= 1),
  lifecycle text NOT NULL CHECK (lifecycle IN ('erasure_pending','erased')),
  owned_live_copies_status text NOT NULL,
  restore_safe_status text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,unit_id),
  UNIQUE (erasure_sequence)
);

CREATE TABLE knowledge_suppression_exports (
  database_lineage_id uuid NOT NULL,
  erasure_sequence bigint NOT NULL,
  manifest_digest text NOT NULL,
  exported_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (database_lineage_id,erasure_sequence,manifest_digest)
);

DO $policies$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'knowledge_lifecycle_changes','knowledge_change_runs','knowledge_change_operations',
    'knowledge_change_outputs','knowledge_change_attempts','knowledge_change_output_bindings',
    'knowledge_change_inputs','knowledge_lifecycle_command_receipts','knowledge_validation_events',
    'knowledge_lifecycle_effects','knowledge_owned_copies','knowledge_suppression_ledger',
    'knowledge_supersessions'
  ] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',table_name||'_tenant',table_name,table_name,table_name);
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE %I FROM PUBLIC',table_name);
  END LOOP;
END
$policies$;
REVOKE ALL PRIVILEGES ON TABLE knowledge_suppression_exports FROM PUBLIC;
