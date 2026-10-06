-- Preserve the authenticated native session and the preference decision used by
-- each new review. Historical rows have no provable session and remain NULL.
ALTER TABLE scope_anti_bloat_reviews
    ADD COLUMN origin_session_id uuid,
    ADD COLUMN session_preference text NOT NULL DEFAULT 'use_workspace'
        CHECK (session_preference IN ('use_workspace','skip')),
    ADD COLUMN request_preference text NOT NULL DEFAULT 'use_workspace'
        CHECK (request_preference IN ('use_workspace','skip')),
    ADD CONSTRAINT scope_anti_bloat_origin_session_fk FOREIGN KEY
        (tenant_id,workspace_id,origin_session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id);

ALTER TABLE scope_anti_bloat_reviews DROP CONSTRAINT scope_anti_bloat_reviews_state_check;
ALTER TABLE scope_anti_bloat_reviews ADD CONSTRAINT scope_anti_bloat_reviews_state_check CHECK
    (state IN ('disabled','skipped','session_skipped','no_eligible','prepared','sending','ranked',
              'send_unknown','provider_abstained','invalid_response','provider_unconfigured',
              'preflight_invalid_configuration','preflight_invalid_arguments',
              'preflight_input_conflict','preflight_request_too_large'));

CREATE OR REPLACE FUNCTION public.scope_anti_bloat_review_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp AS $guard$
BEGIN
    IF (NEW.tenant_id,NEW.workspace_id,NEW.review_id,NEW.candidate_set_id,
        NEW.candidate_set_revision,NEW.actor_id,NEW.input_payload,NEW.review_payload,
        NEW.eligible_ids,NEW.created_at,NEW.origin_session_id,NEW.session_preference,
        NEW.request_preference) IS DISTINCT FROM
       (OLD.tenant_id,OLD.workspace_id,OLD.review_id,OLD.candidate_set_id,
        OLD.candidate_set_revision,OLD.actor_id,OLD.input_payload,OLD.review_payload,
        OLD.eligible_ids,OLD.created_at,OLD.origin_session_id,OLD.session_preference,
        OLD.request_preference)
       OR (OLD.request_bytes IS NOT NULL AND
           (NEW.request_bytes,NEW.request_sha256,NEW.send_started_at,NEW.request_adapter_identity) IS DISTINCT FROM
           (OLD.request_bytes,OLD.request_sha256,OLD.send_started_at,OLD.request_adapter_identity))
       OR (OLD.raw_response IS NOT NULL AND
           (NEW.raw_response,NEW.response_sha256,NEW.response_sealed_at) IS DISTINCT FROM
           (OLD.raw_response,OLD.response_sha256,OLD.response_sealed_at))
       OR (OLD.ranked_ids IS NOT NULL AND
           (NEW.ranked_ids,NEW.sealed_at) IS DISTINCT FROM
           (OLD.ranked_ids,OLD.sealed_at))
       OR NOT ((OLD.state='prepared' AND NEW.state IN ('session_skipped',
                    'provider_unconfigured','preflight_invalid_configuration',
                    'preflight_invalid_arguments','preflight_input_conflict',
                    'preflight_request_too_large')
                AND OLD.request_bytes IS NULL AND NEW.request_bytes IS NULL
                AND NEW.request_sha256 IS NULL AND NEW.send_started_at IS NULL
                AND NEW.raw_response IS NULL AND NEW.ranked_ids IS NULL) OR
               (OLD.state='prepared' AND NEW.state='sending'
                AND OLD.request_bytes IS NULL AND NEW.request_bytes IS NOT NULL
                AND NEW.raw_response IS NULL AND NEW.ranked_ids IS NULL) OR
               (OLD.state='sending' AND NEW.state='sending'
                AND OLD.raw_response IS NULL AND NEW.raw_response IS NOT NULL
                AND NEW.request_bytes=OLD.request_bytes AND NEW.ranked_ids IS NULL) OR
               (OLD.state='sending' AND NEW.state='ranked'
                AND OLD.raw_response IS NOT NULL AND NEW.raw_response=OLD.raw_response
                AND NEW.ranked_ids IS NOT NULL) OR
               (OLD.state='sending' AND NEW.state='send_unknown'
                AND NEW.raw_response IS NOT DISTINCT FROM OLD.raw_response
                AND NEW.ranked_ids IS NULL) OR
               (OLD.state='sending' AND NEW.state IN ('provider_abstained','invalid_response')
                AND OLD.raw_response IS NOT NULL AND NEW.raw_response=OLD.raw_response
                AND NEW.ranked_ids IS NULL AND NEW.sealed_at IS NOT NULL
                AND EXISTS (SELECT 1 FROM public.scope_anti_bloat_budget_consumptions c WHERE
                    (c.tenant_id,c.workspace_id,c.review_id)=(OLD.tenant_id,OLD.workspace_id,OLD.review_id)
                    AND c.request_sha256=OLD.request_sha256 AND c.response_sha256=OLD.response_sha256
                    AND NOT c.transport_failed AND (NEW.state='invalid_response' OR
                        (NOT c.unknown_usage AND NOT c.exhausted_after_response)))))
    THEN
        RAISE EXCEPTION 'anti-bloat review audit is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;
