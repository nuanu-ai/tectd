-- Same-run explicit target/source basis reconciliation. Previous source bytes remain immutable.
ALTER TABLE knowledge_lifecycle_changes
  ADD COLUMN source_revision bigint NOT NULL DEFAULT 0,
  ADD CONSTRAINT knowledge_lifecycle_changes_source_revision_check
    CHECK (source_revision >= 0);

ALTER TABLE knowledge_change_inputs
  ADD COLUMN applied_basis_amendment jsonb;

CREATE FUNCTION tect_dk_basis_amendment_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
  SELECT pg_catalog.jsonb_typeof(value)='object'
    AND value ? 'target_updates'
    AND value-ARRAY['target_updates','source_change']='{}'::jsonb
    AND pg_catalog.jsonb_typeof(value->'target_updates')='array'
    AND COALESCE((SELECT pg_catalog.bool_and(
      pg_catalog.jsonb_typeof(target)='object'
      AND target ?& ARRAY['operation_id','previous_expected_revision',
        'previous_expected_lifecycle','replacement_guard']
      AND target-ARRAY['operation_id','previous_expected_revision',
        'previous_expected_lifecycle','replacement_guard']='{}'::jsonb
      AND pg_catalog.jsonb_typeof(target->'operation_id')='string'
      AND target->>'operation_id' OPERATOR(pg_catalog.~)
        '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND pg_catalog.jsonb_typeof(target->'previous_expected_revision')='number'
      AND target->>'previous_expected_revision' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'
      AND target->>'previous_expected_lifecycle'=ANY(
        ARRAY['active','retracted','superseded','erasure_pending','erased'])
      AND pg_catalog.jsonb_typeof(target->'replacement_guard')='object'
      AND target->'replacement_guard' ?&
        ARRAY['unit_id','revision','lifecycle','rdf_digest','unit_iri','revision_iri']
      AND (target->'replacement_guard')-
        ARRAY['unit_id','revision','lifecycle','rdf_digest','unit_iri','revision_iri']='{}'::jsonb
      AND pg_catalog.jsonb_typeof(target->'replacement_guard'->'unit_id')='string'
      AND target->'replacement_guard'->>'unit_id' OPERATOR(pg_catalog.~)
        '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
      AND pg_catalog.jsonb_typeof(target->'replacement_guard'->'revision')='number'
      AND target->'replacement_guard'->>'revision' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'
      AND target->'replacement_guard'->>'lifecycle'=ANY(
        ARRAY['active','retracted','superseded','erasure_pending','erased'])
      AND pg_catalog.jsonb_typeof(target->'replacement_guard'->'rdf_digest')='string'
      AND pg_catalog.jsonb_typeof(target->'replacement_guard'->'unit_iri')='string'
      AND pg_catalog.jsonb_typeof(target->'replacement_guard'->'revision_iri')='string')
      FROM pg_catalog.jsonb_array_elements(value->'target_updates') target),true)
    AND (NOT value ? 'source_change' OR (
      pg_catalog.jsonb_typeof(value->'source_change')='object'
      AND value->'source_change' ?& ARRAY['previous_sources','previous_pins',
        'previous_source_revision','replacement_sources','replacement_pins',
        'replacement_source_revision']
      AND (value->'source_change')-ARRAY['previous_sources','previous_pins',
        'previous_source_revision','replacement_sources','replacement_pins',
        'replacement_source_revision']='{}'::jsonb
      AND pg_catalog.jsonb_typeof(value->'source_change'->'previous_sources')='array'
      AND pg_catalog.jsonb_typeof(value->'source_change'->'previous_pins')='array'
      AND pg_catalog.jsonb_typeof(value->'source_change'->'replacement_sources')='array'
      AND pg_catalog.jsonb_typeof(value->'source_change'->'replacement_pins')='array'
      AND pg_catalog.jsonb_typeof(value->'source_change'->'previous_source_revision')='number'
      AND value->'source_change'->>'previous_source_revision' OPERATOR(pg_catalog.~) '^[0-9]+$'
      AND pg_catalog.jsonb_typeof(value->'source_change'->'replacement_source_revision')='number'
      AND value->'source_change'->>'replacement_source_revision' OPERATOR(pg_catalog.~) '^[1-9][0-9]*$'))
$$;

ALTER TABLE knowledge_change_inputs
  DROP CONSTRAINT knowledge_change_inputs_payload_shape,
  ADD CONSTRAINT knowledge_change_inputs_payload_shape CHECK (
    (payload_erased AND input IS NULL AND digest IS NULL AND reason IS NULL
      AND applied_basis_amendment IS NULL)
    OR (NOT payload_erased AND input IS NOT NULL AND digest IS NOT NULL AND reason IS NOT NULL
      AND (applied_basis_amendment IS NULL
        OR public.tect_dk_basis_amendment_valid(applied_basis_amendment))));
