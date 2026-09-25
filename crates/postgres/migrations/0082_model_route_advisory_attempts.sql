-- Slice 05 optional adviser fence. The selected-save caller is provenance;
-- invocation identity is the current authenticated live host session.
CREATE TABLE model_route_advisory_attempts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL,
    preparation_request_key text NOT NULL,
    invoking_session_id uuid NOT NULL,
    invoking_principal_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    work_node_id uuid NOT NULL,
    work_node_revision bigint NOT NULL,
    task_id uuid NOT NULL,
    task_revision bigint NOT NULL,
    work_digest text NOT NULL,
    catalogue_digest text,
    host_capability_evidence_ref text,
    state text NOT NULL CHECK (state IN ('no_call','send_unknown','raw_sealed','parsed')),
    no_call_reason text CHECK (no_call_reason IN (
        'workspace_disabled','session_skip','request_skip','capability_unavailable',
        'unknown_work_facts','no_eligible_routes','provider_unavailable')),
    adviser_model text,
    request_payload bytea,
    request_sha256 text,
    response_payload bytea,
    response_sha256 text,
    parsed_outcome jsonb,
    authorized_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    raw_sealed_at timestamptz,
    parsed_at timestamptz,
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,preparation_request_key),
    CONSTRAINT model_route_attempt_preparation_fk FOREIGN KEY
        (tenant_id,workspace_id,preparation_request_key)
        REFERENCES model_route_preparations (tenant_id,workspace_id,request_key),
    CONSTRAINT model_route_attempt_session_fk FOREIGN KEY
        (tenant_id,workspace_id,invoking_session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id),
    CONSTRAINT model_route_attempt_principal_fk FOREIGN KEY
        (tenant_id,invoking_principal_id) REFERENCES principals (tenant_id,id),
    CONSTRAINT model_route_attempt_shape CHECK (
        id <> '00000000-0000-0000-0000-000000000000'::uuid
        AND work_node_revision >= 1 AND task_revision >= 1
        AND work_digest ~ '^[0-9a-f]{64}$'
        AND (catalogue_digest IS NULL OR catalogue_digest ~ '^[0-9a-f]{64}$')
        AND (request_sha256 IS NULL OR request_sha256 ~ '^[0-9a-f]{64}$')
        AND (response_sha256 IS NULL OR response_sha256 ~ '^[0-9a-f]{64}$')
        AND (request_payload IS NULL OR request_sha256 =
            pg_catalog.encode(pg_catalog.sha256(request_payload),'hex'))
        AND (response_payload IS NULL OR response_sha256 =
            pg_catalog.encode(pg_catalog.sha256(response_payload),'hex'))
        AND ((state='no_call' AND no_call_reason IS NOT NULL AND adviser_model IS NULL
            AND request_payload IS NULL AND request_sha256 IS NULL
            AND response_payload IS NULL AND response_sha256 IS NULL
            AND parsed_outcome IS NULL AND raw_sealed_at IS NULL AND parsed_at IS NULL)
          OR (state='send_unknown' AND no_call_reason IS NULL AND adviser_model IS NOT NULL
            AND request_payload IS NOT NULL AND request_sha256 IS NOT NULL
            AND response_payload IS NULL AND response_sha256 IS NULL
            AND parsed_outcome IS NULL AND raw_sealed_at IS NULL AND parsed_at IS NULL)
          OR (state='raw_sealed' AND no_call_reason IS NULL AND adviser_model IS NOT NULL
            AND request_payload IS NOT NULL AND request_sha256 IS NOT NULL
            AND response_payload IS NOT NULL AND response_sha256 IS NOT NULL
            AND parsed_outcome IS NULL AND raw_sealed_at IS NOT NULL AND parsed_at IS NULL)
          OR (state='parsed' AND no_call_reason IS NULL AND adviser_model IS NOT NULL
            AND request_payload IS NOT NULL AND request_sha256 IS NOT NULL
            AND response_payload IS NOT NULL AND response_sha256 IS NOT NULL
            AND parsed_outcome IS NOT NULL AND raw_sealed_at IS NOT NULL AND parsed_at IS NOT NULL)))
);

CREATE FUNCTION model_route_attempt_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE p public.model_route_preparations%ROWTYPE;
DECLARE authorized boolean;
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
    IF (NEW.state='no_call' AND (
          (NEW.no_call_reason='provider_unavailable' AND p.prepared_payload->>'preparation'='Prepared')
          OR CASE p.prepared_payload->>'preparation'
             WHEN 'WorkspaceDisabled' THEN 'workspace_disabled'
             WHEN 'SessionSkip' THEN 'session_skip'
             WHEN 'RequestSkip' THEN 'request_skip'
             WHEN 'CapabilityUnavailable' THEN 'capability_unavailable'
             WHEN 'UnknownWorkFacts' THEN 'unknown_work_facts'
             WHEN 'NoEligibleRoutes' THEN 'no_eligible_routes'
             ELSE NULL END=NEW.no_call_reason)) IS NOT TRUE
       AND (NEW.state='send_unknown' AND p.prepared_payload->>'preparation'='Prepared') IS NOT TRUE THEN
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
CREATE TRIGGER model_route_attempt_guard_trigger
    BEFORE INSERT OR UPDATE OR DELETE ON model_route_advisory_attempts
    FOR EACH ROW EXECUTE FUNCTION model_route_attempt_guard();

ALTER TABLE model_route_advisory_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE model_route_advisory_attempts FORCE ROW LEVEL SECURITY;
CREATE POLICY model_route_attempt_tenant_scope ON model_route_advisory_attempts
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_advisory_attempts'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='model_route_advisory_attempts'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE model_route_advisory_attempts FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION model_route_attempt_guard() FROM PUBLIC;

-- A single read surface for call counts and step context. This does not grant
-- mutation authority over either audit family.
CREATE VIEW advisory_call_audit WITH (security_invoker=true) AS
SELECT o.tenant_id,o.workspace_id,o.capability,o.request_key,o.work_item_kind,
       o.work_item_id,o.phase,o.step,o.authorized_actor_id AS actor_id,
       o.session_id,o.state AS opportunity_state,o.primary_reason AS reason,
       d.id AS call_id,d.authorized_at AS call_authorized_at,
       d.sealed_at AS raw_sealed_at,
       CASE WHEN d.id IS NULL THEN 0 ELSE 1 END AS call_count
FROM advisory_opportunity o LEFT JOIN advisory_dispatch d
  ON (d.tenant_id,d.workspace_id,d.opportunity_id)=(o.tenant_id,o.workspace_id,o.id)
UNION ALL
SELECT a.tenant_id,a.workspace_id,'model_routing'::text,a.preparation_request_key,
       'work_node'::text,a.work_node_id,NULL::text,'recommendation_before_model_choice'::text,
       a.invoking_principal_id,a.invoking_session_id,a.state,a.no_call_reason,
       CASE WHEN a.state='no_call' THEN NULL::uuid ELSE a.id END,
       CASE WHEN a.state='no_call' THEN NULL::timestamptz ELSE a.authorized_at END,
       a.raw_sealed_at,CASE WHEN a.state='no_call' THEN 0 ELSE 1 END
FROM model_route_advisory_attempts a;
REVOKE ALL PRIVILEGES ON advisory_call_audit FROM PUBLIC;
