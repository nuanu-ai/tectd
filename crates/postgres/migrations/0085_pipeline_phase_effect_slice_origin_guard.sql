-- Native slices retain their opening result and have no payload_erased column.
-- Replace the applied 0074 verifier routine without weakening its identity or
-- completed-attempt checks; the existing trigger continues to call this function.
CREATE OR REPLACE FUNCTION pipeline_phase_effect_require_verifier() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $guard$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline phase effect tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT true INTO authorized
    FROM public.slice_pipeline_phase_attempts a
    JOIN public.slice_pipeline_runs r ON (r.tenant_id,r.workspace_id,r.id)=(a.tenant_id,a.workspace_id,a.run_id)
    JOIN public.slice_pipeline_phase_outputs o ON (o.tenant_id,o.workspace_id,o.run_id,o.attempt_id)=
        (a.tenant_id,a.workspace_id,a.run_id,a.id)
    JOIN public.native_slices s ON (s.tenant_id,s.workspace_id,s.id)=(r.tenant_id,r.workspace_id,r.slice_id)
    JOIN public.pipeline_advice_dispositions d ON (d.tenant_id,d.workspace_id,d.disposition_id)=
        (s.tenant_id,s.workspace_id,(s.origin_payload->>'disposition_id')::uuid)
    JOIN public.pipeline_advice_contexts c ON (c.tenant_id,c.workspace_id,c.opportunity_id)=
        (d.tenant_id,d.workspace_id,d.opportunity_id)
    JOIN public.matrix_planning_effect_attestations me ON (me.tenant_id,me.workspace_id,me.id)=
        (c.tenant_id,c.workspace_id,c.match_effect_attestation_id)
    JOIN public.matrix_planning_selection_links l ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
        (me.tenant_id,me.workspace_id,me.candidate_set_id,me.caller_request_id)
    JOIN public.matrix_task_revisions tr ON (tr.tenant_id,tr.workspace_id,tr.task_id,tr.revision)=
        (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
    JOIN public.agent_sessions actor ON (actor.tenant_id,actor.workspace_id,actor.id)=
        (a.tenant_id,a.workspace_id,a.actor_session_id)
    JOIN public.hosts actor_host ON (actor_host.tenant_id,actor_host.id)=(actor.tenant_id,actor.host_id)
    JOIN public.agent_sessions verifier_session ON (verifier_session.tenant_id,verifier_session.workspace_id,verifier_session.id)=
        (a.tenant_id,a.workspace_id,NEW.verifier_session_id)
    JOIN public.hosts verifier_host ON (verifier_host.tenant_id,verifier_host.id)=
        (verifier_session.tenant_id,verifier_session.host_id)
    JOIN public.principals verifier ON (verifier.tenant_id,verifier.id)=
        (verifier_host.tenant_id,verifier_host.principal_id)
    JOIN public.memberships member ON (member.tenant_id,member.workspace_id,member.principal_id)=
        (a.tenant_id,a.workspace_id,verifier.id)
    WHERE (a.tenant_id,a.workspace_id,a.run_id,a.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.run_id,NEW.attempt_id)
      AND a.outcome='completed' AND a.result_payload IS NOT NULL AND NOT a.payload_erased
      AND NOT o.payload_erased AND NOT r.payload_erased AND s.origin_result IS NOT NULL
      AND (r.slice_id,o.id,o.body_digest)=(NEW.slice_id,NEW.output_id,NEW.output_digest)
      AND (r.verification_plan_id,r.verification_plan_version,r.verification_plan_digest)=
          (NEW.plan_id,NEW.plan_version,NEW.plan_digest)
      AND r.verification_plan_id=s.verification_plan_id
      AND r.verification_plan_digest=s.verification_plan_digest
      AND r.verification_plan_version=s.verification_plan_source_definition_version
      AND me.verdict='match' AND l.disposition_id=c.matrix_disposition_id
      AND verifier.id=NEW.verifier_principal_id AND verifier.role='verifier'
      AND s.opened_by_principal_id IS NOT NULL
      AND verifier.id<>actor_host.principal_id AND verifier.id<>s.opened_by_principal_id
      AND verifier.id<>d.actor_id
      AND verifier.id<>tr.recorded_by_principal_id
      AND verifier_session.id<>actor.id
      AND NOT verifier_session.revoked AND NOT verifier_host.revoked
    FOR SHARE OF a,r,o,s,d,c,me,l,tr,actor,actor_host,verifier_session,verifier_host,verifier,member;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline phase effect requires exact completed attempt and independent verifier'
            USING ERRCODE='42501';
    END IF;
    NEW.verified_at := pg_catalog.clock_timestamp();
    RETURN NEW;
END
$guard$;
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_phase_effect_require_verifier() FROM PUBLIC;
