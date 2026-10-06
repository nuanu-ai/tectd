-- Schema/3 identifies an eligible choice by its exact pipeline and derived
-- verification plan. Historical advice remains immutable and readable.
ALTER TABLE pipeline_advice_contexts
    RENAME COLUMN eligible_kind_ids TO eligible_option_ids;

ALTER TABLE pipeline_advice_contexts
    ADD COLUMN verification_plan_bindings jsonb,
    DROP CONSTRAINT pipeline_advice_compatibility_policy_digest_check,
    ADD CONSTRAINT pipeline_advice_compatibility_policy_digest_check CHECK (
        (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
            AND compatibility_policy_digest IS NULL)
        OR (manifest_payload->>'schema' IN
                ('tect.pipeline-recommendation/2','tect.pipeline-recommendation/3')
            AND compatibility_policy_digest ~ '^[0-9a-f]{64}$'
            AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest)
    ) NOT VALID,
    DROP CONSTRAINT pipeline_advice_context_manifest_shape_check,
    ADD CONSTRAINT pipeline_advice_context_manifest_shape_check CHECK (COALESCE((
        pg_catalog.jsonb_typeof(manifest_payload) = 'object'
        AND manifest_payload->>'schema' IN
            ('tect.pipeline-recommendation/1','tect.pipeline-recommendation/2',
             'tect.pipeline-recommendation/3')
        AND manifest_payload->>'work_id' = work_node_id::text
        AND manifest_payload->>'work_revision' = work_node_revision::text
        AND manifest_payload->>'catalogue_revision' = catalogue_revision
        AND manifest_payload->>'catalogue_digest' = catalogue_digest
        AND pg_catalog.jsonb_typeof(manifest_payload->'options') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'options') =
            pg_catalog.cardinality(eligible_option_ids)
        AND pg_catalog.jsonb_typeof(manifest_payload->'mandatory_card_ids') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'mandatory_card_ids') > 0
        AND (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
             OR (manifest_payload->>'matrix_input_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'selected_candidate_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest
                 AND pg_catalog.jsonb_typeof(manifest_payload->'excluded') = 'array'))
        AND (manifest_payload->>'schema' <> 'tect.pipeline-recommendation/3'
             OR (pg_catalog.jsonb_typeof(verification_plan_bindings) = 'array'
                 AND pg_catalog.jsonb_array_length(verification_plan_bindings) =
                     pg_catalog.cardinality(eligible_option_ids)))
    ), false)) NOT VALID;

COMMENT ON COLUMN pipeline_advice_contexts.eligible_option_ids IS
    'Ordered exact pipeline-kind plus verification-plan pair IDs; never bare pipeline kinds.';
COMMENT ON COLUMN pipeline_advice_contexts.verification_plan_bindings IS
    'Immutable schema/3 projections of selected option ID and pinned plan ID/version/digest.';

-- Old PL/pgSQL bodies retain column names as text after ALTER RENAME. Change
-- only their references; the historical migration files are not rewritten.
DO $rename_option_refs$
DECLARE name text; definition text;
BEGIN
    FOREACH name IN ARRAY ARRAY[
        'pipeline_advice_context_require_current',
        'pipeline_advice_manifest_require_shape',
        'pipeline_advice_preserve_empty_no_call',
        'pipeline_advice_dispatch_guard',
        'pipeline_advice_disposition_guard',
        'pipeline_advice_disposition_require_sealed_response'
    ] LOOP
        definition := pg_catalog.pg_get_functiondef(
            ('public.' || name || '()')::pg_catalog.regprocedure);
        -- Migration 0067 removed the disposition guard's sole reference to
        -- eligible_kind_ids. Its no-call branch must still be present, but
        -- there is no column reference to rename in that function.
        IF name = 'pipeline_advice_disposition_guard' THEN
            IF pg_catalog.strpos(definition,'eligible_kind_ids') <> 0
               OR pg_catalog.strpos(definition,'eligible_option_ids') <> 0
               OR pg_catalog.strpos(definition,
                   'NEW.advice_kind=''no_call'' AND o.state=''no_call''') = 0
               OR pg_catalog.strpos(definition,
                   'AND NOT EXISTS (SELECT 1 FROM public.advisory_dispatch AS d') = 0 THEN
                RAISE EXCEPTION 'pipeline function drifted: %', name USING ERRCODE='23514';
            END IF;
            CONTINUE;
        END IF;
        IF pg_catalog.strpos(definition,'eligible_kind_ids') = 0 THEN
            RAISE EXCEPTION 'pipeline function drifted: %', name USING ERRCODE='23514';
        END IF;
        EXECUTE pg_catalog.replace(definition,'eligible_kind_ids','eligible_option_ids');
    END LOOP;
END
$rename_option_refs$;

-- Replace the former bare-kind whitelist with pair-ID syntax and uniqueness.
DO $pair_guard$
DECLARE definition text; first_part text; last_part text; marker text;
BEGIN
    definition := pg_catalog.pg_get_functiondef(
        'public.pipeline_advice_context_require_current()'::pg_catalog.regprocedure);
    marker := '    IF EXISTS (' || E'\n' ||
        '        SELECT 1 FROM pg_catalog.unnest(NEW.eligible_option_ids) AS kind';
    IF pg_catalog.strpos(definition, marker) = 0
       OR pg_catalog.strpos(definition, 'pipeline advice eligible kinds must be unique current Slice run IDs') = 0 THEN
        RAISE EXCEPTION 'pipeline eligibility guard drifted' USING ERRCODE='23514';
    END IF;
    first_part := pg_catalog.split_part(definition, marker, 1);
    last_part := pg_catalog.substr(definition,
        pg_catalog.strpos(definition, '    RETURN NEW;'));
    EXECUTE first_part ||
        '    IF EXISTS (SELECT 1 FROM pg_catalog.unnest(NEW.eligible_option_ids) AS option_id' || E'\n' ||
        '        WHERE option_id IS NULL OR option_id !~ ' ||
        quote_literal('^slice\.(lightweight-tdd-development|full-design-to-execution|debug-root-cause|operational-preparation|operational-execution|research|deep-brainstorming|custom-procedure-capture)\+verification-plan:[0-9a-f]{64}$') || ')' || E'\n' ||
        '       OR (SELECT pg_catalog.count(DISTINCT option_id)' || E'\n' ||
        '           FROM pg_catalog.unnest(NEW.eligible_option_ids) AS option_id)' || E'\n' ||
        '           <> pg_catalog.cardinality(NEW.eligible_option_ids) THEN' || E'\n' ||
        '        RAISE EXCEPTION ' || quote_literal('pipeline option IDs must be unique exact pairs') ||
        ' USING ERRCODE=' || quote_literal('23514') || ';' || E'\n' ||
        '    END IF;' || E'\n' || last_part;
END
$pair_guard$;

CREATE OR REPLACE FUNCTION pipeline_advice_manifest_require_shape() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $manifest_shape$
DECLARE opportunity_state text;
DECLARE option_ids text[];
DECLARE manifest_matches boolean;
DECLARE option jsonb;
DECLARE bindings jsonb := '[]'::jsonb;
DECLARE plan jsonb;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'pipeline advice tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT o.state INTO opportunity_state FROM public.advisory_opportunity AS o
    WHERE (o.tenant_id,o.workspace_id,o.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id) FOR SHARE;
    IF opportunity_state IS NULL OR
       (pg_catalog.cardinality(NEW.eligible_option_ids)<2 AND opportunity_state <> 'no_call') THEN
        RAISE EXCEPTION 'fewer than two options require a no-call opportunity' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE(pg_catalog.array_agg(item->>'id' ORDER BY ordinal), ARRAY[]::text[])
      INTO option_ids
    FROM pg_catalog.jsonb_array_elements(NEW.manifest_payload->'options')
         WITH ORDINALITY AS options(item, ordinal);
    IF option_ids IS DISTINCT FROM NEW.eligible_option_ids THEN
        RAISE EXCEPTION 'pipeline advice options differ from eligible IDs' USING ERRCODE='23514';
    END IF;
    IF NEW.manifest_payload->>'schema' = 'tect.pipeline-recommendation/3' THEN
        FOR option IN SELECT value FROM pg_catalog.jsonb_array_elements(NEW.manifest_payload->'options')
        LOOP
            plan := option->'verification_plan';
            IF pg_catalog.jsonb_typeof(plan) <> 'object'
               OR option->>'kind' !~ '^slice\.'
               OR option->>'id' IS DISTINCT FROM
                  (option->>'kind') || '+' || (plan->>'id')
               OR plan->>'schema' <> 'tect.pipeline-verification-plan/1'
               OR plan->>'id' IS DISTINCT FROM 'verification-plan:' || (plan->>'digest')
               OR plan->>'digest' !~ '^[0-9a-f]{64}$'
               OR plan->>'source_kind' IS DISTINCT FROM option->>'kind'
               OR plan->>'source_definition_version' IS DISTINCT FROM option->>'definition_version'
               OR plan->>'source_definition_digest' IS DISTINCT FROM option->>'definition_digest'
               OR plan->>'source_definition_version' IS NULL
               OR plan->>'source_definition_digest' !~ '^[0-9a-f]{64}$'
               OR pg_catalog.jsonb_typeof(plan->'obligations') <> 'array'
               OR pg_catalog.jsonb_array_length(plan->'obligations') = 0 THEN
                RAISE EXCEPTION 'pipeline option lacks an exact pinned verification plan'
                    USING ERRCODE='23514';
            END IF;
            bindings := bindings || pg_catalog.jsonb_build_array(
                pg_catalog.jsonb_build_object('option_id',option->>'id',
                    'plan_id',plan->>'id',
                    'plan_version',plan->>'source_definition_version',
                    'plan_digest',plan->>'digest'));
        END LOOP;
        NEW.verification_plan_bindings := bindings;
    ELSIF NEW.verification_plan_bindings IS NOT NULL THEN
        RAISE EXCEPTION 'historical pipeline advice has no plan binding' USING ERRCODE='23514';
    END IF;
    SELECT true INTO manifest_matches
    FROM public.matrix_planning_effect_attestations AS a
    JOIN public.matrix_planning_selection_links AS l
      ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
         (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
    WHERE (a.tenant_id,a.workspace_id,a.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.match_effect_attestation_id)
      AND NEW.manifest_payload->>'matrix_task_id'=l.task_id::text
      AND NEW.manifest_payload->>'matrix_task_revision'=l.task_revision::text
      AND NEW.manifest_payload->>'selected_choice_id'=l.selected_choice_id
      AND NEW.manifest_payload->>'matrix_choice_set_digest'=l.choice_set_digest
      AND NEW.manifest_payload->>'matrix_verification_digest'=l.verification_digest
    FOR SHARE OF a,l;
    IF manifest_matches IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline advice manifest does not bind the selected Matrix path'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$manifest_shape$;

ALTER TABLE advisory_dispatch ADD COLUMN pipeline_verification_plan_bindings jsonb;
ALTER TABLE pipeline_advice_dispositions
    ADD COLUMN selected_option_id text,
    ADD COLUMN verification_plan_id text,
    ADD COLUMN verification_plan_version text,
    ADD COLUMN verification_plan_digest text,
    ADD COLUMN verification_plan_source_definition_digest text,
    ADD CONSTRAINT pipeline_disposition_plan_identity_check CHECK (
        (selected_option_id IS NULL AND verification_plan_id IS NULL
         AND verification_plan_version IS NULL AND verification_plan_digest IS NULL
         AND verification_plan_source_definition_digest IS NULL)
        OR (selected_option_id IS NOT NULL
            AND verification_plan_id = 'verification-plan:' || verification_plan_digest
            AND pg_catalog.length(pg_catalog.btrim(verification_plan_version)) > 0
            AND verification_plan_digest ~ '^[0-9a-f]{64}$'
            AND verification_plan_source_definition_digest ~ '^[0-9a-f]{64}$')) NOT VALID;
ALTER TABLE native_slices
    ADD COLUMN selected_option_id text,
    ADD COLUMN verification_plan_id text,
    ADD COLUMN verification_plan_schema text,
    ADD COLUMN verification_plan_source_definition_version text,
    ADD COLUMN verification_plan_source_definition_digest text,
    ADD COLUMN verification_plan_digest text,
    ADD CONSTRAINT native_slice_plan_identity_check CHECK (
        (selected_option_id IS NULL AND verification_plan_id IS NULL
         AND verification_plan_schema IS NULL
         AND verification_plan_source_definition_version IS NULL
         AND verification_plan_source_definition_digest IS NULL
         AND verification_plan_digest IS NULL)
        OR (selected_option_id IS NOT NULL
            AND verification_plan_id = 'verification-plan:' || verification_plan_digest
            AND verification_plan_schema = 'tect.pipeline-verification-plan/1'
            AND pg_catalog.length(pg_catalog.btrim(verification_plan_source_definition_version)) > 0
            AND verification_plan_digest ~ '^[0-9a-f]{64}$'
            AND verification_plan_source_definition_digest ~ '^[0-9a-f]{64}$')) NOT VALID;

CREATE FUNCTION pipeline_advice_persist_plan_binding() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $plan_binding$
DECLARE context public.pipeline_advice_contexts%ROWTYPE;
DECLARE binding jsonb;
BEGIN
    IF TG_TABLE_NAME = 'advisory_dispatch' THEN
        IF NOT EXISTS (SELECT 1 FROM public.advisory_opportunity AS o
            WHERE (o.tenant_id,o.workspace_id,o.id)=
                  (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
              AND o.capability='pipeline_recommendation') THEN
            RETURN NEW;
        END IF;
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline plan tenant does not match session' USING ERRCODE='42501';
    END IF;
    IF TG_TABLE_NAME = 'advisory_dispatch' THEN
        SELECT * INTO context FROM public.pipeline_advice_contexts AS c
        WHERE (c.tenant_id,c.workspace_id,c.opportunity_id)=
              (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id) FOR SHARE;
        IF NOT FOUND OR context.manifest_payload->>'schema' <> 'tect.pipeline-recommendation/3' THEN
            RAISE EXCEPTION 'dispatch requires schema/3 plan bindings' USING ERRCODE='23514';
        END IF;
        IF TG_OP = 'UPDATE' THEN
            IF NEW.pipeline_verification_plan_bindings IS DISTINCT FROM
               OLD.pipeline_verification_plan_bindings THEN
                RAISE EXCEPTION 'dispatch plan binding is immutable' USING ERRCODE='23514';
            END IF;
        ELSE
            NEW.pipeline_verification_plan_bindings := context.verification_plan_bindings;
        END IF;
        RETURN NEW;
    END IF;
    IF TG_TABLE_NAME = 'pipeline_advice_dispositions' THEN
        SELECT * INTO context FROM public.pipeline_advice_contexts AS c
        WHERE (c.tenant_id,c.workspace_id,c.opportunity_id)=
              (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id) FOR SHARE;
        IF NOT FOUND OR context.manifest_payload->>'schema' <> 'tect.pipeline-recommendation/3' THEN
            RAISE EXCEPTION 'disposition requires schema/3 plan bindings' USING ERRCODE='23514';
        END IF;
        NEW.selected_option_id := NEW.result_payload->>'selected_option_id';
        IF NEW.selected_option_id IS NULL THEN
            IF NEW.result_payload->>'selected_kind' IS NOT NULL THEN
                RAISE EXCEPTION 'kind-only disposition is forbidden' USING ERRCODE='23514';
            END IF;
            RETURN NEW;
        END IF;
        SELECT value INTO binding FROM pg_catalog.jsonb_array_elements(
            context.verification_plan_bindings) AS item(value)
        WHERE value->>'option_id'=NEW.selected_option_id;
        IF binding IS NULL OR NEW.result_payload->>'selected_kind' IS DISTINCT FROM
            pg_catalog.split_part(NEW.selected_option_id,'+',1) THEN
            RAISE EXCEPTION 'disposition selected stale or unknown option' USING ERRCODE='23514';
        END IF;
        NEW.verification_plan_id := binding->>'plan_id';
        NEW.verification_plan_version := binding->>'plan_version';
        NEW.verification_plan_digest := binding->>'plan_digest';
        NEW.verification_plan_source_definition_digest :=
            (SELECT item->>'definition_digest'
             FROM pg_catalog.jsonb_array_elements(context.manifest_payload->'options') AS options(item)
             WHERE item->>'id'=NEW.selected_option_id);
        RETURN NEW;
    END IF;
    RETURN NEW;
END
$plan_binding$;

CREATE TRIGGER z_pipeline_dispatch_plan_binding
    BEFORE INSERT OR UPDATE ON advisory_dispatch FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_persist_plan_binding();
CREATE TRIGGER z_pipeline_disposition_plan_binding
    BEFORE INSERT ON pipeline_advice_dispositions FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_persist_plan_binding();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_persist_plan_binding() FROM PUBLIC;

-- The caller-owned open carries only the selected plan identity. The ordinary
-- Slice and pipeline run phase/verdict guards remain the sole execution path.
CREATE FUNCTION pipeline_slice_open_plan_binding() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $open_plan$
DECLARE disposition public.pipeline_advice_dispositions%ROWTYPE;
BEGIN
    IF NEW.origin_payload->>'disposition_id' IS NULL THEN
        IF NEW.selected_option_id IS NOT NULL OR NEW.verification_plan_id IS NOT NULL
           OR NEW.verification_plan_schema IS NOT NULL
           OR NEW.verification_plan_source_definition_version IS NOT NULL
           OR NEW.verification_plan_source_definition_digest IS NOT NULL
           OR NEW.verification_plan_digest IS NOT NULL THEN
            RAISE EXCEPTION 'unadvised open cannot set a plan' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline open tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT * INTO disposition FROM public.pipeline_advice_dispositions AS d
    WHERE (d.tenant_id,d.workspace_id,d.disposition_id)=
          (NEW.tenant_id,NEW.workspace_id,(NEW.origin_payload->>'disposition_id')::uuid)
    FOR SHARE;
    IF NOT FOUND OR disposition.selected_option_id IS NULL
       OR disposition.work_node_id <> NEW.candidate_id
       OR disposition.work_node_revision <> NEW.candidate_revision
       OR pg_catalog.split_part(disposition.selected_option_id,'+',1) <> NEW.pipeline THEN
        RAISE EXCEPTION 'pipeline open requires exact selected option' USING ERRCODE='23514';
    END IF;
    NEW.selected_option_id := disposition.selected_option_id;
    NEW.verification_plan_id := disposition.verification_plan_id;
    NEW.verification_plan_schema := 'tect.pipeline-verification-plan/1';
    NEW.verification_plan_source_definition_version := disposition.verification_plan_version;
    NEW.verification_plan_source_definition_digest :=
        disposition.verification_plan_source_definition_digest;
    NEW.verification_plan_digest := disposition.verification_plan_digest;
    RETURN NEW;
END
$open_plan$;
CREATE TRIGGER z_pipeline_slice_open_plan_binding
    BEFORE INSERT ON native_slices FOR EACH ROW
    EXECUTE FUNCTION pipeline_slice_open_plan_binding();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_slice_open_plan_binding() FROM PUBLIC;
