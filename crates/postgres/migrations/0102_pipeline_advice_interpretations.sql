-- Interpretation is advisory output, never the agent's explicit disposition.
CREATE TABLE pipeline_advice_interpretations (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL, dispatch_id uuid NOT NULL,
    manifest_digest text NOT NULL CHECK (manifest_digest ~ '^[0-9a-f]{64}$'),
    response_sha256 text NOT NULL CHECK (response_sha256 ~ '^[0-9a-f]{64}$'),
    contract_version integer NOT NULL CHECK (contract_version=1),
    ranking jsonb NOT NULL CHECK (jsonb_typeof(ranking)='object'),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,opportunity_id),
    UNIQUE (tenant_id,workspace_id,dispatch_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id)
        REFERENCES pipeline_advice_contexts (tenant_id,workspace_id,opportunity_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id)
);

CREATE FUNCTION pipeline_advice_interpretation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog,public AS $guard$
DECLARE eligible text[];
BEGIN
    IF TG_OP<>'INSERT' THEN
        RAISE EXCEPTION 'pipeline interpretation is immutable' USING ERRCODE='23514';
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline interpretation tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT context.eligible_kind_ids INTO eligible
    FROM public.pipeline_advice_contexts context
    JOIN public.advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=
        (context.tenant_id,context.workspace_id,context.opportunity_id)
    JOIN public.advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.opportunity_id)=
        (o.tenant_id,o.workspace_id,o.id)
    JOIN public.advisory_provider_observations r ON (r.tenant_id,r.workspace_id,r.opportunity_id,r.dispatch_id)=
        (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)
    JOIN public.advisory_budget_consumptions c ON (c.tenant_id,c.workspace_id,c.dispatch_id)=
        (d.tenant_id,d.workspace_id,d.id)
    WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=
        (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
      AND o.capability='pipeline_recommendation' AND o.state IN ('awaiting_response','advised')
      AND context.manifest_digest=NEW.manifest_digest AND d.material_digest=NEW.manifest_digest
      AND d.state='sealed' AND d.send_certainty='sent' AND d.outcome='provider_response'
      AND d.pipeline_response_sha256=NEW.response_sha256
      AND encode(sha256(d.response_payload),'hex')=NEW.response_sha256
      AND r.response_sha256=NEW.response_sha256 AND r.response_payload=d.response_payload
      AND r.response_complete AND NOT c.unknown_usage AND NOT c.exhausted_after_response
    -- Context, raw observation and consumption are immutable and runtime has
    -- SELECT/INSERT only; lock just the mutable lifecycle rows.
    FOR SHARE OF o,d;
    IF eligible IS NULL OR NOT COALESCE((
        NEW.ranking='{"status":"abstained"}'::jsonb OR (
            NEW.ranking->>'status'='ranked' AND jsonb_typeof(NEW.ranking->'ranked_ids')='array'
            AND NEW.ranking=jsonb_build_object('status','ranked','ranked_ids',NEW.ranking->'ranked_ids')
            AND jsonb_array_length(NEW.ranking->'ranked_ids')=cardinality(eligible)
            AND ARRAY(SELECT value FROM jsonb_array_elements_text(NEW.ranking->'ranked_ids') ORDER BY value)
                = ARRAY(SELECT value FROM unnest(eligible) value ORDER BY value))),false) THEN
        RAISE EXCEPTION 'pipeline interpretation binding or ranking invalid' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $guard$;
CREATE TRIGGER pipeline_advice_interpretation_guard BEFORE INSERT OR UPDATE OR DELETE
    ON pipeline_advice_interpretations FOR EACH ROW EXECUTE FUNCTION pipeline_advice_interpretation_guard();
ALTER TABLE pipeline_advice_interpretations ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_advice_interpretations FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_advice_interpretations_tenant_scope ON pipeline_advice_interpretations
    USING (tenant_id=NULLIF(current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL ON TABLE pipeline_advice_interpretations FROM PUBLIC;
REVOKE ALL ON FUNCTION pipeline_advice_interpretation_guard() FROM PUBLIC;

-- Preserve the applied legacy codec only when no interpretation exists.
DO $extend$
DECLARE definition text; old_parse text;
BEGIN
    SELECT pg_get_functiondef('public.pipeline_advice_disposition_require_sealed_response()'::regprocedure) INTO definition;
    old_parse := 'ranking := pg_catalog.convert_from(saved_bytes,''UTF8'')::jsonb;';
    IF strpos(definition,old_parse)=0 THEN RAISE EXCEPTION 'legacy ranking parser changed'; END IF;
    definition := replace(definition,old_parse,$parse$
        IF EXISTS (SELECT 1 FROM public.pipeline_advice_interpretations i
            WHERE (i.tenant_id,i.workspace_id,i.opportunity_id)=
                  (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)) THEN
            SELECT i.ranking INTO ranking FROM public.pipeline_advice_interpretations i
            JOIN public.pipeline_advice_contexts context ON (context.tenant_id,context.workspace_id,context.opportunity_id)=
                (i.tenant_id,i.workspace_id,i.opportunity_id)
            WHERE (i.tenant_id,i.workspace_id,i.opportunity_id,i.dispatch_id)=
                  (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.dispatch_id)
              AND i.contract_version=1 AND i.manifest_digest=context.manifest_digest
              AND i.response_sha256=encode(sha256(saved_bytes),'hex');
            IF ranking IS NULL THEN RAISE EXCEPTION 'saved interpretation binding invalid'; END IF;
        ELSE
            ranking := pg_catalog.convert_from(saved_bytes,'UTF8')::jsonb;
        END IF;
    $parse$);
    EXECUTE definition;
END $extend$;
