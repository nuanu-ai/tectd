-- Missing metadata on older seals remains unknown; never invent usage/status.
ALTER TABLE public.scope_anti_bloat_reviews
    ADD COLUMN response_http_status integer CHECK (response_http_status BETWEEN 100 AND 599),
    ADD COLUMN response_original_input_tokens bigint,
    ADD COLUMN response_original_output_tokens bigint,
    ADD COLUMN response_original_elapsed_ms bigint CHECK (response_original_elapsed_ms >= 0);

CREATE FUNCTION public.scope_anti_bloat_observation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp AS $guard$
BEGIN
    IF (OLD.raw_response IS NOT NULL AND
        (NEW.response_http_status,NEW.response_original_input_tokens,
         NEW.response_original_output_tokens,NEW.response_original_elapsed_ms) IS DISTINCT FROM
        (OLD.response_http_status,OLD.response_original_input_tokens,
         OLD.response_original_output_tokens,OLD.response_original_elapsed_ms))
       OR (NEW.raw_response IS NULL AND
           (NEW.response_http_status IS NOT NULL OR NEW.response_original_input_tokens IS NOT NULL
            OR NEW.response_original_output_tokens IS NOT NULL OR NEW.response_original_elapsed_ms IS NOT NULL)) THEN
        RAISE EXCEPTION 'anti-bloat sealed observation is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;
CREATE TRIGGER scope_anti_bloat_observation_guard BEFORE UPDATE ON public.scope_anti_bloat_reviews
    FOR EACH ROW EXECUTE FUNCTION public.scope_anti_bloat_observation_guard();
REVOKE ALL PRIVILEGES ON FUNCTION public.scope_anti_bloat_observation_guard() FROM PUBLIC;
