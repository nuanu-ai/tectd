-- Common receipt retention binds only exact already committed advisory families.
CREATE OR REPLACE FUNCTION advisory_provider_observation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public AS $guard$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        RAISE EXCEPTION 'Provider raw observation is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
       OR NOT EXISTS (SELECT 1 FROM public.advisory_dispatch d
            JOIN public.advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id)
            WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=(NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
              AND d.state='sending' AND d.configuration_digest=NEW.configuration_digest
              AND d.payload_digest=NEW.request_sha256
              AND pg_catalog.encode(pg_catalog.sha256(d.request_payload),'hex')=NEW.request_sha256
              AND o.work_item_id IS NOT NULL
              AND (o.capability,o.decision_point,o.work_item_kind) IN (
                ('engineering_profile','engineering.profile.before_selection','matrix_task'),
                ('scope_decomposition','scope.decomposition.before_selection','scope_candidate_set'),
                ('pipeline_recommendation','pipeline_recommendation_before_slice_open','slice_candidate_node'))) THEN
        RAISE EXCEPTION 'Provider committed dispatch mismatch' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
REVOKE ALL PRIVILEGES ON FUNCTION advisory_provider_observation_guard() FROM PUBLIC;
