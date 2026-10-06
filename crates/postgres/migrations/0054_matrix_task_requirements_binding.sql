-- A context-aware Matrix revision carries one immutable link to the full
-- effective requirements snapshot frozen in the same transaction.
CREATE TABLE public.matrix_task_requirements_bindings (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    task_id uuid NOT NULL,
    revision bigint NOT NULL,
    request_id uuid NOT NULL,
    requirements_locator jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(requirements_locator)='object'),
    snapshot_id uuid NOT NULL,
    semantic_digest text NOT NULL CHECK (semantic_digest ~ '^[0-9a-f]{64}$'),
    authority_schema text NOT NULL CHECK (authority_schema='tect.matrix-requirements/1'),
    original_request_digest text NOT NULL CHECK (original_request_digest ~ '^[0-9a-f]{64}$'),
    PRIMARY KEY (tenant_id,workspace_id,task_id,revision),
    UNIQUE (tenant_id,workspace_id,request_id),
    FOREIGN KEY (tenant_id,workspace_id,task_id,revision)
        REFERENCES public.matrix_task_revisions(tenant_id,workspace_id,task_id,revision),
    FOREIGN KEY (tenant_id,workspace_id,snapshot_id)
        REFERENCES public.matrix_requirements_snapshots(tenant_id,workspace_id,id)
);

CREATE FUNCTION public.matrix_task_requirements_binding_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE source record;
DECLARE frozen record;
DECLARE level_name text;
DECLARE opened record;
DECLARE actual_program uuid;
DECLARE actual_scope uuid;
BEGIN
    IF TG_OP<>'INSERT' THEN
        RAISE EXCEPTION 'matrix task requirements binding is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'matrix task requirements tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT request_id,recorded_by_principal_id,recorded_by_session_id,xmin::text AS inserting_xid
      INTO source FROM public.matrix_task_revisions
     WHERE (tenant_id,workspace_id,task_id,revision)=(NEW.tenant_id,NEW.workspace_id,NEW.task_id,NEW.revision);
    IF NOT FOUND OR source.request_id IS DISTINCT FROM NEW.request_id THEN
        RAISE EXCEPTION 'matrix task requirements revision mismatch' USING ERRCODE='23514';
    END IF;
    -- The bound revision must have been inserted by this very transaction;
    -- this excludes retroactive authority links on historical revisions.
    IF source.inserting_xid IS DISTINCT FROM
       (pg_catalog.pg_current_xact_id()::text::bigint % 4294967296)::text THEN
        RAISE EXCEPTION 'historical matrix revision cannot gain requirements binding' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM public.agent_sessions s
      JOIN public.hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
      JOIN public.principals p ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id)
      JOIN public.memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)=(s.tenant_id,s.workspace_id,p.id)
     WHERE (s.tenant_id,s.workspace_id,s.id)=(NEW.tenant_id,NEW.workspace_id,source.recorded_by_session_id)
       AND h.principal_id=source.recorded_by_principal_id AND NOT s.revoked AND NOT h.revoked AND p.role='owner'
     FOR SHARE OF s,h,p,m;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'matrix task requirements requires active owner session' USING ERRCODE='42501';
    END IF;
    SELECT anchor,program_id,schema_version,semantic_digest
      INTO frozen FROM public.matrix_requirements_snapshots
     WHERE (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.snapshot_id);
    IF NOT FOUND OR frozen.schema_version IS DISTINCT FROM NEW.authority_schema
       OR frozen.semantic_digest IS DISTINCT FROM NEW.semantic_digest THEN
        RAISE EXCEPTION 'matrix task requirements snapshot mismatch' USING ERRCODE='23514';
    END IF;
    level_name := NEW.requirements_locator->>'level';
    IF level_name='opened_slice' THEN
        SELECT n.scope_id,n.candidate_id,n.candidate_revision,n.opening_snapshot_id,
               n.origin_payload,s.candidate_set_id,c.program_id
          INTO opened FROM public.native_slices n
          JOIN public.slice_planning_snapshots s ON (s.tenant_id,s.workspace_id,s.id)=(n.tenant_id,n.workspace_id,n.opening_snapshot_id)
          JOIN public.slice_candidate_sets work_set ON (work_set.tenant_id,work_set.workspace_id,work_set.id)=(s.tenant_id,s.workspace_id,s.candidate_set_id)
              AND work_set.scope_id=n.scope_id
          JOIN public.native_scopes sc ON (sc.tenant_id,sc.workspace_id,sc.id)=(n.tenant_id,n.workspace_id,n.scope_id)
          JOIN public.scope_candidate_sets c ON (c.tenant_id,c.workspace_id,c.id)=(sc.tenant_id,sc.workspace_id,sc.source_candidate_set_id)
         WHERE (n.tenant_id,n.workspace_id,n.id)=(NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'slice_id')::uuid)
         FOR UPDATE OF n,work_set,sc;
        IF NOT FOUND OR frozen.anchor IS DISTINCT FROM pg_catalog.jsonb_build_object(
            'level','slice','program_id',opened.program_id,'scope_id',opened.scope_id,
            'candidate_set_id',opened.candidate_set_id,'work_candidate_id',opened.candidate_id)
           OR opened.origin_payload->>'scope_id' IS DISTINCT FROM opened.scope_id::text
           OR opened.origin_payload->>'candidate_set_id' IS DISTINCT FROM opened.candidate_set_id::text
           OR opened.origin_payload->>'candidate_snapshot_id' IS DISTINCT FROM opened.opening_snapshot_id::text
           OR opened.origin_payload->>'candidate_id' IS DISTINCT FROM opened.candidate_id::text
           OR opened.origin_payload->>'candidate_revision' IS DISTINCT FROM opened.candidate_revision::text
           OR NOT EXISTS (
               SELECT 1 FROM public.slice_candidate_drafts d
               CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(d.payload->'nodes') node
               WHERE (d.tenant_id,d.workspace_id,d.candidate_set_id)=
                     (NEW.tenant_id,NEW.workspace_id,opened.candidate_set_id)
                 AND d.set_revision=(
                     SELECT max(historical.set_revision) FROM public.slice_candidate_drafts historical
                     WHERE (historical.tenant_id,historical.workspace_id,historical.candidate_set_id)=
                           (NEW.tenant_id,NEW.workspace_id,opened.candidate_set_id)
                       AND historical.set_revision<=(opened.origin_payload->>'candidate_set_revision')::bigint)
                 AND NOT d.payload_erased AND node->>'kind'='work'
                 AND node->>'id'=opened.candidate_id::text
                 AND node->>'revision'=opened.candidate_revision::text) THEN
            RAISE EXCEPTION 'matrix task requirements opened Slice mismatch' USING ERRCODE='23514';
        END IF;
    ELSIF level_name IN ('program','scope','slice') THEN
        IF frozen.anchor IS DISTINCT FROM (NEW.requirements_locator - 'expected_work_revision') THEN
            RAISE EXCEPTION 'matrix task requirements locator mismatch' USING ERRCODE='23514';
        END IF;
        PERFORM 1 FROM public.programs
         WHERE (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'program_id')::uuid)
         FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION 'matrix task requirements Program absent' USING ERRCODE='23514';
        END IF;
        IF level_name IN ('scope','slice') THEN
            SELECT c.program_id INTO actual_program FROM public.native_scopes n
            JOIN public.scope_candidate_sets c
              ON (c.tenant_id,c.workspace_id,c.id)=(n.tenant_id,n.workspace_id,n.source_candidate_set_id)
            WHERE (n.tenant_id,n.workspace_id,n.id)=
                  (NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'scope_id')::uuid)
            FOR UPDATE OF n;
            IF actual_program IS DISTINCT FROM (NEW.requirements_locator->>'program_id')::uuid THEN
                RAISE EXCEPTION 'matrix task requirements Scope ancestry mismatch' USING ERRCODE='23514';
            END IF;
        END IF;
        IF level_name='slice' THEN
            IF pg_catalog.jsonb_typeof(NEW.requirements_locator->'expected_work_revision') IS DISTINCT FROM 'number' THEN
                RAISE EXCEPTION 'matrix task requirements Work revision type invalid' USING ERRCODE='23514';
            END IF;
            IF (NEW.requirements_locator->>'expected_work_revision')::bigint IS NULL
               OR (NEW.requirements_locator->>'expected_work_revision')::bigint <= 0 THEN
                RAISE EXCEPTION 'matrix task requirements Work revision invalid' USING ERRCODE='23514';
            END IF;
            SELECT scope_id INTO actual_scope FROM public.slice_candidate_sets
             WHERE (tenant_id,workspace_id,id)=
                   (NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'candidate_set_id')::uuid)
             FOR UPDATE;
            IF actual_scope IS DISTINCT FROM (NEW.requirements_locator->>'scope_id')::uuid
               OR NOT EXISTS (
                   SELECT 1 FROM public.slice_candidate_drafts d
                   CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(d.payload->'nodes') node
                   WHERE (d.tenant_id,d.workspace_id,d.candidate_set_id)=
                         (NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'candidate_set_id')::uuid)
                     AND d.set_revision=(
                         SELECT max(latest.set_revision) FROM public.slice_candidate_drafts latest
                         WHERE (latest.tenant_id,latest.workspace_id,latest.candidate_set_id)=
                               (NEW.tenant_id,NEW.workspace_id,(NEW.requirements_locator->>'candidate_set_id')::uuid))
                     AND NOT d.payload_erased AND node->>'kind'='work'
                     AND node->>'id'=NEW.requirements_locator->>'work_candidate_id'
                     AND node->>'revision'=NEW.requirements_locator->>'expected_work_revision') THEN
                RAISE EXCEPTION 'matrix task requirements Work ancestry or revision mismatch' USING ERRCODE='23514';
            END IF;
        END IF;
    ELSE
        RAISE EXCEPTION 'matrix task requirements locator level invalid' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER matrix_task_requirements_binding_immutable
    BEFORE INSERT OR UPDATE OR DELETE ON public.matrix_task_requirements_bindings
    FOR EACH ROW EXECUTE FUNCTION public.matrix_task_requirements_binding_guard();
ALTER TABLE public.matrix_task_requirements_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.matrix_task_requirements_bindings FORCE ROW LEVEL SECURITY;
CREATE POLICY matrix_task_requirements_binding_tenant_scope
    ON public.matrix_task_requirements_bindings
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE public.matrix_task_requirements_bindings FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_task_requirements_binding_guard() FROM PUBLIC;
