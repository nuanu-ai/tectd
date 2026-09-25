-- A durable no-call may result from disabled or skipped advice even when the
-- deterministic manifest still has eligible options. Empty options imply a
-- no-call, but are not required for one. Preserve every other guard clause.
DO $repair$
DECLARE
    definition text;
    old_clause constant text := 'AND pg_catalog.cardinality(context.eligible_kind_ids)=0';
BEGIN
    definition := pg_catalog.pg_get_functiondef(
        'public.pipeline_advice_disposition_guard()'::pg_catalog.regprocedure);
    IF pg_catalog.strpos(definition, old_clause)=0
       OR pg_catalog.strpos(
           pg_catalog.substr(definition, pg_catalog.strpos(definition, old_clause)
                                     + pg_catalog.length(old_clause)), old_clause)<>0 THEN
        RAISE EXCEPTION 'pipeline disposition no-call clause drifted'
            USING ERRCODE='23514';
    END IF;
    EXECUTE pg_catalog.replace(definition, old_clause, '');
END
$repair$;
