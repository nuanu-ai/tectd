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

-- Internal adapter capability: lock one complete V2 parent before child reads.
-- No content return, TTL decision, mutation or additional authority-head locks.
CREATE FUNCTION public.matrix_planning_lock_verification(
    p_tenant uuid,p_workspace uuid,p_verification uuid
) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $verification_lock$
DECLARE tenant_setting text;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Matrix V2 planning requires READ COMMITTED'
            USING ERRCODE='0A000';
    END IF;
    IF pg_catalog.current_setting('transaction_read_only') <> 'off' THEN
        RAISE EXCEPTION 'Matrix verification lock requires writable transaction'
            USING ERRCODE='42501';
    END IF;
    tenant_setting := NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '');
    IF tenant_setting IS NULL OR tenant_setting !~
        '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$' THEN
        RAISE EXCEPTION 'Matrix planning requires valid tenant' USING ERRCODE='42501';
    END IF;
    IF p_tenant IS DISTINCT FROM tenant_setting::uuid THEN
        RAISE EXCEPTION 'Matrix verification tenant mismatch' USING ERRCODE='42501';
    END IF;
    PERFORM 1 FROM public.matrix_verifications v
     WHERE (v.tenant_id,v.workspace_id,v.id)=(p_tenant,p_workspace,p_verification)
       AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND v.frozen_snapshot_id IS NOT NULL
       AND v.authority_schema='tect.matrix-requirements/1'
       AND v.requirements_semantic_digest ~ '^[0-9a-f]{64}$'
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix V2 verification parent absent' USING ERRCODE='23514';
    END IF;
END
$verification_lock$;

-- Authority lock order is explicit: task, revision, revision source binding,
-- exact V2 verification parent, then existing evidence bindings. Outer PG
-- declaration-head locks are acquired by the adapter before this task lock.
-- SQL enforces tuple identity/point-in-time TTL, not Domain coverage/composition.
CREATE FUNCTION public.matrix_planning_lock_context(
    p_tenant uuid,p_workspace uuid,p_task uuid,p_revision bigint,
    p_input text,p_choice text,p_verification text,p_snapshot uuid,p_authority text,p_semantic text
) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $context_lock$
DECLARE locked_verification_id uuid; owner_id uuid; now_epoch bigint; tenant_setting text;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Matrix V2 planning requires READ COMMITTED'
            USING ERRCODE = '0A000';
    END IF;
    tenant_setting := NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '');
    IF tenant_setting IS NULL OR tenant_setting !~
        '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$' THEN
        RAISE EXCEPTION 'Matrix planning requires valid tenant' USING ERRCODE='42501';
    END IF;
    IF p_tenant IS DISTINCT FROM tenant_setting::uuid THEN
        RAISE EXCEPTION 'Matrix context tenant mismatch' USING ERRCODE='42501';
    END IF;
    IF p_snapshot IS NULL THEN
        RAISE EXCEPTION 'New Matrix authority requires V2 context' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM public.matrix_tasks t
     WHERE (t.tenant_id,t.workspace_id,t.id,t.current_revision)=
           (p_tenant,p_workspace,p_task,p_revision) FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix task head is stale' USING ERRCODE='23514';
    END IF;
    SELECT r.recorded_by_principal_id INTO owner_id
      FROM public.matrix_task_revisions r
     WHERE (r.tenant_id,r.workspace_id,r.task_id,r.revision,r.input_digest,r.choice_set_digest)=
           (p_tenant,p_workspace,p_task,p_revision,p_input,p_choice) FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix revision material is stale' USING ERRCODE='23514';
    END IF;
    PERFORM 1 FROM public.matrix_task_requirements_bindings b
     WHERE (b.tenant_id,b.workspace_id,b.task_id,b.revision,
            b.snapshot_id,b.authority_schema,b.semantic_digest)=
           (p_tenant,p_workspace,p_task,p_revision,p_snapshot,p_authority,p_semantic) FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix source binding is stale' USING ERRCODE='23514';
    END IF;
    -- Digest uniqueness in migration0052 resolves one parent, never a "latest" row.
    SELECT v.id INTO locked_verification_id FROM public.matrix_verifications v
     WHERE (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.record_digest)=
           (p_tenant,p_workspace,p_task,p_revision,p_verification)
       AND v.input_digest=p_input AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND (v.frozen_snapshot_id,v.authority_schema,v.requirements_semantic_digest)=
           (p_snapshot,p_authority,p_semantic)
       AND v.owner_principal_id=owner_id AND v.verifier_principal_id<>owner_id
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix verification parent is stale' USING ERRCODE='23514';
    END IF;
    PERFORM b.fact_path FROM public.matrix_verification_bindings b
     WHERE (b.tenant_id,b.workspace_id,b.verification_id)=
           (p_tenant,p_workspace,locked_verification_id) ORDER BY b.fact_path FOR SHARE;
    -- One actual clock reading after all authority and existing-child locks.
    now_epoch := pg_catalog.floor(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint;
    IF EXISTS(SELECT 1 FROM public.matrix_verification_bindings b
       WHERE (b.tenant_id,b.workspace_id,b.verification_id)=
             (p_tenant,p_workspace,locked_verification_id)
         AND (b.expires_at<=now_epoch OR b.observed_at>now_epoch
              OR b.expires_at<=b.observed_at OR b.validation_outcome<>'accepted')) THEN
        RAISE EXCEPTION 'Matrix operating evidence expired or invalid' USING ERRCODE='23514';
    END IF;
    -- Empty bindings are TTL-neutral. Required coverage remains Domain/PG proof.
END
$context_lock$;

CREATE FUNCTION public.matrix_planning_selection_require_context() RETURNS trigger
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $selection_context$
BEGIN
    PERFORM public.matrix_planning_lock_context(
        NEW.tenant_id,NEW.workspace_id,NEW.task_id,NEW.task_revision,
        NEW.input_digest,NEW.choice_set_digest,NEW.verification_digest,
        NEW.frozen_snapshot_id,NEW.authority_schema,NEW.requirements_semantic_digest);
    RETURN NEW;
END
$selection_context$;
CREATE TRIGGER matrix_planning_selection_context_guard
    BEFORE INSERT ON public.matrix_planning_selection_links
    FOR EACH ROW EXECUTE FUNCTION public.matrix_planning_selection_require_context();

CREATE FUNCTION public.matrix_planning_effect_require_context() RETURNS trigger
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $effect_context$
DECLARE link public.matrix_planning_selection_links%ROWTYPE;
BEGIN
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Matrix V2 planning requires READ COMMITTED'
            USING ERRCODE = '0A000';
    END IF;
    SELECT * INTO link FROM public.matrix_planning_selection_links l
     WHERE (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.candidate_set_id,NEW.caller_request_id)
     FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix effect link absent' USING ERRCODE='23514';
    END IF;
    PERFORM public.matrix_planning_lock_context(
        NEW.tenant_id,NEW.workspace_id,link.task_id,link.task_revision,
        link.input_digest,link.choice_set_digest,link.verification_digest,
        link.frozen_snapshot_id,link.authority_schema,link.requirements_semantic_digest);
    RETURN NEW;
END
$effect_context$;
CREATE TRIGGER matrix_planning_effect_context_guard
    BEFORE INSERT ON public.matrix_planning_effect_attestations
    FOR EACH ROW EXECUTE FUNCTION public.matrix_planning_effect_require_context();

-- V2 child INSERTs serialize on exactly the same parent as authority INSERTs.
-- No task/head lock here: that would invert the authority lock order.
CREATE FUNCTION public.matrix_verification_bindings_require_unconsumed_v2() RETURNS trigger
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp
AS $binding_guard$
DECLARE parent public.matrix_verifications%ROWTYPE; schema_name text; consumed boolean;
        tenant_setting text;
BEGIN
    SELECT v.schema INTO schema_name FROM public.matrix_verifications v
     WHERE (v.tenant_id,v.workspace_id,v.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.verification_id);
    IF schema_name IS DISTINCT FROM 'tect.context-matrix-verification/1' THEN
        RETURN NEW; -- Preserve the original V1/FK/RLS behavior.
    END IF;
    IF pg_catalog.current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Matrix V2 planning requires READ COMMITTED'
            USING ERRCODE = '0A000';
    END IF;
    tenant_setting := NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '');
    IF tenant_setting IS NULL OR tenant_setting !~
        '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$' THEN
        RAISE EXCEPTION 'Matrix planning requires a valid tenant setting'
            USING ERRCODE = '42501';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM tenant_setting::uuid THEN
        RAISE EXCEPTION 'Matrix planning tenant mismatch' USING ERRCODE = '42501';
    END IF;
    SELECT * INTO parent FROM public.matrix_verifications v
     WHERE (v.tenant_id,v.workspace_id,v.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.verification_id)
       AND v.schema='tect.context-matrix-verification/1'
     FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Matrix V2 binding parent absent' USING ERRCODE='23514';
    END IF;
    -- Deliberately separate SPI statement: VOLATILE READ COMMITTED obtains
    -- fresh visibility after a parent-lock wait, including a consumed link.
    SELECT EXISTS(SELECT 1 FROM public.matrix_planning_selection_links l
      WHERE (l.tenant_id,l.workspace_id,l.task_id,l.task_revision,l.verification_digest)=
            (parent.tenant_id,parent.workspace_id,parent.task_id,parent.task_revision,parent.record_digest)
        AND (l.input_digest,l.frozen_snapshot_id,l.authority_schema,l.requirements_semantic_digest)=
            (parent.input_digest,parent.frozen_snapshot_id,parent.authority_schema,parent.requirements_semantic_digest))
      INTO consumed;
    IF consumed THEN
        RAISE EXCEPTION 'Consumed Matrix V2 verification cannot gain bindings'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$binding_guard$;
CREATE TRIGGER matrix_verification_bindings_unconsumed_v2_guard
    BEFORE INSERT ON public.matrix_verification_bindings
    FOR EACH ROW EXECUTE FUNCTION public.matrix_verification_bindings_require_unconsumed_v2();

REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_lock_context(
    uuid,uuid,uuid,bigint,text,text,text,uuid,text,text) FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_lock_verification(uuid,uuid,uuid) FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_selection_require_context() FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_planning_effect_require_context() FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.matrix_verification_bindings_require_unconsumed_v2() FROM PUBLIC;
