-- Opaque control proof for a fresh Erase whose exact suppression is already complete.
CREATE FUNCTION public.tect_dk_erased_no_change_proof_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path = pg_catalog, public AS $$
  SELECT pg_catalog.jsonb_typeof(value)='object'
    AND value ?& ARRAY['completion','operations']
    AND value-ARRAY['completion','operations']='{}'::jsonb
    AND pg_catalog.jsonb_typeof(value->'completion')='object'
    AND value->'completion' ?& ARRAY['canonical_result','exact_delivery','impact_recorded','search','erasure']
    AND (value->'completion')-ARRAY['canonical_result','exact_delivery','impact_recorded','search','erasure']='{}'::jsonb
    AND pg_catalog.jsonb_typeof(value->'completion'->'canonical_result')='boolean'
    AND pg_catalog.jsonb_typeof(value->'completion'->'exact_delivery')='boolean'
    AND pg_catalog.jsonb_typeof(value->'completion'->'impact_recorded')='boolean'
    AND value->'completion'->>'search'='not_required'
    AND value->'completion'->>'erasure'=ANY(ARRAY['owned_live_copies','restore_safe'])
    AND pg_catalog.jsonb_typeof(value->'operations')='array'
    AND pg_catalog.jsonb_array_length(value->'operations') BETWEEN 1 AND 16
    AND COALESCE((SELECT pg_catalog.bool_and(
      pg_catalog.jsonb_typeof(operation)='object'
      AND operation ?& ARRAY['operation_id','unit_id','expected_revision','expected_lifecycle','erasure_sequence']
      AND operation-ARRAY['operation_id','unit_id','expected_revision','expected_lifecycle','erasure_sequence']='{}'::jsonb
      AND public.tect_dk_uuid_json(operation->'operation_id')
      AND public.tect_dk_uuid_json(operation->'unit_id')
      AND pg_catalog.jsonb_typeof(operation->'expected_revision')='number'
      AND operation->>'expected_revision' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'
      AND operation->>'expected_lifecycle'='erased'
      AND pg_catalog.jsonb_typeof(operation->'erasure_sequence')='number'
      AND operation->>'erasure_sequence' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$')
      FROM pg_catalog.jsonb_array_elements(value->'operations') operation),false)
    AND (SELECT pg_catalog.count(*)=pg_catalog.count(DISTINCT operation->>'operation_id')
      FROM pg_catalog.jsonb_array_elements(value->'operations') operation)
    AND (SELECT pg_catalog.count(*)=pg_catalog.count(DISTINCT operation->>'unit_id')
      FROM pg_catalog.jsonb_array_elements(value->'operations') operation)
$$;

REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_erased_no_change_proof_valid(jsonb) FROM PUBLIC;

ALTER TABLE knowledge_change_runs
  ADD COLUMN erased_no_change_proof jsonb,
  ADD CONSTRAINT knowledge_change_runs_erased_no_change_shape CHECK (
    erased_no_change_proof IS NULL OR (
      payload_erased
      AND public.tect_dk_erased_no_change_proof_valid(erased_no_change_proof)
      AND terminal_review_outcome='no_change'
      AND publisher_receipt IS NULL AND erased_publisher_receipt IS NULL
      AND effects_report IS NULL AND erased_effects_report IS NULL AND result IS NULL
      AND (
        (status='active' AND current_phase_id='kc-result-handoff'
          AND current_phase_ordinal=12 AND erased_result IS NULL)
        OR
        (status='completed' AND current_phase_id IS NULL AND current_phase_ordinal IS NULL
          AND erased_result IS NOT NULL
          AND erased_result->>'canonical'='no_change'
          AND erased_result->>'user_outcome'='achieved'
          AND NOT erased_result ? 'publisher_receipt_id'
          AND erased_result->'effects'='[]'::jsonb)
      )
    )
  );
