-- Preserve bounded response prefixes when a completed body cannot be read.
-- The existing immutable row and CHECKs distinguish partial received evidence
-- from complete responses and transport failures; only the Matrix guard widens.
CREATE OR REPLACE FUNCTION advisory_provider_observation_guard() RETURNS trigger
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
              AND o.capability='engineering_profile' AND o.work_item_kind='matrix_task') THEN
        RAISE EXCEPTION 'Matrix committed dispatch mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_provider_observation_guard() FROM PUBLIC;
