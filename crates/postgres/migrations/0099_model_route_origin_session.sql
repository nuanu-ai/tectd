-- New model-route preparations bind the immutable request to the native
-- session which captured it. Older receipts retain NULL and fail replay.
ALTER TABLE model_route_preparations
    ADD COLUMN origin_session_id uuid;

ALTER TABLE model_route_preparations
    ADD CONSTRAINT model_route_preparation_origin_session_fk
    FOREIGN KEY (tenant_id, origin_session_id)
    REFERENCES agent_sessions (tenant_id, id);

ALTER TABLE model_route_preparations
    ADD CONSTRAINT model_route_preparation_origin_payload_match
    CHECK (origin_session_id IS NULL OR
           prepared_payload->>'origin_session_id' = origin_session_id::text);

-- Retain the original one-use transition and owner checks while adding the
-- origin-session fence and a live session-skip no-call for a prepared receipt.
CREATE OR REPLACE FUNCTION model_route_attempt_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE p public.model_route_preparations%ROWTYPE;
DECLARE authorized boolean;
DECLARE live_preference text;
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'model-route attempt is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'model-route attempt tenant mismatch' USING ERRCODE='42501';
    END IF;
    IF TG_OP='UPDATE' THEN
        IF NEW IS NOT DISTINCT FROM OLD THEN RETURN NEW; END IF;
        IF (NEW.tenant_id,NEW.workspace_id,NEW.id,NEW.preparation_request_key,
            NEW.invoking_session_id,NEW.invoking_principal_id,NEW.candidate_set_id,
            NEW.work_node_id,NEW.work_node_revision,NEW.task_id,NEW.task_revision,
            NEW.work_digest,NEW.catalogue_digest,NEW.host_capability_evidence_ref,
            NEW.no_call_reason,NEW.adviser_model,NEW.request_payload,NEW.request_sha256,
            NEW.authorized_at) IS DISTINCT FROM
           (OLD.tenant_id,OLD.workspace_id,OLD.id,OLD.preparation_request_key,
            OLD.invoking_session_id,OLD.invoking_principal_id,OLD.candidate_set_id,
            OLD.work_node_id,OLD.work_node_revision,OLD.task_id,OLD.task_revision,
            OLD.work_digest,OLD.catalogue_digest,OLD.host_capability_evidence_ref,
            OLD.no_call_reason,OLD.adviser_model,OLD.request_payload,OLD.request_sha256,
            OLD.authorized_at) THEN
            RAISE EXCEPTION 'model-route attempt binding is immutable' USING ERRCODE='23514';
        END IF;
        IF NOT ((OLD.state='send_unknown' AND NEW.state='raw_sealed'
                   AND NEW.response_payload IS NOT NULL AND NEW.response_sha256 IS NOT NULL
                   AND NEW.parsed_outcome IS NULL AND NEW.raw_sealed_at IS NOT NULL
                   AND NEW.parsed_at IS NULL)
                OR (OLD.state='raw_sealed' AND NEW.state='parsed'
                   AND NEW.response_payload IS NOT DISTINCT FROM OLD.response_payload
                   AND NEW.response_sha256 IS NOT DISTINCT FROM OLD.response_sha256
                   AND NEW.raw_sealed_at IS NOT DISTINCT FROM OLD.raw_sealed_at
                   AND NEW.parsed_outcome IS NOT NULL AND NEW.parsed_at IS NOT NULL)) THEN
            RAISE EXCEPTION 'model-route attempt transition forbidden' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    SELECT * INTO p FROM public.model_route_preparations
    WHERE (tenant_id,workspace_id,request_key)=
          (NEW.tenant_id,NEW.workspace_id,NEW.preparation_request_key);
    IF NOT FOUND OR
       (NEW.candidate_set_id,NEW.work_node_id,NEW.work_node_revision,
        NEW.task_id,NEW.task_revision,NEW.work_digest,NEW.catalogue_digest,
        NEW.host_capability_evidence_ref) IS DISTINCT FROM
       (p.candidate_set_id,p.work_node_id,p.work_node_revision,
        p.task_id,p.task_revision,p.work_digest,p.catalogue_digest,
        p.host_capability_evidence_ref) THEN
        RAISE EXCEPTION 'model-route attempt preparation mismatch' USING ERRCODE='23514';
    END IF;
    IF p.origin_session_id IS NULL OR p.origin_session_id<>NEW.invoking_session_id THEN
        RAISE EXCEPTION 'model-route origin session mismatch' USING ERRCODE='42501';
    END IF;
    SELECT s.advisory_preference INTO live_preference FROM public.agent_sessions s
    WHERE (s.tenant_id,s.workspace_id,s.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.invoking_session_id)
      AND NOT s.revoked FOR SHARE;
    IF live_preference IS NULL THEN
        RAISE EXCEPTION 'model-route session unavailable' USING ERRCODE='42501';
    END IF;
    IF (NEW.state='no_call' AND (
          (NEW.no_call_reason='provider_unavailable' AND p.prepared_payload->>'preparation'='Prepared')
          OR (NEW.no_call_reason='session_skip' AND p.prepared_payload->>'preparation'='Prepared'
              AND live_preference='skip')
          OR CASE p.prepared_payload->>'preparation'
             WHEN 'WorkspaceDisabled' THEN 'workspace_disabled'
             WHEN 'SessionSkip' THEN 'session_skip'
             WHEN 'RequestSkip' THEN 'request_skip'
             WHEN 'CapabilityUnavailable' THEN 'capability_unavailable'
             WHEN 'UnknownWorkFacts' THEN 'unknown_work_facts'
             WHEN 'NoEligibleRoutes' THEN 'no_eligible_routes'
             ELSE NULL END=NEW.no_call_reason)) IS NOT TRUE
       AND (NEW.state='send_unknown' AND p.prepared_payload->>'preparation'='Prepared'
            AND live_preference='use_workspace') IS NOT TRUE THEN
        RAISE EXCEPTION 'model-route attempt is not valid for preparation'
            USING ERRCODE='23514';
    END IF;
    SELECT true INTO authorized FROM public.agent_sessions s
    JOIN public.hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
    JOIN public.principals pr ON (pr.tenant_id,pr.id)=(h.tenant_id,h.principal_id)
    JOIN public.memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)=
        (s.tenant_id,s.workspace_id,pr.id)
    WHERE (s.tenant_id,s.workspace_id,s.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.invoking_session_id)
      AND h.principal_id=NEW.invoking_principal_id
      AND NOT s.revoked AND NOT h.revoked AND pr.role='owner'
    FOR SHARE OF s,h,pr,m;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'model-route adviser requires a live owner session'
            USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $guard$;
