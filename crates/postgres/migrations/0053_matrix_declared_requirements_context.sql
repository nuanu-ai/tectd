-- Declared promises require an explicit, append-only owner confirmation.
CREATE TABLE public.matrix_requirements_proposals (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    request_id uuid NOT NULL CHECK (request_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    anchor jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(anchor)='object'),
    program_id uuid NOT NULL,
    scope_id uuid,
    candidate_set_id uuid,
    work_candidate_id uuid,
    context_revision bigint NOT NULL CHECK (context_revision > 0),
    proposal_digest text NOT NULL CHECK (proposal_digest ~ '^[0-9a-f]{64}$'),
    request_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(request_payload)='object'),
    proposal_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(proposal_payload)='object'),
    recorded_by_principal_id uuid NOT NULL,
    recorded_by_session_id uuid NOT NULL,
    recorded_at_epoch_seconds bigint NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,anchor,context_revision),
    UNIQUE (tenant_id,workspace_id,request_id),
    UNIQUE (tenant_id,workspace_id,anchor,context_revision,proposal_digest),
    FOREIGN KEY (tenant_id,workspace_id,program_id) REFERENCES public.programs(tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,workspace_id,scope_id) REFERENCES public.native_scopes(tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,workspace_id,candidate_set_id) REFERENCES public.slice_candidate_sets(tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,recorded_by_principal_id) REFERENCES public.principals(tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,recorded_by_session_id) REFERENCES public.agent_sessions(tenant_id,workspace_id,id),
    CHECK ((proposal_payload->'anchor'=anchor AND proposal_payload->>'digest'=proposal_digest
        AND (proposal_payload->>'revision')::bigint=context_revision
        AND proposal_payload->'recorder'->>'principal'=recorded_by_principal_id::text
        AND proposal_payload->'recorder'->>'session'=recorded_by_session_id::text) IS TRUE)
);

CREATE TABLE public.matrix_requirements_confirmations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    request_id uuid NOT NULL CHECK (request_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    anchor jsonb NOT NULL,
    program_id uuid NOT NULL,
    proposal_revision bigint NOT NULL CHECK (proposal_revision > 0),
    proposal_digest text NOT NULL,
    owner_response_ref text NOT NULL CHECK (pg_catalog.btrim(owner_response_ref)<>''),
    request_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(request_payload)='object'),
    confirmation_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(confirmation_payload)='object'),
    recorded_by_principal_id uuid NOT NULL,
    recorded_by_session_id uuid NOT NULL,
    recorded_at_epoch_seconds bigint NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,anchor,proposal_revision),
    UNIQUE (tenant_id,workspace_id,request_id),
    FOREIGN KEY (tenant_id,workspace_id,anchor,proposal_revision,proposal_digest)
        REFERENCES public.matrix_requirements_proposals(tenant_id,workspace_id,anchor,context_revision,proposal_digest),
    FOREIGN KEY (tenant_id,workspace_id,program_id) REFERENCES public.programs(tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,recorded_by_principal_id) REFERENCES public.principals(tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,recorded_by_session_id) REFERENCES public.agent_sessions(tenant_id,workspace_id,id),
    CHECK ((confirmation_payload->'anchor'=anchor
        AND (confirmation_payload->>'proposal_revision')::bigint=proposal_revision
        AND confirmation_payload->>'proposal_digest'=proposal_digest
        AND confirmation_payload->>'owner_response_ref'=owner_response_ref
        AND confirmation_payload->>'owner_principal'=recorded_by_principal_id::text
        AND confirmation_payload->'recorder'->>'principal'=recorded_by_principal_id::text
        AND confirmation_payload->'recorder'->>'session'=recorded_by_session_id::text) IS TRUE)
);

CREATE TABLE public.matrix_requirements_snapshots (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL CHECK (id <> '00000000-0000-0000-0000-000000000000'::uuid),
    anchor jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(anchor)='object'),
    program_id uuid NOT NULL,
    schema_version text NOT NULL CHECK (schema_version='tect.matrix-requirements/1'),
    payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(payload)='object'),
    canonical_payload bytea NOT NULL CHECK (pg_catalog.octet_length(canonical_payload) BETWEEN 1 AND 8388608),
    payload_sha256 text NOT NULL CHECK (payload_sha256=pg_catalog.encode(pg_catalog.sha256(canonical_payload),'hex')),
    semantic_digest text NOT NULL CHECK (semantic_digest ~ '^[0-9a-f]{64}$'),
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,anchor,payload_sha256),
    FOREIGN KEY (tenant_id,workspace_id,program_id) REFERENCES public.programs(tenant_id,workspace_id,id),
    CHECK ((pg_catalog.convert_from(canonical_payload,'UTF8')::jsonb=payload
        AND payload->>'schema'=schema_version AND payload->>'program_id'=program_id::text
        AND payload->>'semantic_digest'=semantic_digest) IS TRUE)
);

CREATE FUNCTION public.matrix_requirements_anchor_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE actual_program uuid;
DECLARE actual_scope uuid;
DECLARE target_scope uuid;
DECLARE target_set uuid;
DECLARE target_work uuid;
DECLARE level_name text;
BEGIN
    IF TG_OP<>'INSERT' THEN
        RAISE EXCEPTION 'matrix requirements record is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'matrix requirements tenant mismatch' USING ERRCODE='42501';
    END IF;
    level_name := NEW.anchor->>'level';
    target_scope := (NEW.anchor->>'scope_id')::uuid;
    target_set := (NEW.anchor->>'candidate_set_id')::uuid;
    target_work := (NEW.anchor->>'work_candidate_id')::uuid;
    IF (NEW.anchor->>'program_id')::uuid IS DISTINCT FROM NEW.program_id
       OR level_name NOT IN ('program','scope','slice') OR level_name IS NULL THEN
        RAISE EXCEPTION 'matrix requirements anchor mismatch' USING ERRCODE='23514';
    END IF;
    IF (level_name='program' AND (target_scope IS NOT NULL OR target_set IS NOT NULL OR target_work IS NOT NULL))
       OR (level_name='scope' AND (target_scope IS NULL OR target_set IS NOT NULL OR target_work IS NOT NULL))
       OR (level_name='slice' AND (target_scope IS NULL OR target_set IS NULL OR target_work IS NULL)) THEN
        RAISE EXCEPTION 'matrix requirements anchor shape mismatch' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM public.programs WHERE (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.program_id) FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'matrix requirements Program absent' USING ERRCODE='23514'; END IF;
    IF level_name IN ('scope','slice') THEN
        SELECT c.program_id INTO actual_program FROM public.native_scopes n
        JOIN public.scope_candidate_sets c ON (c.tenant_id,c.workspace_id,c.id)=(n.tenant_id,n.workspace_id,n.source_candidate_set_id)
        WHERE (n.tenant_id,n.workspace_id,n.id)=(NEW.tenant_id,NEW.workspace_id,target_scope) FOR UPDATE OF n;
        IF actual_program IS DISTINCT FROM NEW.program_id THEN
            RAISE EXCEPTION 'matrix requirements Scope ancestry mismatch' USING ERRCODE='23514';
        END IF;
    END IF;
    IF level_name='slice' THEN
        SELECT scope_id INTO actual_scope FROM public.slice_candidate_sets
        WHERE (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,target_set) FOR UPDATE;
        IF actual_scope IS DISTINCT FROM target_scope OR target_work IS NULL OR NOT (
            EXISTS (
                SELECT 1 FROM public.slice_candidate_drafts d
                CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(d.payload->'nodes') node
                WHERE (d.tenant_id,d.workspace_id,d.candidate_set_id)=(NEW.tenant_id,NEW.workspace_id,target_set)
                AND d.set_revision=(SELECT max(latest.set_revision) FROM public.slice_candidate_drafts latest
                    WHERE (latest.tenant_id,latest.workspace_id,latest.candidate_set_id)=(NEW.tenant_id,NEW.workspace_id,target_set))
                AND NOT d.payload_erased AND node->>'kind'='work' AND node->>'id'=target_work::text
            ) OR EXISTS (
                SELECT 1 FROM public.native_slices opened
                JOIN public.slice_planning_snapshots snapshot
                    ON (snapshot.tenant_id,snapshot.workspace_id,snapshot.id)=(opened.tenant_id,opened.workspace_id,opened.opening_snapshot_id)
                    AND snapshot.candidate_set_id=target_set
                JOIN public.slice_candidate_drafts d
                    ON (d.tenant_id,d.workspace_id,d.candidate_set_id)=(opened.tenant_id,opened.workspace_id,target_set)
                    AND d.set_revision=(SELECT max(historical.set_revision) FROM public.slice_candidate_drafts historical
                        WHERE (historical.tenant_id,historical.workspace_id,historical.candidate_set_id)=(opened.tenant_id,opened.workspace_id,target_set)
                        AND historical.set_revision<=(opened.origin_payload->>'candidate_set_revision')::bigint)
                CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(d.payload->'nodes') node
                WHERE (opened.tenant_id,opened.workspace_id,opened.scope_id,opened.candidate_id)=(NEW.tenant_id,NEW.workspace_id,target_scope,target_work)
                AND opened.origin_payload->>'scope_id'=target_scope::text
                AND opened.origin_payload->>'candidate_set_id'=target_set::text
                AND opened.origin_payload->>'candidate_snapshot_id'=opened.opening_snapshot_id::text
                AND opened.origin_payload->>'candidate_id'=opened.candidate_id::text
                AND opened.origin_payload->>'candidate_revision'=opened.candidate_revision::text
                AND NOT d.payload_erased AND node->>'kind'='work' AND node->>'id'=target_work::text
                AND node->>'revision'=opened.candidate_revision::text
            )) THEN
            RAISE EXCEPTION 'matrix requirements Work ancestry mismatch' USING ERRCODE='23514';
        END IF;
    END IF;
    IF TG_TABLE_NAME='matrix_requirements_proposals' THEN
        IF (NEW.scope_id,NEW.candidate_set_id,NEW.work_candidate_id) IS DISTINCT FROM (target_scope,target_set,target_work) THEN
            RAISE EXCEPTION 'matrix requirements anchor columns mismatch' USING ERRCODE='23514';
        END IF;
        IF NEW.context_revision <> COALESCE((SELECT max(context_revision) FROM public.matrix_requirements_proposals
            WHERE (tenant_id,workspace_id,anchor)=(NEW.tenant_id,NEW.workspace_id,NEW.anchor)),0)+1 THEN
            RAISE EXCEPTION 'matrix requirements context revision conflict' USING ERRCODE='23514';
        END IF;
    END IF;
    IF TG_TABLE_NAME<>'matrix_requirements_snapshots' THEN
        IF NOT EXISTS (SELECT 1 FROM public.agent_sessions s
            JOIN public.hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
            JOIN public.principals p ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id)
            JOIN public.memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)=(s.tenant_id,s.workspace_id,p.id)
            WHERE (s.tenant_id,s.workspace_id,s.id)=(NEW.tenant_id,NEW.workspace_id,NEW.recorded_by_session_id)
            AND h.principal_id=NEW.recorded_by_principal_id AND NOT s.revoked AND NOT h.revoked AND p.role='owner') THEN
            RAISE EXCEPTION 'matrix requirements requires active owner session' USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END
$guard$;

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY['matrix_requirements_proposals','matrix_requirements_confirmations','matrix_requirements_snapshots'] LOOP
        EXECUTE pg_catalog.format('CREATE TRIGGER matrix_requirements_record_guard BEFORE INSERT OR UPDATE OR DELETE ON public.%I FOR EACH ROW EXECUTE FUNCTION public.matrix_requirements_anchor_guard()',relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE public.%I FORCE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('CREATE POLICY matrix_requirements_tenant_scope ON public.%I USING (tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',relation_name);
    END LOOP;
END
$policy$;
REVOKE ALL PRIVILEGES ON TABLE public.matrix_requirements_proposals,public.matrix_requirements_confirmations,public.matrix_requirements_snapshots FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_requirements_anchor_guard() FROM PUBLIC;
