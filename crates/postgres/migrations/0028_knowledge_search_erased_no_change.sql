-- DK-3 can prove required search closure from the opaque suppression residual.
CREATE OR REPLACE FUNCTION public.tect_dk_erased_no_change_proof_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path = pg_catalog, public AS $$
  SELECT COALESCE((
    pg_catalog.jsonb_typeof(value)='object'
    AND value ?& ARRAY['completion','operations']
    AND value-ARRAY['completion','operations']='{}'::jsonb
    AND pg_catalog.jsonb_typeof(value->'completion')='object'
    AND value->'completion' ?& ARRAY['canonical_result','exact_delivery','impact_recorded','search','erasure']
    AND (value->'completion')-ARRAY['canonical_result','exact_delivery','impact_recorded','search','erasure']='{}'::jsonb
    AND pg_catalog.jsonb_typeof(value->'completion'->'canonical_result')='boolean'
    AND pg_catalog.jsonb_typeof(value->'completion'->'exact_delivery')='boolean'
    AND pg_catalog.jsonb_typeof(value->'completion'->'impact_recorded')='boolean'
    AND value->'completion'->>'search'=ANY(ARRAY['not_required','required'])
    AND value->'completion'->>'erasure'=ANY(ARRAY['owned_live_copies','restore_safe'])
    AND pg_catalog.jsonb_typeof(value->'operations')='array'
    AND pg_catalog.jsonb_array_length(value->'operations') BETWEEN 1 AND 16
    AND COALESCE((SELECT pg_catalog.bool_and(COALESCE((
      pg_catalog.jsonb_typeof(operation)='object'
      AND operation ?& ARRAY['operation_id','unit_id','expected_revision','expected_lifecycle','erasure_sequence']
      AND operation-ARRAY['operation_id','unit_id','expected_revision','expected_lifecycle','erasure_sequence']='{}'::jsonb
      AND public.tect_dk_uuid_json(operation->'operation_id')
      AND public.tect_dk_uuid_json(operation->'unit_id')
      AND pg_catalog.jsonb_typeof(operation->'expected_revision')='number'
      AND operation->>'expected_revision' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'
      AND operation->>'expected_lifecycle'='erased'
      AND pg_catalog.jsonb_typeof(operation->'erasure_sequence')='number'
      AND operation->>'erasure_sequence' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'
    ), false)) FROM pg_catalog.jsonb_array_elements(value->'operations') operation),false)
    AND (SELECT pg_catalog.count(*)=pg_catalog.count(DISTINCT operation->>'operation_id')
      FROM pg_catalog.jsonb_array_elements(value->'operations') operation)
    AND (SELECT pg_catalog.count(*)=pg_catalog.count(DISTINCT operation->>'unit_id')
      FROM pg_catalog.jsonb_array_elements(value->'operations') operation)
  ), false)
$$;
REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_erased_no_change_proof_valid(jsonb) FROM PUBLIC;
