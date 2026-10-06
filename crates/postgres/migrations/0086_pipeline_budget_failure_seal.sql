-- A committed Pipeline send must be sealable even when transport cannot prove
-- delivery. The immutable reservation and append-only consumption still bind
-- that attempt; no response or advice can be released from this failure seal.
-- Preserve the complete existing context guard and change only its seal arm.
DO $pipeline_budget_seal$
DECLARE
    definition text;
    first_at integer;
    last_at integer;
    old_arm text;
    new_arm text;
BEGIN
    SELECT pg_catalog.pg_get_functiondef('public.pipeline_advice_dispatch_guard()'::pg_catalog.regprocedure)
      INTO definition;
    first_at := pg_catalog.strpos(definition, 'IF OLD.state=''sending'' AND NEW.state=''sealed'' THEN');
    IF first_at = 0 THEN
        RAISE EXCEPTION 'pipeline guard seal arm missing';
    END IF;
    last_at := pg_catalog.strpos(pg_catalog.substr(definition, first_at), 'RETURN NEW;');
    IF last_at = 0 THEN
        RAISE EXCEPTION 'pipeline guard seal arm terminator missing';
    END IF;
    old_arm := pg_catalog.substr(definition, first_at, last_at + pg_catalog.length('RETURN NEW;') - 1);
    IF pg_catalog.strpos(old_arm, 'pipeline response seal is incomplete') = 0
       OR pg_catalog.strpos(old_arm, 'NEW.pipeline_response_sha256') = 0 THEN
        RAISE EXCEPTION 'pipeline guard seal arm changed unexpectedly';
    END IF;
    new_arm := $arm$IF OLD.state='sending' AND NEW.state='sealed' THEN
            IF OLD.pipeline_response_sha256 IS NOT NULL THEN
                RAISE EXCEPTION 'pipeline response was already sealed' USING ERRCODE = '23514';
            END IF;
            IF NEW.send_certainty='sent' AND NEW.outcome='provider_response' THEN
                IF NEW.response_payload IS NULL
                   OR pg_catalog.octet_length(NEW.response_payload) NOT BETWEEN 1 AND 65536
                   OR NEW.pipeline_response_sha256 IS NULL
                   OR NEW.pipeline_response_sha256 <>
                      pg_catalog.encode(pg_catalog.sha256(NEW.response_payload),'hex') THEN
                    RAISE EXCEPTION 'pipeline response seal is incomplete' USING ERRCODE = '23514';
                END IF;
            ELSIF NEW.send_certainty='sent_unknown' AND NEW.outcome='provider_failure' THEN
                IF NEW.response_payload IS NOT NULL OR NEW.pipeline_response_sha256 IS NOT NULL
                   OR NEW.input_tokens IS NOT NULL OR NEW.output_tokens IS NOT NULL
                   OR NEW.latency_ms IS NULL OR NEW.raw_response_ref IS NOT NULL THEN
                    RAISE EXCEPTION 'pipeline failure seal is incomplete' USING ERRCODE = '23514';
                END IF;
            ELSE
                RAISE EXCEPTION 'pipeline seal outcome is invalid' USING ERRCODE = '23514';
            END IF;
            RETURN NEW;$arm$;
    definition := pg_catalog.replace(definition, old_arm, new_arm);
    EXECUTE definition;
END
$pipeline_budget_seal$;
