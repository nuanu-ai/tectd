-- 0072 renamed the frozen finite IDs; correct only the new 0102 reader.
-- Preserve the already applied function's transport/accounting/ranking guards.
DO $fix$
DECLARE definition text;
BEGIN
    SELECT pg_catalog.pg_get_functiondef('public.pipeline_advice_interpretation_guard()'::pg_catalog.regprocedure)
        INTO definition;
    IF pg_catalog.strpos(definition,'context.eligible_kind_ids')=0
       OR pg_catalog.strpos(definition,'pipeline interpretation binding or ranking invalid')=0 THEN
        RAISE EXCEPTION 'pipeline interpretation guard changed unexpectedly';
    END IF;
    definition := pg_catalog.replace(definition,'context.eligible_kind_ids','context.eligible_option_ids');
    EXECUTE definition;
END $fix$;
