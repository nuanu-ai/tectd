-- DK-2 complete owned-copy provenance and live-row redaction state.
ALTER TABLE knowledge_owned_copies
  ADD COLUMN row_revision bigint NOT NULL DEFAULT 0 CHECK (row_revision >= 0),
  ADD COLUMN row_operation text,
  ADD COLUMN row_request_id uuid;
ALTER TABLE knowledge_owned_copies DROP CONSTRAINT knowledge_owned_copies_tenant_id_workspace_id_unit_id_copy__key;
ALTER TABLE knowledge_owned_copies
  ADD CONSTRAINT knowledge_owned_copies_exact_unique
  UNIQUE NULLS NOT DISTINCT (tenant_id,workspace_id,unit_id,copy_kind,relation_name,
    row_id,row_revision,row_operation,row_request_id),
  ADD CONSTRAINT knowledge_owned_copies_relation_check CHECK (relation_name IN (
    'knowledge_changes','knowledge_command_receipts','knowledge_lifecycle_changes',
    'knowledge_change_runs','knowledge_change_operations','knowledge_change_outputs',
    'knowledge_change_attempts','knowledge_change_inputs','knowledge_lifecycle_command_receipts',
    'pipeline_knowledge_manifests','slice_pipeline_runs','slice_pipeline_phase_attempts',
    'slice_pipeline_phase_outputs','slice_pipeline_inputs','slice_pipeline_receipts',
    'slice_results','slice_planning_inputs','slice_planning_snapshots',
    'slice_candidate_drafts','slice_candidate_reviews','native_planning_receipts'
  ));
CREATE INDEX knowledge_owned_copies_unit_live_idx
  ON knowledge_owned_copies(tenant_id,workspace_id,unit_id,redacted,relation_name,row_id,row_revision);

ALTER TABLE knowledge_unit_heads ADD COLUMN payload_erased boolean NOT NULL DEFAULT false;
ALTER TABLE knowledge_revisions ALTER COLUMN source_sha256 DROP NOT NULL, ALTER COLUMN rdf_digest DROP NOT NULL;
ALTER TABLE knowledge_publication_events ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN rdf_digest DROP NOT NULL;
ALTER TABLE knowledge_validation_events ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN sources DROP NOT NULL, ALTER COLUMN source_pin_digest DROP NOT NULL,
  ALTER COLUMN evidence_basis DROP NOT NULL;

ALTER TABLE knowledge_changes ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN proposal_digest DROP NOT NULL, ALTER COLUMN semantic_diff DROP NOT NULL,
  ALTER COLUMN reason DROP NOT NULL, ALTER COLUMN authority_basis DROP NOT NULL;
ALTER TABLE knowledge_changes DROP CONSTRAINT knowledge_changes_proposal_shape;
ALTER TABLE knowledge_changes ADD CONSTRAINT knowledge_changes_proposal_shape CHECK (
  payload_erased OR ((operation='retract')=(proposal IS NULL)));
ALTER TABLE knowledge_command_receipts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN request_payload DROP NOT NULL, ALTER COLUMN result_payload DROP NOT NULL,
  ADD CONSTRAINT knowledge_command_receipts_erased_shape CHECK (
    (payload_erased AND request_payload IS NULL AND result_payload IS NULL)
    OR (NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL));

ALTER TABLE knowledge_lifecycle_changes ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN intent DROP NOT NULL, ALTER COLUMN desired_outcome DROP NOT NULL,
  ALTER COLUMN sources DROP NOT NULL, ALTER COLUMN source_pins DROP NOT NULL,
  ALTER COLUMN operation_hints DROP NOT NULL, ALTER COLUMN completion DROP NOT NULL;
ALTER TABLE knowledge_change_runs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN erased_publisher_receipt jsonb,
  ADD COLUMN erased_effects_report jsonb,
  ADD COLUMN erased_result jsonb;
ALTER TABLE knowledge_change_operations ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN client_label DROP NOT NULL, ALTER COLUMN reason DROP NOT NULL,
  ALTER COLUMN authority_basis DROP NOT NULL;
ALTER TABLE knowledge_change_outputs ALTER COLUMN digest DROP NOT NULL;
ALTER TABLE knowledge_change_attempts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false;
ALTER TABLE knowledge_change_inputs ALTER COLUMN reason DROP NOT NULL;

ALTER TABLE pipeline_knowledge_manifests ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN digest DROP NOT NULL, ALTER COLUMN semantic_digest DROP NOT NULL,
  ALTER COLUMN selected DROP NOT NULL, ALTER COLUMN unresolved_needs DROP NOT NULL;

ALTER TABLE slice_pipeline_runs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN origin_payload DROP NOT NULL,
  ALTER COLUMN qualification_reason DROP NOT NULL;
ALTER TABLE slice_pipeline_phase_attempts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN request_payload DROP NOT NULL;
ALTER TABLE slice_pipeline_phase_outputs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN body_digest DROP NOT NULL;
ALTER TABLE slice_pipeline_inputs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN input_digest DROP NOT NULL, ALTER COLUMN request_payload DROP NOT NULL;
ALTER TABLE slice_pipeline_receipts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN request_payload DROP NOT NULL, ALTER COLUMN result_payload DROP NOT NULL;

ALTER TABLE slice_results ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN summary DROP NOT NULL, ALTER COLUMN evidence DROP NOT NULL,
  ALTER COLUMN scope_impact DROP NOT NULL, ALTER COLUMN remaining_work DROP NOT NULL,
  ALTER COLUMN request_payload DROP NOT NULL;
ALTER TABLE slice_planning_inputs ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ALTER COLUMN input DROP NOT NULL;
ALTER TABLE slice_planning_snapshots ADD COLUMN payload_erased boolean NOT NULL DEFAULT false;
ALTER TABLE slice_candidate_drafts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN payload DROP NOT NULL;
ALTER TABLE slice_candidate_reviews ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN payload DROP NOT NULL;
ALTER TABLE native_planning_receipts ADD COLUMN payload_erased boolean NOT NULL DEFAULT false,
  ADD COLUMN owner_unit_ids uuid[] NOT NULL DEFAULT '{}',
  ALTER COLUMN request_payload DROP NOT NULL, ALTER COLUMN result_payload DROP NOT NULL;

-- Relaxed columns remain mandatory on every ordinary, non-erased row.
ALTER TABLE knowledge_unit_heads ADD CONSTRAINT knowledge_unit_heads_erased_shape CHECK (
  NOT payload_erased OR proposal_fingerprint='[erased]');
ALTER TABLE knowledge_revisions ADD CONSTRAINT knowledge_revisions_erased_digests CHECK (
  (payload_erased AND source_sha256 IS NULL AND rdf_digest IS NULL) OR
  (NOT payload_erased AND source_sha256 IS NOT NULL AND rdf_digest IS NOT NULL));
ALTER TABLE knowledge_publication_events ADD CONSTRAINT knowledge_publication_events_erased_shape CHECK (
  (payload_erased AND event_payload IS NULL AND rdf_digest IS NULL) OR
  (NOT payload_erased AND rdf_digest IS NOT NULL
    AND (contract_version='dk-1' OR (contract_version='dk-2' AND event_payload IS NOT NULL))));
ALTER TABLE knowledge_validation_events ADD CONSTRAINT knowledge_validation_events_erased_shape CHECK (
  (payload_erased AND sources IS NULL AND source_pin_digest IS NULL AND evidence_basis IS NULL) OR
  (NOT payload_erased AND sources IS NOT NULL AND source_pin_digest IS NOT NULL AND evidence_basis IS NOT NULL));
ALTER TABLE knowledge_changes ADD CONSTRAINT knowledge_changes_erased_shape CHECK (
  (payload_erased AND proposal_digest IS NULL AND proposal_fingerprint IS NULL AND source_sha256 IS NULL
    AND semantic_diff IS NULL AND baseline IS NULL AND proposal IS NULL AND binding_provenance IS NULL
    AND reason IS NULL AND authority_basis IS NULL AND review IS NULL AND publication_receipt IS NULL)
  OR (NOT payload_erased AND proposal_digest IS NOT NULL AND semantic_diff IS NOT NULL
    AND reason IS NOT NULL AND authority_basis IS NOT NULL));
ALTER TABLE knowledge_lifecycle_changes ADD CONSTRAINT knowledge_lifecycle_changes_erased_shape CHECK (
  (payload_erased AND intent IS NULL AND desired_outcome IS NULL AND sources IS NULL AND source_pins IS NULL
    AND operation_hints IS NULL AND completion IS NULL) OR
  (NOT payload_erased AND intent IS NOT NULL AND desired_outcome IS NOT NULL AND sources IS NOT NULL
    AND source_pins IS NOT NULL AND operation_hints IS NOT NULL AND completion IS NOT NULL));
CREATE FUNCTION tect_dk_uuid_json(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT pg_catalog.jsonb_typeof(value)='string'
    AND value#>>'{}' OPERATOR(pg_catalog.~) '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
$$;
CREATE FUNCTION tect_dk_opaque_effects_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT pg_catalog.jsonb_typeof(value)='array' AND COALESCE((SELECT pg_catalog.bool_and(
      pg_catalog.jsonb_typeof(effect)='object'
      AND effect ?& ARRAY['effect_id','kind','status','generation']
      AND effect-ARRAY['effect_id','kind','status','generation']='{}'::jsonb
      AND tect_dk_uuid_json(effect->'effect_id')
      AND effect->>'kind'=ANY(ARRAY['exact_delivery','invalidation','impact','search',
        'visibility_closure','owned_copy_purge','backup_disposition'])
      AND effect->>'status'=ANY(ARRAY['not_applicable','not_configured','pending','ready','failed'])
      AND pg_catalog.jsonb_typeof(effect->'generation')='number'
      AND effect->>'generation' OPERATOR(pg_catalog.~) '^[0-9]+$')
    FROM pg_catalog.jsonb_array_elements(
      CASE WHEN pg_catalog.jsonb_typeof(value)='array' THEN value ELSE '[]'::jsonb END) effect),true)
$$;
ALTER TABLE knowledge_change_runs ADD CONSTRAINT knowledge_change_runs_erased_shape CHECK (
  (payload_erased AND baseline IS NULL AND branch_plan IS NULL AND ready_to_commit IS NULL
    AND publisher_receipt IS NULL AND effects_report IS NULL AND result IS NULL)
  OR (NOT payload_erased AND erased_publisher_receipt IS NULL
    AND erased_effects_report IS NULL AND erased_result IS NULL)),
  ADD CONSTRAINT knowledge_change_runs_erased_effects_shape CHECK (
    erased_effects_report IS NULL OR (
      pg_catalog.jsonb_typeof(erased_effects_report)='object'
      AND erased_effects_report ?& ARRAY['publisher_receipt_id','effects','required_complete']
      AND erased_effects_report-ARRAY['publisher_receipt_id','effects','required_complete']='{}'::jsonb
      AND tect_dk_uuid_json(erased_effects_report->'publisher_receipt_id')
      AND pg_catalog.jsonb_typeof(erased_effects_report->'required_complete')='boolean'
      AND tect_dk_opaque_effects_valid(erased_effects_report->'effects'))),
  ADD CONSTRAINT knowledge_change_runs_erased_result_shape CHECK (
    erased_result IS NULL OR (
      pg_catalog.jsonb_typeof(erased_result)='object'
      AND erased_result ?& ARRAY['canonical','user_outcome','effects']
      AND erased_result-ARRAY['canonical','user_outcome','publisher_receipt_id','effects']='{}'::jsonb
      AND erased_result->>'canonical'=ANY(ARRAY['not_applied','applied','no_change','rejected'])
      AND erased_result->>'user_outcome'=ANY(ARRAY['achieved','not_achieved','partial'])
      AND (NOT erased_result ? 'publisher_receipt_id'
        OR tect_dk_uuid_json(erased_result->'publisher_receipt_id'))
      AND tect_dk_opaque_effects_valid(erased_result->'effects')));
ALTER TABLE knowledge_change_operations ADD CONSTRAINT knowledge_change_operations_erased_shape CHECK (
  (payload_erased AND client_label IS NULL AND reason IS NULL AND authority_basis IS NULL
    AND knowledge_kind IS NULL AND profile_ids IS NULL AND qualification_basis IS NULL)
  OR (NOT payload_erased AND client_label IS NOT NULL AND reason IS NOT NULL AND authority_basis IS NOT NULL));
ALTER TABLE knowledge_change_outputs DROP CONSTRAINT knowledge_change_outputs_payload_shape;
ALTER TABLE knowledge_change_outputs ADD CONSTRAINT knowledge_change_outputs_payload_shape CHECK (
  (payload_erased AND output IS NULL AND digest IS NULL) OR
  (NOT payload_erased AND output IS NOT NULL AND digest IS NOT NULL));
ALTER TABLE knowledge_change_attempts ADD CONSTRAINT knowledge_change_attempts_erased_shape CHECK (
  NOT payload_erased OR output_digest IS NULL);
ALTER TABLE knowledge_change_inputs DROP CONSTRAINT knowledge_change_inputs_payload_shape;
ALTER TABLE knowledge_change_inputs ADD CONSTRAINT knowledge_change_inputs_payload_shape CHECK (
  (payload_erased AND input IS NULL AND digest IS NULL AND reason IS NULL) OR
  (NOT payload_erased AND input IS NOT NULL AND digest IS NOT NULL AND reason IS NOT NULL));
ALTER TABLE pipeline_knowledge_manifests ADD CONSTRAINT pipeline_knowledge_manifests_erased_shape CHECK (
  (payload_erased AND digest IS NULL AND semantic_digest IS NULL AND selected IS NULL AND unresolved_needs IS NULL)
  OR (NOT payload_erased AND digest IS NOT NULL AND semantic_digest IS NOT NULL AND selected IS NOT NULL AND unresolved_needs IS NOT NULL));
ALTER TABLE slice_pipeline_runs ADD CONSTRAINT slice_pipeline_runs_erased_shape CHECK (
  (payload_erased AND origin_payload IS NULL AND origin_result IS NULL AND qualification_reason IS NULL)
  OR (NOT payload_erased AND origin_payload IS NOT NULL AND qualification_reason IS NOT NULL));
ALTER TABLE slice_pipeline_phase_attempts ADD CONSTRAINT slice_pipeline_attempts_erased_shape CHECK (
  (payload_erased AND reviewer_context IS NULL AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND request_payload IS NOT NULL));
ALTER TABLE slice_pipeline_phase_outputs ADD CONSTRAINT slice_pipeline_outputs_erased_shape CHECK (
  (payload_erased AND body_digest IS NULL) OR (NOT payload_erased AND body_digest IS NOT NULL));
ALTER TABLE slice_pipeline_inputs ADD CONSTRAINT slice_pipeline_inputs_erased_shape CHECK (
  (payload_erased AND input_digest IS NULL AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND input_digest IS NOT NULL AND request_payload IS NOT NULL));
ALTER TABLE slice_pipeline_receipts ADD CONSTRAINT slice_pipeline_receipts_erased_shape CHECK (
  (payload_erased AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL));
ALTER TABLE slice_results ADD CONSTRAINT slice_results_erased_shape CHECK (
  (payload_erased AND summary IS NULL AND evidence IS NULL AND scope_impact IS NULL AND remaining_work IS NULL
    AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND summary IS NOT NULL AND evidence IS NOT NULL AND scope_impact IS NOT NULL
    AND remaining_work IS NOT NULL AND request_payload IS NOT NULL));
ALTER TABLE slice_planning_inputs ADD CONSTRAINT slice_planning_inputs_erased_shape CHECK (
  (payload_erased AND input IS NULL) OR (NOT payload_erased AND input IS NOT NULL));
ALTER TABLE slice_candidate_drafts ADD CONSTRAINT slice_candidate_drafts_erased_shape CHECK (
  (payload_erased AND payload IS NULL) OR (NOT payload_erased AND payload IS NOT NULL));
ALTER TABLE slice_candidate_reviews ADD CONSTRAINT slice_candidate_reviews_erased_shape CHECK (
  (payload_erased AND payload IS NULL) OR (NOT payload_erased AND payload IS NOT NULL));
ALTER TABLE native_planning_receipts ADD CONSTRAINT native_planning_receipts_erased_shape CHECK (
  (payload_erased AND request_payload IS NULL AND result_payload IS NULL)
  OR (NOT payload_erased AND request_payload IS NOT NULL AND result_payload IS NOT NULL));

-- Backfill direct canonical and DK-1 semantic copies by exact relational ownership.
INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision)
SELECT pg_catalog.gen_random_uuid(),tenant_id,workspace_id,unit_id,'legacy_change','knowledge_changes',id,proposed_unit_revision
FROM knowledge_changes ON CONFLICT DO NOTHING;
INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_operation,row_request_id)
SELECT DISTINCT pg_catalog.gen_random_uuid(),r.tenant_id,r.workspace_id,c.unit_id,
  'legacy_receipt','knowledge_command_receipts',c.id,NULL::bigint,r.operation,r.request_id
FROM knowledge_command_receipts r JOIN knowledge_changes c
  ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND (
    r.request_payload->>'change_id'=c.id::text
    OR r.result_payload->'prepared'->>'id'=c.id::text
    OR r.result_payload->'approved'->>'id'=c.id::text
    OR r.result_payload->'rejected'->>'id'=c.id::text
    OR r.result_payload->'replay'->>'id'=c.id::text
    OR r.result_payload->'published'->>'change_id'=c.id::text
    OR r.result_payload->'replay'->>'change_id'=c.id::text)
ON CONFLICT DO NOTHING;
INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision)
SELECT DISTINCT pg_catalog.gen_random_uuid(),m.tenant_id,m.workspace_id,(s->>'unit_id')::uuid,
  'pipeline_manifest','pipeline_knowledge_manifests',m.id,(s->>'revision')::bigint
FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(m.selected) s
WHERE s ? 'unit_id' AND s ? 'revision' ON CONFLICT DO NOTHING;
INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision)
SELECT DISTINCT pg_catalog.gen_random_uuid(),r.tenant_id,r.workspace_id,(s->>'unit_id')::uuid,
  'pipeline_run_origin','slice_pipeline_runs',r.id,(s->>'revision')::bigint
FROM slice_pipeline_runs r
JOIN pipeline_knowledge_manifests m ON m.tenant_id=r.tenant_id AND m.workspace_id=r.workspace_id
  AND m.id=COALESCE(NULLIF(r.origin_result->'created'->'knowledge'->>'id','')::uuid,
    NULLIF(r.origin_result->'replay'->'knowledge'->>'id','')::uuid)
CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(m.selected) s
WHERE s ? 'unit_id' AND s ? 'revision' ON CONFLICT DO NOTHING;
