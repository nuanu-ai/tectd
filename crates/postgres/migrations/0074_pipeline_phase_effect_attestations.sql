-- Verifier observation of one completed attempt. This table has no lifecycle authority.
ALTER TABLE native_slices ADD COLUMN opened_by_principal_id uuid;
ALTER TABLE native_slices ADD CONSTRAINT native_slices_opened_by_principal_fk
    FOREIGN KEY (tenant_id,opened_by_principal_id) REFERENCES principals (tenant_id,id);
CREATE FUNCTION native_slice_preserve_opener() RETURNS trigger
LANGUAGE plpgsql AS $guard$
BEGIN
    IF NEW.opened_by_principal_id IS DISTINCT FROM OLD.opened_by_principal_id THEN
        RAISE EXCEPTION 'Slice opener is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;
CREATE TRIGGER native_slice_opener_immutable
    BEFORE UPDATE OF opened_by_principal_id ON native_slices FOR EACH ROW
    EXECUTE FUNCTION native_slice_preserve_opener();
REVOKE ALL PRIVILEGES ON FUNCTION native_slice_preserve_opener() FROM PUBLIC;

-- Resolve the saved attempt actor without granting runtime table reads on hosts.
CREATE FUNCTION pipeline_phase_effect_caller(p_tenant uuid, p_workspace uuid, p_attempt uuid)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $caller$
DECLARE actor uuid;
BEGIN
    IF p_tenant IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline phase effect tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT h.principal_id INTO actor
    FROM public.slice_pipeline_phase_attempts a
    JOIN public.agent_sessions s ON (s.tenant_id,s.workspace_id,s.id)=
        (a.tenant_id,a.workspace_id,a.actor_session_id)
    JOIN public.hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
    WHERE (a.tenant_id,a.workspace_id,a.id)=(p_tenant,p_workspace,p_attempt)
      AND a.outcome='completed' AND NOT a.payload_erased;
    RETURN actor;
END
$caller$;
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_phase_effect_caller(uuid,uuid,uuid) FROM PUBLIC;

CREATE TABLE pipeline_phase_effect_attestations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    slice_id uuid NOT NULL,
    run_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    output_id uuid NOT NULL,
    plan_id text NOT NULL,
    plan_version text NOT NULL,
    plan_digest text NOT NULL CHECK (plan_digest ~ '^[0-9a-f]{64}$'),
    obligation_digest text NOT NULL CHECK (obligation_digest ~ '^[0-9a-f]{64}$'),
    validator_contracts_digest text NOT NULL CHECK (validator_contracts_digest ~ '^[0-9a-f]{64}$'),
    output_digest text NOT NULL CHECK (output_digest ~ '^[0-9a-f]{64}$'),
    effect_digest text NOT NULL CHECK (effect_digest ~ '^[0-9a-f]{64}$'),
    verifier_principal_id uuid NOT NULL,
    verifier_session_id uuid NOT NULL,
    verdict text NOT NULL CHECK (verdict IN ('pass','fail','unknown')),
    observation text,
    observation_digest text,
    summary text NOT NULL CHECK (pg_catalog.length(pg_catalog.btrim(summary)) >= 1
        AND pg_catalog.length(summary) <= 4096),
    verifier_request_id uuid NOT NULL,
    verified_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,verifier_request_id),
    FOREIGN KEY (tenant_id,workspace_id,run_id,attempt_id)
        REFERENCES slice_pipeline_phase_attempts (tenant_id,workspace_id,run_id,id),
    FOREIGN KEY (tenant_id,workspace_id,run_id,output_id)
        REFERENCES slice_pipeline_phase_outputs (tenant_id,workspace_id,run_id,id),
    FOREIGN KEY (tenant_id,workspace_id,slice_id)
        REFERENCES native_slices (tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,verifier_principal_id)
        REFERENCES principals (tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,verifier_session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id),
    CHECK ((verdict='unknown' AND observation IS NULL AND observation_digest IS NULL)
       OR (verdict IN ('pass','fail') AND observation IS NOT NULL
           AND pg_catalog.length(pg_catalog.btrim(observation)) >= 32
           AND pg_catalog.length(observation) <= 16384
           AND observation_digest ~ '^[0-9a-f]{64}$'))
);
CREATE INDEX pipeline_phase_effect_attempt_idx ON pipeline_phase_effect_attestations
    (tenant_id,workspace_id,run_id,attempt_id,verified_at DESC);

CREATE FUNCTION pipeline_phase_effect_require_verifier() RETURNS trigger
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
      AND NOT o.payload_erased AND NOT r.payload_erased AND NOT s.payload_erased
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

CREATE TRIGGER pipeline_phase_effect_active_verifier
    BEFORE INSERT ON pipeline_phase_effect_attestations FOR EACH ROW
    EXECUTE FUNCTION pipeline_phase_effect_require_verifier();
CREATE TRIGGER pipeline_phase_effect_immutable
    BEFORE UPDATE OR DELETE ON pipeline_phase_effect_attestations FOR EACH ROW
    EXECUTE FUNCTION matrix_verification_deny_mutation();
ALTER TABLE pipeline_phase_effect_attestations ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_phase_effect_attestations FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_phase_effect_tenant_scope ON pipeline_phase_effect_attestations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_phase_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_phase_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_phase_effect_attestations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_phase_effect_require_verifier() FROM PUBLIC;
