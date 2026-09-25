-- One immutable planning decision per saved pipeline opportunity. It confers
-- no authority over Slice state, pipeline execution, or verification.
CREATE TABLE pipeline_advice_dispositions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    disposition_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    request_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    session_id uuid NOT NULL,
    work_node_id uuid NOT NULL,
    work_node_revision bigint NOT NULL CHECK (work_node_revision >= 1),
    manifest_digest text NOT NULL CHECK (manifest_digest ~ '^[0-9a-f]{64}$'),
    matrix_disposition_id uuid NOT NULL,
    source_snapshot_id uuid NOT NULL,
    advice_kind text NOT NULL CHECK (advice_kind IN ('no_call','ranked','abstained')),
    dispatch_id uuid,
    request_payload jsonb NOT NULL,
    result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, disposition_id),
    UNIQUE (tenant_id, workspace_id, opportunity_id),
    UNIQUE (tenant_id, workspace_id, request_id),
    CHECK ((advice_kind='no_call' AND dispatch_id IS NULL) OR
           (advice_kind IN ('ranked','abstained') AND dispatch_id IS NOT NULL)),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id)
        REFERENCES advisory_opportunity (tenant_id,workspace_id,id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id)
        REFERENCES pipeline_advice_contexts (tenant_id,workspace_id,opportunity_id),
    FOREIGN KEY (tenant_id,workspace_id,opportunity_id,dispatch_id)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id),
    FOREIGN KEY (tenant_id,workspace_id,matrix_disposition_id)
        REFERENCES advisory_matrix_disposition (tenant_id,workspace_id,disposition_id),
    FOREIGN KEY (tenant_id,actor_id) REFERENCES principals (tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);

CREATE FUNCTION pipeline_advice_disposition_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $guard$
DECLARE admissible boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'pipeline disposition tenant does not match session' USING ERRCODE='42501';
    END IF;
    SELECT true INTO admissible
    FROM public.advisory_opportunity AS o
    JOIN public.pipeline_advice_contexts AS context
      ON (context.tenant_id,context.workspace_id,context.opportunity_id)=
         (o.tenant_id,o.workspace_id,o.id)
    JOIN public.slice_candidate_sets AS candidate
      ON (candidate.tenant_id,candidate.workspace_id,candidate.id)=
         (context.tenant_id,context.workspace_id,context.candidate_set_id)
    JOIN public.slice_planning_snapshots AS snapshot
      ON (snapshot.tenant_id,snapshot.workspace_id,snapshot.candidate_set_id,snapshot.id)=
         (candidate.tenant_id,candidate.workspace_id,candidate.id,candidate.current_snapshot_id)
    JOIN public.native_scopes AS scope
      ON (scope.tenant_id,scope.workspace_id,scope.id)=
         (candidate.tenant_id,candidate.workspace_id,candidate.scope_id)
    JOIN public.scope_candidate_sets AS source_set
      ON (source_set.tenant_id,source_set.workspace_id,source_set.id)=
         (scope.tenant_id,scope.workspace_id,scope.source_candidate_set_id)
    JOIN public.scope_candidate_snapshots AS source_snapshot
      ON (source_snapshot.tenant_id,source_snapshot.workspace_id,
          source_snapshot.candidate_set_id,source_snapshot.id)=
         (source_set.tenant_id,source_set.workspace_id,source_set.id,source_set.current_snapshot_id)
    JOIN public.slice_candidate_drafts AS draft
      ON (draft.tenant_id,draft.workspace_id,draft.candidate_set_id)=
         (candidate.tenant_id,candidate.workspace_id,candidate.id)
    JOIN public.slice_candidate_reviews AS review
      ON (review.tenant_id,review.workspace_id,review.candidate_set_id,review.set_revision)=
         (candidate.tenant_id,candidate.workspace_id,candidate.id,candidate.revision)
    JOIN public.matrix_planning_effect_attestations AS effect
      ON (effect.tenant_id,effect.workspace_id,effect.id)=
         (context.tenant_id,context.workspace_id,context.match_effect_attestation_id)
    JOIN public.matrix_planning_selection_links AS selection
      ON (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,selection.caller_request_id)=
         (effect.tenant_id,effect.workspace_id,effect.candidate_set_id,effect.caller_request_id)
    JOIN public.advisory_matrix_disposition AS matrix
      ON (matrix.tenant_id,matrix.workspace_id,matrix.disposition_id)=
         (context.tenant_id,context.workspace_id,context.matrix_disposition_id)
    JOIN public.matrix_tasks AS task
      ON (task.tenant_id,task.workspace_id,task.id)=
         (selection.tenant_id,selection.workspace_id,selection.task_id)
    JOIN public.native_planning_receipts AS receipt
      ON (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,receipt.operation,receipt.request_id)=
         (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,
          selection.operation,selection.caller_request_id)
    JOIN public.advisory_workspace_config AS config
      ON (config.tenant_id,config.workspace_id)=(o.tenant_id,o.workspace_id)
    JOIN public.agent_sessions AS session
      ON (session.tenant_id,session.workspace_id,session.id)=
         (o.tenant_id,o.workspace_id,o.session_id)
    JOIN public.hosts AS host ON (host.tenant_id,host.id)=(session.tenant_id,session.host_id)
    JOIN public.principals AS actor ON (actor.tenant_id,actor.id)=(host.tenant_id,host.principal_id)
    JOIN public.memberships AS membership
      ON (membership.tenant_id,membership.workspace_id,membership.principal_id)=
         (o.tenant_id,o.workspace_id,actor.id)
    WHERE (o.tenant_id,o.workspace_id,o.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
      AND o.capability='pipeline_recommendation'
      AND o.decision_point='pipeline_recommendation_before_slice_open'
      AND o.run_id IS NULL AND o.phase IS NULL AND o.step IS NULL
      AND o.work_item_id=NEW.work_node_id AND o.scope_id=candidate.scope_id
      AND o.material_digest=NEW.manifest_digest
      AND o.authorized_actor_id=NEW.actor_id AND o.session_id=NEW.session_id
      AND context.work_node_id=NEW.work_node_id
      AND context.work_node_revision=NEW.work_node_revision
      AND context.manifest_digest=NEW.manifest_digest
      AND context.verification_contract_digest=NEW.manifest_digest
      AND context.matrix_disposition_id=NEW.matrix_disposition_id
      AND context.source_snapshot_id=NEW.source_snapshot_id
      AND candidate.status='ready' AND candidate.revision=context.candidate_set_revision
      AND candidate.current_snapshot_id=context.planning_snapshot_id
      AND candidate.latest_input=snapshot.planning_latest_input
      AND scope.revision=snapshot.scope_revision AND NOT scope.payload_erased
      AND scope.source_snapshot_id=context.source_snapshot_id
      AND source_set.current_snapshot_id=context.source_snapshot_id
      AND source_set.revision=snapshot.source_candidate_set_revision
      AND source_snapshot.selected_sources_digest IS NOT NULL
      AND snapshot.source_snapshot_id=context.source_snapshot_id
      AND review.payload IS NOT NULL AND NOT review.payload_erased
      AND review.payload->>'verdict'='ready'
      AND draft.set_revision=(SELECT MAX(latest.set_revision)
          FROM public.slice_candidate_drafts AS latest
          WHERE (latest.tenant_id,latest.workspace_id,latest.candidate_set_id)=
                (draft.tenant_id,draft.workspace_id,draft.candidate_set_id))
      AND draft.set_revision<candidate.revision
      AND draft.payload IS NOT NULL AND NOT draft.payload_erased
      AND effect.verdict='match' AND effect.candidate_set_id=candidate.id
      AND effect.result_revision=draft.set_revision
      AND selection.result_revision=draft.set_revision
      AND selection.disposition_id=matrix.disposition_id
      AND matrix.outcome='selected'
      AND matrix.selected_choice_id=selection.selected_choice_id
      AND task.current_revision=selection.task_revision
      AND NOT receipt.payload_erased
      AND receipt.request_payload IS NOT NULL AND receipt.result_payload IS NOT NULL
      AND EXISTS (SELECT 1 FROM pg_catalog.jsonb_array_elements(draft.payload->'nodes') AS node
          WHERE node->>'kind'='work' AND node->>'id'=NEW.work_node_id::text
            AND node->>'revision'=NEW.work_node_revision::text)
      AND config.revision=o.config_revision
      AND NOT session.revoked AND NOT host.revoked
      AND actor.id=o.authorized_actor_id AND actor.role='owner'
      AND NOT EXISTS (SELECT 1 FROM public.native_slices AS opened
          WHERE (opened.tenant_id,opened.workspace_id,opened.scope_id,opened.candidate_id)=
                (candidate.tenant_id,candidate.workspace_id,candidate.scope_id,NEW.work_node_id))
      AND ((NEW.advice_kind='no_call' AND o.state='no_call'
            AND pg_catalog.cardinality(context.eligible_kind_ids)=0
            AND NOT EXISTS (SELECT 1 FROM public.advisory_dispatch AS d
                WHERE (d.tenant_id,d.workspace_id,d.opportunity_id)=
                      (o.tenant_id,o.workspace_id,o.id)))
        OR (NEW.advice_kind IN ('ranked','abstained')
            AND o.state IN ('awaiting_response','advised')
            AND EXISTS (SELECT 1 FROM public.advisory_dispatch AS d
                WHERE (d.tenant_id,d.workspace_id,d.opportunity_id,d.id)=
                      (o.tenant_id,o.workspace_id,o.id,NEW.dispatch_id)
                  AND d.state='sealed' AND d.send_certainty='sent'
                  AND d.outcome='provider_response'
                  AND d.pipeline_response_sha256=pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex'))))
    FOR SHARE OF o,context,candidate,snapshot,scope,source_set,source_snapshot,
                 draft,review,effect,selection,matrix,task,receipt,config,
                 session,host,actor,membership;
    IF admissible IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline disposition is stale or unauthorized' USING ERRCODE='42501';
    END IF;
    IF NEW.request_payload->>'request_id' IS DISTINCT FROM NEW.request_id::text
       OR NEW.request_payload->>'opportunity_id' IS DISTINCT FROM NEW.opportunity_id::text
       OR NEW.request_payload->>'manifest_digest' IS DISTINCT FROM NEW.manifest_digest
       OR NEW.request_payload->>'expected_work_revision' IS DISTINCT FROM NEW.work_node_revision::text
       OR NEW.result_payload->>'id' IS DISTINCT FROM NEW.disposition_id::text
       OR NEW.result_payload->>'work_id' IS DISTINCT FROM NEW.work_node_id::text
       OR NEW.result_payload->'request' IS DISTINCT FROM NEW.request_payload
       OR NEW.result_payload#>>'{advice,status}' IS DISTINCT FROM NEW.advice_kind
       OR (NEW.advice_kind<>'no_call' AND
           NEW.result_payload#>>'{advice,dispatch_id}' IS DISTINCT FROM NEW.dispatch_id::text)
       OR (NEW.advice_kind='no_call' AND
           NEW.result_payload#>>'{advice,dispatch_id}' IS NOT NULL) THEN
        RAISE EXCEPTION 'pipeline disposition payload binding differs' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;

CREATE TRIGGER pipeline_advice_disposition_current
    BEFORE INSERT ON pipeline_advice_dispositions FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_disposition_guard();
CREATE TRIGGER pipeline_advice_disposition_immutable
    BEFORE UPDATE OR DELETE ON pipeline_advice_dispositions FOR EACH ROW
    EXECUTE FUNCTION matrix_verification_deny_mutation();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_disposition_guard() FROM PUBLIC;

ALTER TABLE pipeline_advice_dispositions ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_advice_dispositions FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_advice_dispositions_tenant_scope ON pipeline_advice_dispositions
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_advice_dispositions'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_advice_dispositions'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_advice_dispositions FROM PUBLIC;
