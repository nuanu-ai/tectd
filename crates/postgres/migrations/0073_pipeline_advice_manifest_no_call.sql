-- A deterministic empty option set is a durable no-call opportunity. The
-- pinned manifest is kept with its context, including the empty case, so a
-- later catalogue revision cannot rewrite the material that was considered.
ALTER TABLE pipeline_advice_contexts
    DROP CONSTRAINT pipeline_advice_contexts_eligible_kind_ids_check,
    ADD CONSTRAINT pipeline_advice_contexts_eligible_kind_ids_check
        CHECK (pg_catalog.cardinality(eligible_kind_ids) BETWEEN 0 AND 8),
    -- Existing 0062 rows remain distinguishable as legacy NULL records.
    -- Both NOT VALID checks still reject incomplete new INSERTs.
    ADD COLUMN manifest_payload jsonb,
    ADD COLUMN manifest_digest text,
    ADD CONSTRAINT pipeline_advice_context_manifest_digest_check CHECK (COALESCE((
        manifest_digest IS NOT NULL AND manifest_digest ~ '^[0-9a-f]{64}$'
        AND manifest_payload IS NOT NULL
        AND manifest_payload->>'digest' = manifest_digest), false)) NOT VALID,
    ADD CONSTRAINT pipeline_advice_context_manifest_shape_check CHECK (COALESCE((
        manifest_payload IS NOT NULL
        AND pg_catalog.jsonb_typeof(manifest_payload) = 'object'
        AND manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
        AND manifest_payload->>'work_id' = work_node_id::text
        AND manifest_payload->>'work_revision' = work_node_revision::text
        AND manifest_payload->>'catalogue_revision' = catalogue_revision
        AND manifest_payload->>'catalogue_digest' = catalogue_digest
        AND pg_catalog.jsonb_typeof(manifest_payload->'options') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'options') =
            pg_catalog.cardinality(eligible_kind_ids)
        AND pg_catalog.jsonb_typeof(manifest_payload->'mandatory_card_ids') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'mandatory_card_ids') > 0
    ), false)) NOT VALID;

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled', 'session_skip', 'request_skip',
        'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
        'budget_policy_invalid', 'choice_set_not_applicable', 'matrix_evidence_unresolved',
        'matrix_source_unverified', 'configuration_changed', 'matrix_task_revision_changed',
        'matrix_verification_stale', 'dispatch_authorized', 'recommendation_prepared',
        'provider_response', 'provider_failure', 'send_unknown'
    )) NOT VALID;

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_state_reason_check,
    ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state = 'no_call' AND primary_reason IN (
            'workspace_disabled', 'session_skip', 'request_skip',
            'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
            'budget_policy_invalid', 'choice_set_not_applicable'
        ))
        OR (state = 'no_call' AND primary_reason IN (
            'matrix_evidence_unresolved', 'matrix_source_unverified'
        ) AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task')
        OR (state = 'prepared' AND primary_reason = 'dispatch_authorized')
        OR (state = 'prepared' AND primary_reason = 'recommendation_prepared'
            AND capability = 'pipeline_recommendation'
            AND decision_point = 'pipeline_recommendation_before_slice_open')
        OR (state = 'awaiting_response' AND primary_reason IN ('dispatch_authorized', 'send_unknown'))
        OR (state = 'advised' AND primary_reason = 'provider_response')
        OR (state = 'invalidated' AND primary_reason = 'configuration_changed')
        OR (state = 'invalidated' AND primary_reason IN (
            'matrix_task_revision_changed', 'matrix_verification_stale'
        ) AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task')
        OR (state = 'failed' AND primary_reason = 'provider_failure')
        OR (state = 'unresolved' AND primary_reason = 'send_unknown')
    ) NOT VALID;

-- The 0062 currentness trigger rechecks and locks the saved Work/Matrix path.
-- This guard adds the cross-row no-call rule and exact manifest-field binding.
CREATE FUNCTION pipeline_advice_manifest_require_shape() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $manifest_shape$
DECLARE opportunity_state text;
DECLARE option_ids text[];
DECLARE manifest_matches boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'pipeline advice tenant does not match session'
            USING ERRCODE = '42501';
    END IF;
    SELECT o.state INTO opportunity_state
    FROM public.advisory_opportunity AS o
    WHERE (o.tenant_id,o.workspace_id,o.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
    FOR SHARE;
    IF opportunity_state IS NULL
       OR (pg_catalog.cardinality(NEW.eligible_kind_ids)=0
           AND opportunity_state <> 'no_call') THEN
        RAISE EXCEPTION 'empty pipeline advice is only valid for a no-call opportunity'
            USING ERRCODE = '23514';
    END IF;

    SELECT COALESCE(pg_catalog.array_agg(option->>'id' ORDER BY ordinal), ARRAY[]::text[])
      INTO option_ids
    FROM pg_catalog.jsonb_array_elements(NEW.manifest_payload->'options')
         WITH ORDINALITY AS options(option, ordinal);
    IF option_ids IS DISTINCT FROM NEW.eligible_kind_ids THEN
        RAISE EXCEPTION 'pipeline advice options differ from eligible IDs'
            USING ERRCODE = '23514';
    END IF;

    SELECT true INTO manifest_matches
    FROM public.matrix_planning_effect_attestations AS a
    JOIN public.matrix_planning_selection_links AS l
      ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
         (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
    WHERE (a.tenant_id,a.workspace_id,a.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.match_effect_attestation_id)
      AND NEW.manifest_payload->>'matrix_task_id'=l.task_id::text
      AND NEW.manifest_payload->>'matrix_task_revision'=l.task_revision::text
      AND NEW.manifest_payload->>'selected_choice_id'=l.selected_choice_id
      AND NEW.manifest_payload->>'matrix_choice_set_digest'=l.choice_set_digest
      AND NEW.manifest_payload->>'matrix_verification_digest'=l.verification_digest
    FOR SHARE OF a,l;
    IF manifest_matches IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline advice manifest does not bind the selected Matrix path'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$manifest_shape$;

CREATE TRIGGER pipeline_advice_context_manifest_shape
    BEFORE INSERT ON pipeline_advice_contexts FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_manifest_require_shape();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_manifest_require_shape() FROM PUBLIC;

-- The opportunity state is mutable, so retain the empty-set invariant after
-- capture as well as at context INSERT.
CREATE FUNCTION pipeline_advice_preserve_empty_no_call() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $empty_no_call$
BEGIN
    IF NEW.capability='pipeline_recommendation' AND NEW.state<>'no_call'
       AND EXISTS (
           SELECT 1 FROM public.pipeline_advice_contexts AS context
           WHERE (context.tenant_id,context.workspace_id,context.opportunity_id)=
                 (NEW.tenant_id,NEW.workspace_id,NEW.id)
             AND pg_catalog.cardinality(context.eligible_kind_ids)=0) THEN
        RAISE EXCEPTION 'empty pipeline advice cannot leave no-call state'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$empty_no_call$;

CREATE TRIGGER advisory_opportunity_pipeline_empty_no_call
    BEFORE UPDATE OF state ON advisory_opportunity FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_preserve_empty_no_call();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_preserve_empty_no_call() FROM PUBLIC;
