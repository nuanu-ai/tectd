-- A single eligible pipeline is deterministic: preserve it as a durable
-- no-call and prevent dispatch even if an opportunity is transitioned later.
DO $single_option$
DECLARE
    definition text;
    old_clause text;
    new_clause text;
    function_name text;
BEGIN
    FOR function_name, old_clause, new_clause IN
        SELECT * FROM (VALUES
            ('public.pipeline_advice_manifest_require_shape()',
             'pg_catalog.cardinality(NEW.eligible_kind_ids)=0',
             'pg_catalog.cardinality(NEW.eligible_kind_ids)<2'),
            ('public.pipeline_advice_preserve_empty_no_call()',
             'pg_catalog.cardinality(context.eligible_kind_ids)=0',
             'pg_catalog.cardinality(context.eligible_kind_ids)<2'),
            ('public.pipeline_advice_dispatch_guard()',
             'pg_catalog.cardinality(context.eligible_kind_ids)>0',
             'pg_catalog.cardinality(context.eligible_kind_ids)>=2')
        ) AS replacements(function_name, old_clause, new_clause)
    LOOP
        definition := pg_catalog.pg_get_functiondef(function_name::pg_catalog.regprocedure);
        IF pg_catalog.strpos(definition, old_clause)=0
           OR pg_catalog.strpos(
               pg_catalog.substr(definition, pg_catalog.strpos(definition, old_clause)
                                           + pg_catalog.length(old_clause)), old_clause)<>0 THEN
            RAISE EXCEPTION 'pipeline single-option guard drifted: %', function_name
                USING ERRCODE='23514';
        END IF;
        EXECUTE pg_catalog.replace(definition, old_clause, new_clause);
    END LOOP;
END
$single_option$;
