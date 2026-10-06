-- Preserve unknown historical metadata; original transport facts seal with raw bytes.
ALTER TABLE public.scope_anti_bloat_reviews
    ADD COLUMN response_complete boolean,
    ADD COLUMN original_transport_context jsonb CHECK (original_transport_context IS NULL OR
        pg_catalog.jsonb_typeof(original_transport_context)='object'),
    ADD CONSTRAINT scope_anti_bloat_response_transport_requires_raw CHECK
        (raw_response IS NOT NULL OR
         (response_complete IS NULL AND original_transport_context IS NULL));

CREATE OR REPLACE FUNCTION public.scope_anti_bloat_observation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp AS $guard$
BEGIN
    IF (OLD.raw_response IS NOT NULL AND
        (NEW.response_http_status,NEW.response_original_input_tokens,
         NEW.response_original_output_tokens,NEW.response_original_elapsed_ms,
         NEW.response_complete,NEW.original_transport_context) IS DISTINCT FROM
        (OLD.response_http_status,OLD.response_original_input_tokens,
         OLD.response_original_output_tokens,OLD.response_original_elapsed_ms,
         OLD.response_complete,OLD.original_transport_context))
       OR (NEW.raw_response IS NULL AND
           (NEW.response_http_status IS NOT NULL OR NEW.response_original_input_tokens IS NOT NULL
            OR NEW.response_original_output_tokens IS NOT NULL OR NEW.response_original_elapsed_ms IS NOT NULL
            OR NEW.response_complete IS NOT NULL OR NEW.original_transport_context IS NOT NULL)) THEN
        RAISE EXCEPTION 'anti-bloat sealed observation is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;

