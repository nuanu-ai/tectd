CREATE TABLE advisory_provider_observations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    dispatch_id uuid NOT NULL,
    configuration_digest text NOT NULL,
    request_sha256 text NOT NULL,
    response_payload bytea,
    response_sha256 text,
    original_transport_outcome text NOT NULL CHECK (original_transport_outcome IN ('received','transport_failure','partial_received')),
    response_complete boolean NOT NULL,
    http_status integer CHECK (http_status BETWEEN 100 AND 599),
    original_input_tokens text,
    original_output_tokens text,
    elapsed_ms bigint NOT NULL CHECK (elapsed_ms >= 0),
    sealed_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    CHECK (response_sha256 IS NOT DISTINCT FROM
        CASE WHEN response_payload IS NULL THEN NULL ELSE pg_catalog.encode(pg_catalog.sha256(response_payload),'hex') END),
    CHECK (response_payload IS NOT NULL OR
        (http_status IS NULL AND original_input_tokens IS NULL AND original_output_tokens IS NULL)),
    CHECK ((original_transport_outcome='transport_failure')=(response_payload IS NULL)),
    CHECK (response_complete=(original_transport_outcome='received')),
    CHECK (original_input_tokens IS NULL OR original_input_tokens ~ '^[0-9]{1,20}$'),
    CHECK (original_output_tokens IS NULL OR original_output_tokens ~ '^[0-9]{1,20}$')
);
CREATE FUNCTION advisory_provider_observation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public AS $guard$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'Matrix raw observation is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
       OR NOT EXISTS (SELECT 1 FROM public.advisory_dispatch d
            JOIN public.advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id)
            WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=(NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
              AND d.state='sending' AND d.configuration_digest=NEW.configuration_digest
              AND d.payload_digest=NEW.request_sha256
              AND pg_catalog.encode(pg_catalog.sha256(d.request_payload),'hex')=NEW.request_sha256
              AND NEW.response_complete=(NEW.response_payload IS NOT NULL)
              AND o.capability='engineering_profile' AND o.work_item_kind='matrix_task') THEN
        RAISE EXCEPTION 'Matrix committed dispatch mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER advisory_provider_observation_guard_trigger BEFORE INSERT OR UPDATE OR DELETE
    ON advisory_provider_observations FOR EACH ROW EXECUTE FUNCTION advisory_provider_observation_guard();
ALTER TABLE advisory_provider_observations ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_provider_observations FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_provider_observations_tenant_scope ON advisory_provider_observations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_provider_observations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_provider_observations'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_provider_observations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_provider_observation_guard() FROM PUBLIC;
