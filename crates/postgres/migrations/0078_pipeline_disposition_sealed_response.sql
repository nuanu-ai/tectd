-- Runtime INSERT cannot substitute caller advice for the saved sealed bytes.
CREATE FUNCTION pipeline_advice_disposition_require_sealed_response() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $response_guard$
DECLARE
    saved_bytes bytea;
    eligible_ids text[];
    ranking jsonb;
    advice jsonb;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline disposition tenant does not match session' USING ERRCODE='42501';
    END IF;
    IF NEW.advice_kind='no_call' THEN
        RETURN NEW;
    END IF;
    SELECT d.response_payload,context.eligible_kind_ids
      INTO saved_bytes,eligible_ids
    FROM public.advisory_dispatch AS d
    JOIN public.pipeline_advice_contexts AS context
      ON (context.tenant_id,context.workspace_id,context.opportunity_id)=
         (d.tenant_id,d.workspace_id,d.opportunity_id)
    WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
      AND d.state='sealed' AND d.send_certainty='sent'
      AND d.outcome='provider_response'
      AND d.pipeline_response_sha256=pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex')
    FOR SHARE OF d,context;
    IF saved_bytes IS NULL THEN
        RAISE EXCEPTION 'pipeline disposition lacks sealed response' USING ERRCODE='23514';
    END IF;
    BEGIN
        ranking := pg_catalog.convert_from(saved_bytes,'UTF8')::jsonb;
    EXCEPTION WHEN OTHERS THEN
        RAISE EXCEPTION 'pipeline sealed response is not typed JSON' USING ERRCODE='23514';
    END;
    advice := NEW.result_payload->'advice';
    IF pg_catalog.jsonb_typeof(ranking)<>'object'
       OR pg_catalog.jsonb_typeof(advice)<>'object'
       OR advice - 'dispatch_id' IS DISTINCT FROM ranking THEN
        RAISE EXCEPTION 'pipeline disposition differs from sealed response' USING ERRCODE='23514';
    END IF;
    IF NEW.advice_kind='abstained' THEN
        IF ranking <> '{"status":"abstained"}'::jsonb THEN
            RAISE EXCEPTION 'pipeline abstention response has wrong shape' USING ERRCODE='23514';
        END IF;
    ELSIF NEW.advice_kind='ranked' THEN
        IF ranking->>'status'<>'ranked'
           OR pg_catalog.jsonb_typeof(ranking->'ranked_ids')<>'array'
           OR ranking<>pg_catalog.jsonb_build_object(
               'status','ranked','ranked_ids',ranking->'ranked_ids')
           OR pg_catalog.jsonb_array_length(ranking->'ranked_ids')<>
              pg_catalog.cardinality(eligible_ids)
           OR ARRAY(SELECT item FROM pg_catalog.jsonb_array_elements_text(
               ranking->'ranked_ids') AS ranked(item) ORDER BY item)
              IS DISTINCT FROM ARRAY(SELECT item FROM pg_catalog.unnest(eligible_ids)
                  AS eligible(item) ORDER BY item) THEN
            RAISE EXCEPTION 'pipeline ranking differs from eligible manifest' USING ERRCODE='23514';
        END IF;
    ELSE
        RAISE EXCEPTION 'pipeline disposition advice kind invalid' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$response_guard$;

CREATE TRIGGER pipeline_advice_disposition_response
    BEFORE INSERT ON pipeline_advice_dispositions FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_disposition_require_sealed_response();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_disposition_require_sealed_response()
    FROM PUBLIC;
