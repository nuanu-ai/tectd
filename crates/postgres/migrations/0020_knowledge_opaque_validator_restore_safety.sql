-- Preserve the frozen schema-15 constraints while making their SQL validators
-- portable through pg_dump/pg_restore sessions with a restricted search_path.
CREATE OR REPLACE FUNCTION public.tect_dk_uuid_json(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path = pg_catalog, public AS $$
  SELECT pg_catalog.jsonb_typeof(value)='string'
    AND value#>>'{}' OPERATOR(pg_catalog.~) '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
$$;

CREATE OR REPLACE FUNCTION public.tect_dk_opaque_effects_valid(value jsonb) RETURNS boolean
LANGUAGE sql IMMUTABLE PARALLEL SAFE
SET search_path = pg_catalog, public AS $$
  SELECT pg_catalog.jsonb_typeof(value)='array' AND COALESCE((SELECT pg_catalog.bool_and(
      pg_catalog.jsonb_typeof(effect)='object'
      AND effect ?& ARRAY['effect_id','kind','status','generation']
      AND effect-ARRAY['effect_id','kind','status','generation']='{}'::jsonb
      AND public.tect_dk_uuid_json(effect->'effect_id')
      AND effect->>'kind'=ANY(ARRAY['exact_delivery','invalidation','impact','search',
        'visibility_closure','owned_copy_purge','backup_disposition'])
      AND effect->>'status'=ANY(ARRAY['not_applicable','not_configured','pending','ready','failed'])
      AND pg_catalog.jsonb_typeof(effect->'generation')='number'
      AND effect->>'generation' OPERATOR(pg_catalog.~) '^[0-9]+$')
    FROM pg_catalog.jsonb_array_elements(
      CASE WHEN pg_catalog.jsonb_typeof(value)='array' THEN value ELSE '[]'::jsonb END) effect),true)
$$;
