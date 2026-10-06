-- Slice 03 starts before a Slice or pipeline run exists. This frozen input is
-- advice provenance only; it has no phase, readiness, or verdict authority.
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_decision_check,
    ADD CONSTRAINT advisory_opportunity_decision_check CHECK (
        decision_point IN (
            'scope.decomposition.before_selection',
            'engineering.profile.before_selection',
            'pipeline_recommendation_before_slice_open'
        )
        AND pg_catalog.btrim(policy_version) <> ''
        AND pg_catalog.btrim(request_key) <> ''
        AND config_revision >= 0
    ),
    DROP CONSTRAINT advisory_opportunity_decision_capability_check,
    ADD CONSTRAINT advisory_opportunity_decision_capability_check CHECK (
        (capability = 'scope_decomposition'
         AND decision_point = 'scope.decomposition.before_selection')
        OR (capability = 'engineering_profile'
            AND decision_point = 'engineering.profile.before_selection')
        OR (capability = 'pipeline_recommendation'
            AND decision_point = 'pipeline_recommendation_before_slice_open')
    ),
    DROP CONSTRAINT advisory_opportunity_matrix_binding_check,
    ADD CONSTRAINT advisory_opportunity_matrix_binding_check CHECK (
        (capability = 'scope_decomposition'
         AND decision_point = 'scope.decomposition.before_selection'
         AND matrix_task_revision IS NULL AND matrix_choice_set_digest IS NULL)
        OR (capability = 'engineering_profile'
            AND decision_point = 'engineering.profile.before_selection'
            AND work_item_kind = 'matrix_task' AND work_item_id IS NOT NULL
            AND scope_id IS NULL AND matrix_task_revision IS NOT NULL
            AND matrix_task_revision >= 1 AND source_revision IS NOT NULL
            AND source_revision = matrix_task_revision::text
            AND ((matrix_choice_set_digest IS NULL AND state = 'no_call')
                 OR (matrix_choice_set_digest IS NOT NULL
                     AND matrix_choice_set_digest ~ '^[0-9a-f]{64}$')))
        OR (capability = 'pipeline_recommendation'
            AND decision_point = 'pipeline_recommendation_before_slice_open'
            AND work_item_kind = 'slice_candidate_node'
            AND work_item_id IS NOT NULL AND scope_id IS NOT NULL
            AND run_id IS NULL AND phase IS NULL AND step IS NULL
            AND matrix_task_revision IS NULL AND matrix_choice_set_digest IS NULL)
    ),
    DROP CONSTRAINT advisory_opportunity_matrix_verification_capability_check,
    ADD CONSTRAINT advisory_opportunity_matrix_verification_capability_check CHECK (
        (capability IN ('scope_decomposition', 'pipeline_recommendation')
         AND matrix_verification_digest IS NULL)
        OR (capability = 'engineering_profile'
            AND (state NOT IN ('prepared', 'awaiting_response', 'advised')
                 OR matrix_verification_digest IS NOT NULL))
    ) NOT VALID;

CREATE TABLE pipeline_advice_contexts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    candidate_set_revision bigint NOT NULL CHECK (candidate_set_revision >= 2),
    planning_snapshot_id uuid NOT NULL,
    source_snapshot_id uuid NOT NULL,
    work_node_id uuid NOT NULL,
    work_node_revision bigint NOT NULL CHECK (work_node_revision >= 1),
    source_snapshot_digest text NOT NULL
        CHECK (source_snapshot_digest ~ '^[0-9a-f]{64}$'),
    matrix_disposition_id uuid NOT NULL,
    match_effect_attestation_id uuid NOT NULL,
    catalogue_revision text NOT NULL CHECK (
        pg_catalog.length(pg_catalog.btrim(catalogue_revision)) BETWEEN 1 AND 256),
    catalogue_digest text NOT NULL CHECK (catalogue_digest ~ '^[0-9a-f]{64}$'),
    eligible_kind_ids text[] NOT NULL CHECK (
        pg_catalog.cardinality(eligible_kind_ids) BETWEEN 1 AND 8),
    verification_contract_digest text NOT NULL
        CHECK (verification_contract_digest ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, opportunity_id),
    CONSTRAINT pipeline_advice_context_opportunity_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id) REFERENCES advisory_opportunity
        (tenant_id, workspace_id, id),
    CONSTRAINT pipeline_advice_context_set_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id) REFERENCES slice_candidate_sets
        (tenant_id, workspace_id, id),
    CONSTRAINT pipeline_advice_context_snapshot_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id, planning_snapshot_id)
        REFERENCES slice_planning_snapshots
        (tenant_id, workspace_id, candidate_set_id, id),
    CONSTRAINT pipeline_advice_context_disposition_fk FOREIGN KEY
        (tenant_id, workspace_id, matrix_disposition_id)
        REFERENCES advisory_matrix_disposition (tenant_id, workspace_id, disposition_id),
    CONSTRAINT pipeline_advice_context_effect_fk FOREIGN KEY
        (tenant_id, workspace_id, match_effect_attestation_id)
        REFERENCES matrix_planning_effect_attestations (tenant_id, workspace_id, id)
);

-- Lock the live saved set, its receipt, the selected Matrix path, and the
-- independently matched effect. Old attestations remain historical evidence
-- when the set advances and cannot seed a new advice context.
CREATE FUNCTION pipeline_advice_context_require_current() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $current_context$
DECLARE admissible boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'pipeline advice tenant does not match session'
            USING ERRCODE = '42501';
    END IF;
    SELECT true INTO admissible
    FROM public.advisory_opportunity AS o
    JOIN public.slice_candidate_sets AS c
      ON (c.tenant_id,c.workspace_id,c.id)=
         (o.tenant_id,o.workspace_id,NEW.candidate_set_id)
    JOIN public.slice_planning_snapshots AS snapshot
      ON (snapshot.tenant_id,snapshot.workspace_id,snapshot.candidate_set_id,
          snapshot.id)=
         (c.tenant_id,c.workspace_id,c.id,NEW.planning_snapshot_id)
    JOIN public.matrix_planning_effect_attestations AS a
      ON (a.tenant_id,a.workspace_id,a.id)=
         (o.tenant_id,o.workspace_id,NEW.match_effect_attestation_id)
    JOIN public.matrix_planning_selection_links AS l
      ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
         (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
    JOIN public.advisory_matrix_disposition AS d
      ON (d.tenant_id,d.workspace_id,d.disposition_id)=
         (l.tenant_id,l.workspace_id,l.disposition_id)
    JOIN public.matrix_tasks AS task
      ON (task.tenant_id,task.workspace_id,task.id)=
         (l.tenant_id,l.workspace_id,l.task_id)
    JOIN public.native_planning_receipts AS receipt
      ON (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,
          receipt.operation,receipt.request_id)=
         (l.tenant_id,l.workspace_id,l.candidate_set_id,
          l.operation,l.caller_request_id)
    JOIN public.agent_sessions AS s
      ON (s.tenant_id,s.workspace_id,s.id)=
         (o.tenant_id,o.workspace_id,o.session_id)
    JOIN public.hosts AS h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
    JOIN public.principals AS p
      ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id)
    JOIN public.memberships AS m
      ON (m.tenant_id,m.workspace_id,m.principal_id)=
         (o.tenant_id,o.workspace_id,p.id)
    WHERE (o.tenant_id,o.workspace_id,o.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
      AND o.capability='pipeline_recommendation'
      AND o.decision_point='pipeline_recommendation_before_slice_open'
      AND o.work_item_kind='slice_candidate_node'
      AND o.work_item_id=NEW.work_node_id AND o.scope_id=c.scope_id
      AND o.run_id IS NULL AND o.phase IS NULL AND o.step IS NULL
      AND c.revision=NEW.candidate_set_revision
      AND c.current_snapshot_id=snapshot.id
      AND snapshot.source_snapshot_id=NEW.source_snapshot_id
      AND a.candidate_set_id=c.id AND a.result_revision=c.revision
      AND a.verdict='match' AND l.result_revision=c.revision
      AND l.disposition_id=NEW.matrix_disposition_id
      AND d.outcome='selected' AND d.selected_choice_id=l.selected_choice_id
      AND d.task_id=l.task_id AND d.matrix_task_revision=l.task_revision
      AND task.current_revision=l.task_revision
      AND NOT receipt.payload_erased
      AND receipt.request_payload IS NOT NULL
      AND receipt.result_payload IS NOT NULL
      AND pg_catalog.jsonb_typeof(l.mapped_nodes)='array'
      AND EXISTS (
          SELECT 1 FROM pg_catalog.jsonb_array_elements(l.mapped_nodes) AS node
          WHERE node->>'node_id'=NEW.work_node_id::text
            AND node->>'node_revision'=NEW.work_node_revision::text)
      AND p.id=o.authorized_actor_id AND p.role='owner'
      AND NOT s.revoked AND NOT h.revoked
    FOR SHARE OF o,c,snapshot,a,l,d,task,receipt,s,h,p,m;
    IF admissible IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline advice requires current selected and matched saved node'
            USING ERRCODE = '42501';
    END IF;
    IF EXISTS (
        SELECT 1 FROM pg_catalog.unnest(NEW.eligible_kind_ids) AS kind
        WHERE kind IS NULL OR kind <> ALL (ARRAY[
            'slice.lightweight-tdd-development', 'slice.full-design-to-execution',
            'slice.debug-root-cause', 'slice.operational-preparation',
            'slice.operational-execution', 'slice.research',
            'slice.deep-brainstorming', 'slice.custom-procedure-capture']))
       OR (SELECT pg_catalog.count(DISTINCT kind)
           FROM pg_catalog.unnest(NEW.eligible_kind_ids) AS kind)
           <> pg_catalog.cardinality(NEW.eligible_kind_ids) THEN
        RAISE EXCEPTION 'pipeline advice eligible kinds must be unique current Slice run IDs'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$current_context$;

CREATE TRIGGER pipeline_advice_context_current
    BEFORE INSERT ON pipeline_advice_contexts FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_context_require_current();
CREATE TRIGGER pipeline_advice_context_immutable
    BEFORE UPDATE OR DELETE ON pipeline_advice_contexts FOR EACH ROW
    EXECUTE FUNCTION matrix_verification_deny_mutation();

ALTER TABLE pipeline_advice_contexts ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_advice_contexts FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_advice_contexts_tenant_scope ON pipeline_advice_contexts
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_advice_contexts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_advice_contexts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_advice_contexts FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_context_require_current() FROM PUBLIC;
