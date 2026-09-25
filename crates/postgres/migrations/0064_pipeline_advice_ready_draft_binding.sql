-- A ready review advances the candidate set revision after the saved draft.
-- Preserve the 0062 guard while binding Matrix effect evidence to the latest
-- saved draft, rather than to the later ready-review revision.
CREATE OR REPLACE FUNCTION pipeline_advice_context_require_current() RETURNS trigger
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
    JOIN public.slice_candidate_reviews AS review
      ON (review.tenant_id,review.workspace_id,review.candidate_set_id,
          review.set_revision)=
         (c.tenant_id,c.workspace_id,c.id,c.revision)
    JOIN public.matrix_planning_effect_attestations AS a
      ON (a.tenant_id,a.workspace_id,a.id)=
         (o.tenant_id,o.workspace_id,NEW.match_effect_attestation_id)
    JOIN public.matrix_planning_selection_links AS l
      ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=
         (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)
    JOIN public.slice_candidate_drafts AS draft
      ON (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,
          draft.set_revision)=
         (l.tenant_id,l.workspace_id,l.candidate_set_id,l.result_revision)
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
      AND c.status='ready' AND c.revision=NEW.candidate_set_revision
      AND c.current_snapshot_id=snapshot.id
      AND snapshot.source_snapshot_id=NEW.source_snapshot_id
      AND review.payload IS NOT NULL AND NOT review.payload_erased
      AND review.payload->>'verdict'='ready'
      AND draft.set_revision<c.revision
      AND NOT draft.payload_erased AND draft.payload IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM public.slice_candidate_drafts AS newer
          WHERE (newer.tenant_id,newer.workspace_id,newer.candidate_set_id)=
                (draft.tenant_id,draft.workspace_id,draft.candidate_set_id)
            AND newer.set_revision>draft.set_revision)
      AND a.candidate_set_id=c.id AND a.result_revision=draft.set_revision
      AND a.verdict='match' AND l.result_revision=draft.set_revision
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
      AND pg_catalog.jsonb_typeof(draft.payload->'nodes')='array'
      AND EXISTS (
          SELECT 1 FROM pg_catalog.jsonb_array_elements(draft.payload->'nodes') AS node
          WHERE node->>'kind'='work'
            AND node->>'id'=NEW.work_node_id::text
            AND node->>'revision'=NEW.work_node_revision::text)
      AND p.id=o.authorized_actor_id AND p.role='owner'
      AND NOT s.revoked AND NOT h.revoked
    FOR SHARE OF o,c,snapshot,review,draft,a,l,d,task,receipt,s,h,p,m;
    IF admissible IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline advice requires current ready set, saved draft and matched effect'
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
