-- Typed inquiry contracts and Deep Brainstorming -> Research checkpoints.
ALTER TABLE native_slices DROP CONSTRAINT native_slices_pipeline_check;
ALTER TABLE native_slices ADD CONSTRAINT native_slices_pipeline_check CHECK (pipeline IN (
  'slice.lightweight-tdd-development','slice.full-design-to-execution','slice.debug-root-cause',
  'slice.operational-preparation','slice.operational-execution','slice.research',
  'slice.deep-brainstorming','slice.research-to-durable-knowledge','slice.custom-procedure-capture',
  'slice.promote-to-durable-knowledge'
));

ALTER TABLE slice_pipeline_runs
  ADD COLUMN inquiry jsonb CHECK (inquiry IS NULL OR pg_catalog.jsonb_typeof(inquiry)='object'),
  ADD COLUMN source_checkpoint_id uuid,
  ADD COLUMN source_checkpoint_digest text,
  ADD CONSTRAINT slice_pipeline_runs_source_checkpoint_shape CHECK (
    (payload_erased AND source_checkpoint_digest IS NULL)
    OR (NOT payload_erased AND (source_checkpoint_id IS NULL) = (source_checkpoint_digest IS NULL)
      AND (source_checkpoint_digest IS NULL OR pg_catalog.btrim(source_checkpoint_digest)<>''))
  );

ALTER TABLE slice_pipeline_runs DROP CONSTRAINT slice_pipeline_runs_erased_shape;
ALTER TABLE slice_pipeline_runs ADD CONSTRAINT slice_pipeline_runs_erased_shape CHECK (
  (payload_erased AND origin_payload IS NULL AND origin_result IS NULL
    AND qualification_reason IS NULL AND inquiry IS NULL AND source_checkpoint_digest IS NULL)
  OR (NOT payload_erased AND origin_payload IS NOT NULL AND qualification_reason IS NOT NULL)
);

ALTER TABLE pipeline_knowledge_manifests
  ADD COLUMN resource_inquiry jsonb CHECK (
    resource_inquiry IS NULL OR pg_catalog.jsonb_typeof(resource_inquiry)='object'
  ),
  ADD COLUMN resource_projection_policy text CHECK (
    resource_projection_policy IS NULL OR resource_projection_policy IN (
      'program_planning_briefs','scope_planning_briefs','full_resources'
    )
  ),
  ADD CONSTRAINT pipeline_knowledge_manifest_inquiry_shape CHECK (
    (resource_inquiry IS NULL) = (resource_projection_policy IS NULL)
  );

CREATE TABLE pipeline_research_checkpoints (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  digest text,
  status text NOT NULL DEFAULT 'open' CHECK (
    status IN ('open','accepted','rejected','cancelled','superseded')
  ),
  producer_run_id uuid NOT NULL,
  producer_run_revision bigint NOT NULL CHECK (producer_run_revision>=1),
  producer_definition_version text NOT NULL,
  producer_definition_digest text NOT NULL,
  producer_phase_id text NOT NULL,
  producer_attempt_id uuid NOT NULL,
  producer_output_id uuid NOT NULL,
  producer_output_revision bigint NOT NULL CHECK (producer_output_revision>=1),
  producer_output_digest text,
  basis jsonb,
  question text,
  answer_criteria text,
  inquiry jsonb,
  reason text,
  consumer_run_id uuid,
  consumer_result_id uuid,
  consumer_terminal_output_id uuid,
  consumer_terminal_output_digest text,
  resolution_action text CHECK (
    resolution_action IS NULL OR resolution_action IN ('accept','reject','cancel')
  ),
  resolution_reason text,
  resolved_by_session_id uuid,
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  resolved_at timestamptz,
  CONSTRAINT pipeline_research_checkpoint_run_fk
    FOREIGN KEY (tenant_id,workspace_id,producer_run_id)
    REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_research_checkpoint_attempt_fk
    FOREIGN KEY (tenant_id,workspace_id,producer_run_id,producer_attempt_id)
    REFERENCES slice_pipeline_phase_attempts(tenant_id,workspace_id,run_id,id),
  CONSTRAINT pipeline_research_checkpoint_output_fk
    FOREIGN KEY (tenant_id,workspace_id,producer_run_id,producer_output_id)
    REFERENCES slice_pipeline_phase_outputs(tenant_id,workspace_id,run_id,id),
  CONSTRAINT pipeline_research_checkpoint_consumer_fk
    FOREIGN KEY (tenant_id,workspace_id,consumer_run_id)
    REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_research_checkpoint_result_fk
    FOREIGN KEY (tenant_id,workspace_id,consumer_result_id)
    REFERENCES slice_results(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_research_checkpoint_session_fk
    FOREIGN KEY (tenant_id,workspace_id,resolved_by_session_id)
    REFERENCES agent_sessions(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_research_checkpoint_unique UNIQUE (tenant_id,workspace_id,id),
  CONSTRAINT pipeline_research_checkpoint_consumer_unique
    UNIQUE (tenant_id,workspace_id,consumer_run_id),
  CONSTRAINT pipeline_research_checkpoint_erased_shape CHECK (
    (payload_erased AND digest IS NULL AND producer_output_digest IS NULL AND basis IS NULL
      AND question IS NULL AND answer_criteria IS NULL AND inquiry IS NULL AND reason IS NULL
      AND consumer_terminal_output_digest IS NULL AND resolution_reason IS NULL)
    OR
    (NOT payload_erased AND digest IS NOT NULL AND producer_output_digest IS NOT NULL
      AND basis IS NOT NULL AND question IS NOT NULL AND answer_criteria IS NOT NULL
      AND inquiry IS NOT NULL AND reason IS NOT NULL)
  ),
  CONSTRAINT pipeline_research_checkpoint_resolution_shape CHECK (
    payload_erased OR
    (status='open' AND resolution_action IS NULL AND resolution_reason IS NULL
      AND resolved_by_session_id IS NULL AND resolved_at IS NULL)
    OR
    (status<>'open' AND resolution_action IS NOT NULL AND resolution_reason IS NOT NULL
      AND resolved_by_session_id IS NOT NULL AND resolved_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX pipeline_research_checkpoint_open_producer_idx
  ON pipeline_research_checkpoints(tenant_id,workspace_id,producer_run_id)
  WHERE status='open';
CREATE INDEX pipeline_research_checkpoint_scope_idx
  ON pipeline_research_checkpoints(tenant_id,workspace_id,status,created_at,id);

ALTER TABLE slice_pipeline_runs ADD CONSTRAINT slice_pipeline_runs_source_checkpoint_fk
  FOREIGN KEY (tenant_id,workspace_id,source_checkpoint_id)
  REFERENCES pipeline_research_checkpoints(tenant_id,workspace_id,id)
  DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE native_slices
  ADD COLUMN source_checkpoint_id uuid,
  ADD COLUMN source_checkpoint_digest text,
  ADD CONSTRAINT native_slices_source_checkpoint_shape CHECK (
    (source_checkpoint_id IS NULL) = (source_checkpoint_digest IS NULL)
    AND (source_checkpoint_digest IS NULL OR pg_catalog.btrim(source_checkpoint_digest)<>'')
  ),
  ADD CONSTRAINT native_slices_source_checkpoint_fk
    FOREIGN KEY (tenant_id,workspace_id,source_checkpoint_id)
    REFERENCES pipeline_research_checkpoints(tenant_id,workspace_id,id);

ALTER TABLE slice_pipeline_inputs
  ADD COLUMN checkpoint_id uuid,
  ADD COLUMN checkpoint_digest text,
  ADD COLUMN checkpoint_result_id uuid,
  ADD CONSTRAINT slice_pipeline_inputs_checkpoint_shape CHECK (
    (payload_erased AND checkpoint_digest IS NULL)
    OR (NOT payload_erased AND (checkpoint_id IS NULL) = (checkpoint_digest IS NULL)
      AND (checkpoint_digest IS NULL OR pg_catalog.btrim(checkpoint_digest)<>''))
  ),
  ADD CONSTRAINT slice_pipeline_inputs_checkpoint_fk
    FOREIGN KEY (tenant_id,workspace_id,checkpoint_id)
    REFERENCES pipeline_research_checkpoints(tenant_id,workspace_id,id),
  ADD CONSTRAINT slice_pipeline_inputs_checkpoint_result_fk
    FOREIGN KEY (tenant_id,workspace_id,checkpoint_result_id)
    REFERENCES slice_results(tenant_id,workspace_id,id);
ALTER TABLE slice_pipeline_inputs DROP CONSTRAINT slice_pipeline_inputs_erased_shape;
ALTER TABLE slice_pipeline_inputs ADD CONSTRAINT slice_pipeline_inputs_erased_shape CHECK (
  (payload_erased AND input_digest IS NULL AND request_payload IS NULL
    AND result_payload IS NULL AND checkpoint_digest IS NULL)
  OR (NOT payload_erased AND input_digest IS NOT NULL AND request_payload IS NOT NULL)
);

CREATE TABLE pipeline_checkpoint_receipts (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  checkpoint_id uuid NOT NULL,
  request_id uuid NOT NULL,
  actor_session_id uuid NOT NULL,
  request_payload jsonb,
  result_payload jsonb,
  owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  payload_erased boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,request_id),
  CONSTRAINT pipeline_checkpoint_receipt_checkpoint_fk
    FOREIGN KEY (tenant_id,workspace_id,checkpoint_id)
    REFERENCES pipeline_research_checkpoints(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_checkpoint_receipt_session_fk
    FOREIGN KEY (tenant_id,workspace_id,actor_session_id)
    REFERENCES agent_sessions(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_checkpoint_receipt_erased_shape CHECK (
    (payload_erased AND request_payload IS NULL AND result_payload IS NULL)
    OR (NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL)
  )
);

ALTER TABLE pipeline_research_checkpoints ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_research_checkpoints FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_research_checkpoints_tenant ON pipeline_research_checkpoints
  USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_research_checkpoints'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
  WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_research_checkpoints'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
ALTER TABLE pipeline_checkpoint_receipts ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_checkpoint_receipts FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_checkpoint_receipts_tenant ON pipeline_checkpoint_receipts
  USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_checkpoint_receipts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
  WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_checkpoint_receipts'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_research_checkpoints,pipeline_checkpoint_receipts FROM PUBLIC;
CREATE TRIGGER pipeline_research_checkpoints_created_at_immutable BEFORE UPDATE
  ON pipeline_research_checkpoints FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER pipeline_checkpoint_receipts_created_at_immutable BEFORE UPDATE
  ON pipeline_checkpoint_receipts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

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
    'planning_knowledge_manifests','program_knowledge_refresh_receipts',
    'planning_knowledge_consumptions','programs','native_scopes',
    'pipeline_research_checkpoints','pipeline_checkpoint_receipts'
  )
);

ALTER TABLE pipeline_knowledge_manifests DROP CONSTRAINT pipeline_knowledge_manifests_erased_shape;
ALTER TABLE pipeline_knowledge_manifests ADD CONSTRAINT pipeline_knowledge_manifests_erased_shape CHECK (
  (payload_erased
    AND digest IS NULL AND semantic_digest IS NULL AND selected IS NULL AND unresolved_needs IS NULL
    AND definition_version IS NULL AND definition_digest IS NULL AND method_requirements IS NULL
    AND selected_resources IS NULL AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
    AND resource_semantic_digest IS NULL AND resource_inquiry IS NULL
    AND resource_projection_policy IS NULL)
  OR
  (NOT payload_erased AND digest IS NOT NULL AND semantic_digest IS NOT NULL
    AND selected IS NOT NULL AND unresolved_needs IS NOT NULL
    AND ((contract_version='dk-1' AND definition_version IS NULL AND definition_digest IS NULL
      AND method_requirements IS NULL AND selected_resources IS NULL
      AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
      AND resource_semantic_digest IS NULL AND resource_inquiry IS NULL
      AND resource_projection_policy IS NULL)
    OR (contract_version='dk-2' AND definition_version IS NOT NULL AND definition_digest IS NOT NULL
      AND method_requirements IS NOT NULL AND pg_catalog.jsonb_typeof(method_requirements)='array'
      AND selected_resources IS NOT NULL AND pg_catalog.jsonb_typeof(selected_resources)='array'
      AND resource_unresolved_needs IS NOT NULL AND pg_catalog.jsonb_typeof(resource_unresolved_needs)='array'
      AND freshness_warnings IS NOT NULL AND pg_catalog.jsonb_typeof(freshness_warnings)='array'
      AND resource_semantic_digest IS NOT NULL)))
);
