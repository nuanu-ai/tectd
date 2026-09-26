-- Exact outbound bytes remain request_payload. Native material is separately rehydratable.
ALTER TABLE public.model_route_advisory_attempts
    ADD COLUMN typed_request_payload jsonb,
    ADD COLUMN adapter_identity text CHECK (length(adapter_identity) BETWEEN 1 AND 256),
    ADD COLUMN response_http_status integer CHECK (response_http_status BETWEEN 100 AND 599),
    ADD COLUMN response_original_input_tokens bigint,
    ADD COLUMN response_original_output_tokens bigint,
    ADD COLUMN response_original_elapsed_ms bigint CHECK (response_original_elapsed_ms >= 0),
    ADD CONSTRAINT model_route_native_material CHECK (
        adapter_identity IS NULL OR (typed_request_payload IS NOT NULL AND request_payload IS NOT NULL));

CREATE FUNCTION public.model_route_native_observation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp AS $guard$
BEGIN
    IF TG_OP='UPDATE' AND
       (NEW.typed_request_payload,NEW.adapter_identity) IS DISTINCT FROM
       (OLD.typed_request_payload,OLD.adapter_identity) THEN
        RAISE EXCEPTION 'model-route native material is immutable' USING ERRCODE='23514';
    END IF;
    IF (TG_OP='UPDATE' AND OLD.response_payload IS NOT NULL AND
        (NEW.response_http_status,NEW.response_original_input_tokens,
         NEW.response_original_output_tokens,NEW.response_original_elapsed_ms) IS DISTINCT FROM
        (OLD.response_http_status,OLD.response_original_input_tokens,
         OLD.response_original_output_tokens,OLD.response_original_elapsed_ms))
       OR (NEW.response_payload IS NULL AND
           (NEW.response_http_status IS NOT NULL OR NEW.response_original_input_tokens IS NOT NULL
            OR NEW.response_original_output_tokens IS NOT NULL OR NEW.response_original_elapsed_ms IS NOT NULL)) THEN
        RAISE EXCEPTION 'model-route sealed observation is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;
CREATE TRIGGER model_route_native_observation_guard BEFORE INSERT OR UPDATE ON public.model_route_advisory_attempts
    FOR EACH ROW EXECUTE FUNCTION public.model_route_native_observation_guard();
REVOKE ALL PRIVILEGES ON FUNCTION public.model_route_native_observation_guard() FROM PUBLIC;
