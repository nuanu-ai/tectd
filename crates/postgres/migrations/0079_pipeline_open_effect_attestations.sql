-- Observation of one explicit Slice open. No phase or completion authority.
CREATE TABLE pipeline_open_effect_attestations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    slice_id uuid NOT NULL,
    open_request_id uuid NOT NULL,
    effect_digest text NOT NULL CHECK (effect_digest ~ '^[0-9a-f]{64}$'),
    verifier_principal_id uuid NOT NULL,
    verifier_session_id uuid NOT NULL,
    verdict text NOT NULL CHECK (verdict IN ('match','reject')),
    summary text NOT NULL CHECK (pg_catalog.length(pg_catalog.btrim(summary)) >= 1
        AND pg_catalog.length(summary) <= 4096),
    verifier_request_id uuid NOT NULL,
    verified_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,verifier_request_id),
    FOREIGN KEY (tenant_id,workspace_id,slice_id)
        REFERENCES native_slices (tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,verifier_principal_id)
        REFERENCES principals (tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,verifier_session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);
CREATE INDEX pipeline_open_effect_slice_idx ON pipeline_open_effect_attestations
    (tenant_id,workspace_id,slice_id,verified_at DESC);

CREATE FUNCTION pipeline_open_effect_require_verifier() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $guard$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline open effect tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT true INTO authorized
    FROM public.native_slices AS opened
    JOIN public.pipeline_advice_dispositions AS disposition
      ON (disposition.tenant_id,disposition.workspace_id,disposition.disposition_id)=
         (opened.tenant_id,opened.workspace_id,(opened.origin_payload->>'disposition_id')::uuid)
    JOIN public.pipeline_advice_contexts AS context
      ON (context.tenant_id,context.workspace_id,context.opportunity_id)=
         (disposition.tenant_id,disposition.workspace_id,disposition.opportunity_id)
    JOIN public.matrix_planning_effect_attestations AS matrix_effect
      ON (matrix_effect.tenant_id,matrix_effect.workspace_id,matrix_effect.id)=
         (context.tenant_id,context.workspace_id,context.match_effect_attestation_id)
    JOIN public.matrix_planning_selection_links AS selection
      ON (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,selection.caller_request_id)=
         (matrix_effect.tenant_id,matrix_effect.workspace_id,matrix_effect.candidate_set_id,matrix_effect.caller_request_id)
    JOIN public.matrix_task_revisions AS task_revision
      ON (task_revision.tenant_id,task_revision.workspace_id,task_revision.task_id,task_revision.revision)=
         (selection.tenant_id,selection.workspace_id,selection.task_id,selection.task_revision)
    JOIN public.agent_sessions AS session
      ON (session.tenant_id,session.workspace_id,session.id)=
         (opened.tenant_id,opened.workspace_id,NEW.verifier_session_id)
    JOIN public.hosts AS host ON (host.tenant_id,host.id)=(session.tenant_id,session.host_id)
    JOIN public.principals AS principal
      ON (principal.tenant_id,principal.id)=(host.tenant_id,host.principal_id)
    JOIN public.memberships AS member
      ON (member.tenant_id,member.workspace_id,member.principal_id)=
         (opened.tenant_id,opened.workspace_id,principal.id)
    WHERE (opened.tenant_id,opened.workspace_id,opened.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.slice_id)
      AND opened.origin_request_id=NEW.open_request_id
      AND opened.origin_result IS NOT NULL
      AND opened.origin_payload->>'request_id'=NEW.open_request_id::text
      AND disposition.work_node_id=opened.candidate_id
      AND disposition.work_node_revision=opened.candidate_revision
      AND disposition.source_snapshot_id=context.source_snapshot_id
      AND disposition.matrix_disposition_id=context.matrix_disposition_id
      AND selection.disposition_id=context.matrix_disposition_id
      AND matrix_effect.verdict='match'
      AND matrix_effect.candidate_set_id=context.candidate_set_id
      AND disposition.actor_id<>principal.id
      AND task_revision.recorded_by_principal_id<>principal.id
      AND principal.id=NEW.verifier_principal_id AND principal.role='verifier'
      AND NOT session.revoked AND NOT host.revoked
    FOR SHARE OF opened,disposition,context,matrix_effect,selection,
                 task_revision,session,host,principal,member;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline open effect requires exact open and independent verifier'
            USING ERRCODE='42501';
    END IF;
    NEW.verified_at := pg_catalog.clock_timestamp();
    RETURN NEW;
END
$guard$;

CREATE TRIGGER pipeline_open_effect_active_verifier
    BEFORE INSERT ON pipeline_open_effect_attestations FOR EACH ROW
    EXECUTE FUNCTION pipeline_open_effect_require_verifier();
CREATE TRIGGER pipeline_open_effect_immutable
    BEFORE UPDATE OR DELETE ON pipeline_open_effect_attestations FOR EACH ROW
    EXECUTE FUNCTION matrix_verification_deny_mutation();
ALTER TABLE pipeline_open_effect_attestations ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_open_effect_attestations FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_open_effect_tenant_scope ON pipeline_open_effect_attestations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_open_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_open_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_open_effect_attestations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_open_effect_require_verifier() FROM PUBLIC;
