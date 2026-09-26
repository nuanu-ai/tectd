-- Keep the applied request/start guard and historical seal arms intact.
-- A new common receipt permits truthful complete, empty and partial transport.
DO $extend$
DECLARE definition text; first_at integer; last_at integer; old_arm text;
BEGIN
    SELECT pg_catalog.pg_get_functiondef('public.pipeline_advice_dispatch_guard()'::regprocedure) INTO definition;
    first_at := strpos(definition, 'IF OLD.state=''sending'' AND NEW.state=''sealed'' THEN');
    last_at := strpos(substr(definition, first_at), 'RETURN NEW;');
    IF first_at=0 OR last_at=0 THEN RAISE EXCEPTION 'pipeline seal arm missing'; END IF;
    old_arm := substr(definition, first_at, last_at+length('RETURN NEW;')-1);
    IF strpos(old_arm,'pipeline failure seal is incomplete')=0 THEN
        RAISE EXCEPTION 'pipeline legacy seal arm changed';
    END IF;
    definition := replace(definition, old_arm, $arm$
    IF OLD.state='sending' AND NEW.state='sealed' AND EXISTS (
        SELECT 1 FROM public.advisory_provider_observations r
        WHERE (r.tenant_id,r.workspace_id,r.opportunity_id,r.dispatch_id)=
              (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.id)) THEN
        IF OLD.pipeline_response_sha256 IS NOT NULL OR NOT EXISTS (
            SELECT 1 FROM public.advisory_provider_observations r
            WHERE (r.tenant_id,r.workspace_id,r.opportunity_id,r.dispatch_id)=
                  (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.id)
              AND r.configuration_digest=NEW.configuration_digest
              AND r.request_sha256=NEW.payload_digest
              AND r.response_payload IS NOT DISTINCT FROM NEW.response_payload
              AND r.elapsed_ms=NEW.latency_ms
              AND r.original_transport_context->>'send_certainty'=NEW.send_certainty
              AND r.original_transport_context->>'outcome'=NEW.outcome
              AND r.original_transport_context->>'raw_response_ref' IS NOT DISTINCT FROM NEW.raw_response_ref
              AND ((NEW.response_payload IS NULL AND NEW.pipeline_response_sha256 IS NULL)
                   OR (NEW.response_payload IS NOT NULL
                       AND octet_length(NEW.response_payload) BETWEEN 0 AND 65536
                       AND NEW.pipeline_response_sha256=encode(sha256(NEW.response_payload),'hex')))
              AND ((r.response_complete AND r.response_payload IS NOT NULL)
                   OR (NEW.input_tokens IS NULL AND NEW.output_tokens IS NULL))) THEN
            RAISE EXCEPTION 'pipeline seal differs from committed raw receipt' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    $arm$ || old_arm);
    EXECUTE definition;
END $extend$;
