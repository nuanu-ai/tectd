-- Context-aware Pipeline advice binds the exact independently verified Matrix
-- requirements snapshot. Existing schema/1-3 receipts remain immutable and
-- readable, but cannot authorize a new provider send.
ALTER TABLE public.pipeline_advice_contexts
    ADD COLUMN frozen_snapshot_id uuid,
    ADD COLUMN requirements_semantic_digest text,
    ADD COLUMN authority_schema text,
    ADD COLUMN operating_verification_digest text,
    ADD CONSTRAINT pipeline_context_matrix_authority_shape CHECK (
        (manifest_payload->>'schema' = 'tect.pipeline-recommendation/4'
         AND frozen_snapshot_id IS NOT NULL
         AND frozen_snapshot_id <> '00000000-0000-0000-0000-000000000000'::uuid
         AND requirements_semantic_digest IS NOT NULL
         AND requirements_semantic_digest ~ '^[0-9a-f]{64}$'
         AND authority_schema IS NOT NULL
         AND authority_schema = 'tect.matrix-requirements/1'
         AND operating_verification_digest IS NOT NULL
         AND operating_verification_digest ~ '^[0-9a-f]{64}$'
         AND COALESCE(manifest_payload->'matrix_authority' = pg_catalog.jsonb_build_object(
             'frozen_snapshot_id',frozen_snapshot_id::text,
             'requirements_semantic_digest',requirements_semantic_digest,
             'authority_schema',authority_schema,
             'operating_verification_digest',operating_verification_digest),false))
        OR (manifest_payload->>'schema' IN
                ('tect.pipeline-recommendation/1','tect.pipeline-recommendation/2',
                 'tect.pipeline-recommendation/3')
            AND frozen_snapshot_id IS NULL
            AND requirements_semantic_digest IS NULL
            AND authority_schema IS NULL
            AND operating_verification_digest IS NULL
            AND (manifest_payload->'matrix_authority' IS NULL
                 OR manifest_payload->'matrix_authority'='null'::jsonb))
    ) NOT VALID,
    ADD CONSTRAINT pipeline_context_matrix_snapshot_fk FOREIGN KEY
        (tenant_id,workspace_id,frozen_snapshot_id)
        REFERENCES public.matrix_requirements_snapshots(tenant_id,workspace_id,id);

ALTER TABLE public.pipeline_advice_contexts
    DROP CONSTRAINT pipeline_advice_compatibility_policy_digest_check,
    ADD CONSTRAINT pipeline_advice_compatibility_policy_digest_check CHECK (
        (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
            AND compatibility_policy_digest IS NULL)
        OR (manifest_payload->>'schema' IN
                ('tect.pipeline-recommendation/2','tect.pipeline-recommendation/3',
                 'tect.pipeline-recommendation/4')
            AND compatibility_policy_digest ~ '^[0-9a-f]{64}$'
            AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest)
    ) NOT VALID,
    DROP CONSTRAINT pipeline_advice_context_manifest_shape_check,
    ADD CONSTRAINT pipeline_advice_context_manifest_shape_check CHECK (COALESCE((
        pg_catalog.jsonb_typeof(manifest_payload) = 'object'
        AND manifest_payload->>'schema' IN
            ('tect.pipeline-recommendation/1','tect.pipeline-recommendation/2',
             'tect.pipeline-recommendation/3','tect.pipeline-recommendation/4')
        AND manifest_payload->>'work_id' = work_node_id::text
        AND manifest_payload->>'work_revision' = work_node_revision::text
        AND manifest_payload->>'catalogue_revision' = catalogue_revision
        AND manifest_payload->>'catalogue_digest' = catalogue_digest
        AND pg_catalog.jsonb_typeof(manifest_payload->'options') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'options') =
            pg_catalog.cardinality(eligible_option_ids)
        AND pg_catalog.jsonb_typeof(manifest_payload->'mandatory_card_ids') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'mandatory_card_ids') > 0
        AND (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
             OR (manifest_payload->>'matrix_input_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'selected_candidate_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest
                 AND pg_catalog.jsonb_typeof(manifest_payload->'excluded') = 'array'))
        AND (manifest_payload->>'schema' NOT IN
                ('tect.pipeline-recommendation/3','tect.pipeline-recommendation/4')
             OR (pg_catalog.jsonb_typeof(verification_plan_bindings) = 'array'
                 AND pg_catalog.jsonb_array_length(verification_plan_bindings) =
                     pg_catalog.cardinality(eligible_option_ids)))
    ), false)) NOT VALID;

-- Reuse the proven schema/3 pinned-plan projection for schema/4. This is a
-- forward replacement of the live functions, never an edit to old migrations.
DO $extend_pipeline_plan_shape$
DECLARE definition text; old_clause text; new_clause text;
BEGIN
    definition := pg_catalog.pg_get_functiondef(
        'public.pipeline_advice_manifest_require_shape()'::pg_catalog.regprocedure);
    old_clause := 'IF NEW.manifest_payload->>''schema'' = ''tect.pipeline-recommendation/3'' THEN';
    new_clause := 'IF NEW.manifest_payload->>''schema'' IN ' ||
        '(''tect.pipeline-recommendation/3'',''tect.pipeline-recommendation/4'') THEN';
    IF pg_catalog.strpos(definition,old_clause)=0 THEN
        RAISE EXCEPTION 'pipeline manifest shape function drifted' USING ERRCODE='23514';
    END IF;
    EXECUTE pg_catalog.replace(definition,old_clause,new_clause);

    definition := pg_catalog.pg_get_functiondef(
        'public.pipeline_advice_persist_plan_binding()'::pg_catalog.regprocedure);
    old_clause := 'context.manifest_payload->>''schema'' <> ''tect.pipeline-recommendation/3''';
    new_clause := 'context.manifest_payload->>''schema'' NOT IN ' ||
        '(''tect.pipeline-recommendation/3'',''tect.pipeline-recommendation/4'')';
    IF pg_catalog.strpos(definition,old_clause)=0 THEN
        RAISE EXCEPTION 'pipeline plan binding function drifted' USING ERRCODE='23514';
    END IF;
    EXECUTE pg_catalog.replace(definition,old_clause,new_clause);
END
$extend_pipeline_plan_shape$;

-- The insert gate checks exact saved Matrix selection/effect lineage and the
-- V2 verification tuple. Rust additionally validates current effective
-- declaration semantics and operating evidence under declaration head locks.
CREATE FUNCTION public.pipeline_context_matrix_authority_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE valid_path boolean;
BEGIN
    IF NEW.manifest_payload->>'schema' <> 'tect.pipeline-recommendation/4' THEN
        RETURN NEW;
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline Matrix authority tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT true INTO valid_path
      FROM public.matrix_planning_effect_attestations a
      JOIN public.matrix_planning_selection_links l
        ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
           (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
      JOIN public.matrix_tasks t
        ON (t.tenant_id,t.workspace_id,t.id)=(l.tenant_id,l.workspace_id,l.task_id)
      JOIN public.matrix_task_requirements_bindings b
        ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)=
           (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
      JOIN public.matrix_requirements_snapshots s
        ON (s.tenant_id,s.workspace_id,s.id)=(b.tenant_id,b.workspace_id,b.snapshot_id)
      JOIN public.matrix_verifications v
        ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,
            v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema)=
           (b.tenant_id,b.workspace_id,b.task_id,b.revision,
            b.snapshot_id,b.semantic_digest,b.authority_schema)
     WHERE (a.tenant_id,a.workspace_id,a.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.match_effect_attestation_id)
       AND a.verdict='match' AND l.disposition_id=NEW.matrix_disposition_id
       AND t.current_revision=l.task_revision
       AND l.frozen_snapshot_id=b.snapshot_id
       AND l.authority_schema=b.authority_schema
       AND l.requirements_semantic_digest=b.semantic_digest
       AND b.snapshot_id=NEW.frozen_snapshot_id
       AND b.semantic_digest=NEW.requirements_semantic_digest
       AND b.authority_schema=NEW.authority_schema
       AND s.semantic_digest=b.semantic_digest AND s.schema_version=b.authority_schema
       AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND v.input_digest=l.input_digest
       AND v.record_digest=l.verification_digest
       AND v.record_digest=NEW.operating_verification_digest
       AND NEW.manifest_payload->>'matrix_task_id'=l.task_id::text
       AND NEW.manifest_payload->>'matrix_task_revision'=l.task_revision::text
       AND NEW.manifest_payload->>'matrix_verification_digest'=v.record_digest
     FOR SHARE OF a,l,t,b,s,v;
    IF valid_path IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline Matrix V2 authority is unbound or stale' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER zz_pipeline_context_matrix_authority
    BEFORE INSERT ON public.pipeline_advice_contexts FOR EACH ROW
    EXECUTE FUNCTION public.pipeline_context_matrix_authority_guard();

-- Historical schema/3 advice can be inspected and reconciled. A fresh send
-- requires the exact persisted schema/4 tuple and V2 Matrix record.
CREATE FUNCTION public.pipeline_dispatch_matrix_authority_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public,pg_temp AS $guard$
DECLARE pipeline boolean; context public.pipeline_advice_contexts%ROWTYPE;
BEGIN
    SELECT o.capability='pipeline_recommendation' INTO pipeline
      FROM public.advisory_opportunity o
     WHERE (o.tenant_id,o.workspace_id,o.id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id);
    IF pipeline IS DISTINCT FROM true THEN RETURN NEW; END IF;
    IF TG_OP='UPDATE' AND OLD.state='sending' AND NEW.state='sealed' THEN
        RETURN NEW;
    END IF;
    SELECT * INTO context FROM public.pipeline_advice_contexts c
     WHERE (c.tenant_id,c.workspace_id,c.opportunity_id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id) FOR SHARE;
    IF NOT FOUND OR context.manifest_payload->>'schema'<>'tect.pipeline-recommendation/4'
       OR context.frozen_snapshot_id IS NULL
       OR context.requirements_semantic_digest IS NULL
       OR context.authority_schema IS NULL
       OR context.operating_verification_digest IS NULL THEN
        RAISE EXCEPTION 'pipeline send requires bound Matrix V2 authority' USING ERRCODE='42501';
    END IF;
    -- Recheck the persisted immutable tuple against current task/selection
    -- identity. The app holds declaration heads and validates semantic facts.
    PERFORM 1 FROM public.matrix_planning_effect_attestations a
      JOIN public.matrix_planning_selection_links l
        ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
           (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
      JOIN public.matrix_tasks t
        ON (t.tenant_id,t.workspace_id,t.id)=(l.tenant_id,l.workspace_id,l.task_id)
      JOIN public.matrix_task_requirements_bindings b
        ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)=
           (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
      JOIN public.matrix_verifications v
        ON (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,
            v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema)=
           (b.tenant_id,b.workspace_id,b.task_id,b.revision,
            b.snapshot_id,b.semantic_digest,b.authority_schema)
     WHERE (a.tenant_id,a.workspace_id,a.id)=
           (context.tenant_id,context.workspace_id,context.match_effect_attestation_id)
       AND l.disposition_id=context.matrix_disposition_id
       AND t.current_revision=l.task_revision
       AND l.frozen_snapshot_id=b.snapshot_id
       AND l.authority_schema=b.authority_schema
       AND l.requirements_semantic_digest=b.semantic_digest
       AND b.snapshot_id=context.frozen_snapshot_id
       AND b.semantic_digest=context.requirements_semantic_digest
       AND b.authority_schema=context.authority_schema
       AND v.schema='tect.context-matrix-verification/1'
       AND v.verification_reason='operating_facts_verified'
       AND v.input_digest=l.input_digest
       AND v.record_digest=l.verification_digest
       AND v.record_digest=context.operating_verification_digest
     FOR SHARE OF a,l,t,b,v;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'pipeline send Matrix V2 authority changed' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER zz_pipeline_dispatch_matrix_authority
    BEFORE INSERT OR UPDATE ON public.advisory_dispatch FOR EACH ROW
    EXECUTE FUNCTION public.pipeline_dispatch_matrix_authority_guard();

REVOKE ALL PRIVILEGES ON FUNCTION public.pipeline_context_matrix_authority_guard() FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION public.pipeline_dispatch_matrix_authority_guard() FROM PUBLIC;
