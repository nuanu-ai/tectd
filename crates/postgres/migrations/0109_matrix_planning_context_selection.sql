-- Historical V1 links retain a NULL tuple and their original effect digest.
-- Every new selection is bound to the immutable V2 requirements snapshot.
ALTER TABLE public.matrix_planning_selection_links
    ADD COLUMN frozen_snapshot_id uuid,
    ADD COLUMN authority_schema text,
    ADD COLUMN requirements_semantic_digest text,
    ADD CONSTRAINT matrix_planning_selection_context_shape CHECK (
        (frozen_snapshot_id IS NULL
         AND authority_schema IS NULL
         AND requirements_semantic_digest IS NULL)
        OR
        (frozen_snapshot_id IS NOT NULL
         AND frozen_snapshot_id<>'00000000-0000-0000-0000-000000000000'::uuid
         AND authority_schema IS NOT NULL
         AND authority_schema='tect.matrix-requirements/1'
         AND requirements_semantic_digest IS NOT NULL
         AND requirements_semantic_digest ~ '^[0-9a-f]{64}$')),
    ADD CONSTRAINT matrix_planning_selection_context_binding_fk
        FOREIGN KEY (tenant_id,workspace_id,task_id,task_revision,
                     frozen_snapshot_id,requirements_semantic_digest,authority_schema)
        REFERENCES public.matrix_task_requirements_bindings
            (tenant_id,workspace_id,task_id,revision,
             snapshot_id,semantic_digest,authority_schema);

-- The existing owner guard checks the private caller identity. This guard
-- independently prevents a runtime writer from presenting a V1 or stale V2
-- link even if it bypasses the application adapter.
CREATE FUNCTION public.matrix_planning_selection_require_context() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'matrix selection tenant mismatch' USING ERRCODE='42501';
    END IF;
    IF NEW.frozen_snapshot_id IS NULL THEN
        RAISE EXCEPTION 'new matrix selection requires V2 context' USING ERRCODE='23514';
    END IF;
    SELECT true INTO authorized
      FROM public.matrix_tasks t
      JOIN public.matrix_task_revisions r
        ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=
           (t.tenant_id,t.workspace_id,t.id,t.current_revision)
      JOIN public.matrix_task_requirements_bindings b
        ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)=
           (r.tenant_id,r.workspace_id,r.task_id,r.revision)
      JOIN public.matrix_verifications v
        ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision)=
           (r.tenant_id,r.workspace_id,r.task_id,r.revision)
     WHERE (t.tenant_id,t.workspace_id,t.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.task_id)
       AND r.revision=NEW.task_revision
       AND r.input_digest=NEW.input_digest
       AND r.choice_set_digest=NEW.choice_set_digest
       AND (b.snapshot_id,b.semantic_digest,b.authority_schema)=
           (NEW.frozen_snapshot_id,NEW.requirements_semantic_digest,NEW.authority_schema)
       AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND v.input_digest=NEW.input_digest
       AND v.record_digest=NEW.verification_digest
       AND (v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema)=
           (b.snapshot_id,b.semantic_digest,b.authority_schema)
       AND v.owner_principal_id=r.recorded_by_principal_id
       AND v.verifier_principal_id<>v.owner_principal_id
     FOR SHARE OF t,r,b,v;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix selection V2 context is stale' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER matrix_planning_selection_context_guard
    BEFORE INSERT ON public.matrix_planning_selection_links
    FOR EACH ROW EXECUTE FUNCTION public.matrix_planning_selection_require_context();
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_selection_require_context() FROM PUBLIC;

-- Effect writes require a V2 link and the same current source tuple. Historical
-- V1 attestations remain readable through their existing immutable rows.
CREATE FUNCTION public.matrix_planning_effect_require_context() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'matrix effect tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT true INTO authorized
      FROM public.matrix_planning_selection_links l
      JOIN public.matrix_tasks t
        ON (t.tenant_id,t.workspace_id,t.id)=
           (l.tenant_id,l.workspace_id,l.task_id)
      JOIN public.matrix_task_requirements_bindings b
        ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)=
           (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
      JOIN public.matrix_task_revisions r
        ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=
           (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
      JOIN public.matrix_verifications v
        ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision)=
           (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
     WHERE (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.candidate_set_id,NEW.caller_request_id)
       AND l.frozen_snapshot_id IS NOT NULL
       AND t.current_revision=l.task_revision
       AND r.input_digest=l.input_digest
       AND r.choice_set_digest=l.choice_set_digest
       AND (b.snapshot_id,b.semantic_digest,b.authority_schema)=
           (l.frozen_snapshot_id,l.requirements_semantic_digest,l.authority_schema)
       AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND v.input_digest=l.input_digest
       AND v.record_digest=l.verification_digest
       AND (v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema)=
           (b.snapshot_id,b.semantic_digest,b.authority_schema)
       AND v.owner_principal_id=r.recorded_by_principal_id
       AND v.verifier_principal_id<>v.owner_principal_id
     FOR SHARE OF l,t,r,b,v;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix effect V2 context is stale' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER matrix_planning_effect_context_guard
    BEFORE INSERT ON public.matrix_planning_effect_attestations
    FOR EACH ROW EXECUTE FUNCTION public.matrix_planning_effect_require_context();
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_effect_require_context() FROM PUBLIC;
