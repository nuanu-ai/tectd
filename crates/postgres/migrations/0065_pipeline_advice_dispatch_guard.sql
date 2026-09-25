-- Pipeline advice has one irreversible provider-send boundary per opportunity.
-- The request bytes and manifest binding live in advisory_dispatch; no new
-- authority over a Slice run or verification result is introduced here.
ALTER TABLE advisory_dispatch ADD COLUMN pipeline_response_sha256 text;
ALTER TABLE advisory_dispatch ADD CONSTRAINT advisory_dispatch_pipeline_response_digest_check
    CHECK (pipeline_response_sha256 IS NULL OR pipeline_response_sha256 ~ '^[0-9a-f]{64}$') NOT VALID;

CREATE FUNCTION pipeline_advice_dispatch_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $pipeline_dispatch$
DECLARE
    opportunity public.advisory_opportunity%ROWTYPE;
    current_path boolean;
    config_ok boolean;
BEGIN
    SELECT * INTO opportunity FROM public.advisory_opportunity
     WHERE (tenant_id,workspace_id,id)=(NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
     FOR UPDATE;
    IF NOT FOUND OR opportunity.capability <> 'pipeline_recommendation' THEN
        RETURN NEW;
    END IF;
    IF NEW.tenant_id IS DISTINCT FROM
       NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'pipeline dispatch tenant does not match session'
            USING ERRCODE = '42501';
    END IF;
    IF opportunity.decision_point <> 'pipeline_recommendation_before_slice_open'
       OR opportunity.work_item_kind <> 'slice_candidate_node'
       OR opportunity.run_id IS NOT NULL OR opportunity.phase IS NOT NULL
       OR opportunity.step IS NOT NULL OR NEW.material_digest <> opportunity.material_digest
       OR NEW.attempt_number <> 1 OR NEW.predecessor_dispatch_id IS NOT NULL
       OR NEW.retry_basis <> 'initial' THEN
        RAISE EXCEPTION 'pipeline dispatch does not bind its opportunity'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id,NEW.id,
            NEW.provider,NEW.model,NEW.configuration_snapshot,NEW.configuration_digest,
            NEW.material_digest,NEW.payload_digest,NEW.request_payload,
            NEW.attempt_number,NEW.predecessor_dispatch_id,NEW.retry_basis)
           IS DISTINCT FROM
           (OLD.tenant_id,OLD.workspace_id,OLD.opportunity_id,OLD.id,
            OLD.provider,OLD.model,OLD.configuration_snapshot,OLD.configuration_digest,
            OLD.material_digest,OLD.payload_digest,OLD.request_payload,
            OLD.attempt_number,OLD.predecessor_dispatch_id,OLD.retry_basis) THEN
            RAISE EXCEPTION 'pipeline dispatch request is immutable'
                USING ERRCODE = '23514';
        END IF;
        IF OLD.state='sending' AND NEW.state='sealed' THEN
            IF NEW.send_certainty <> 'sent' OR NEW.outcome <> 'provider_response'
               OR NEW.response_payload IS NULL
               OR pg_catalog.octet_length(NEW.response_payload) NOT BETWEEN 1 AND 65536
               OR NEW.pipeline_response_sha256 IS NULL
               OR NEW.pipeline_response_sha256 <>
                  pg_catalog.encode(pg_catalog.sha256(NEW.response_payload),'hex')
               OR OLD.pipeline_response_sha256 IS NOT NULL THEN
                RAISE EXCEPTION 'pipeline response seal is incomplete'
                    USING ERRCODE = '23514';
            END IF;
            RETURN NEW;
        END IF;
        IF OLD.state <> 'authorized' OR NEW.state <> 'sending'
           OR NEW.send_certainty <> 'sent_unknown'
           OR OLD.pipeline_response_sha256 IS NOT NULL
           OR NEW.pipeline_response_sha256 IS NOT NULL THEN
            RAISE EXCEPTION 'pipeline dispatch cannot restart or change its seal'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF NEW.state <> 'authorized' OR NEW.send_certainty <> 'not_sent'
           OR NEW.pipeline_response_sha256 IS NOT NULL
           OR EXISTS (SELECT 1 FROM public.advisory_dispatch AS prior
                      WHERE (prior.tenant_id,prior.workspace_id,prior.opportunity_id)=
                            (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)) THEN
            RAISE EXCEPTION 'pipeline opportunity permits one dispatch only'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    IF opportunity.state <> 'prepared'
       OR opportunity.primary_reason <> 'recommendation_prepared'
       OR opportunity.session_preference <> 'use_workspace'
       OR opportunity.request_preference <> 'use_workspace'
       OR NEW.configuration_snapshot->>'request_body_sha256' IS DISTINCT FROM NEW.payload_digest
       OR pg_catalog.octet_length(NEW.request_payload) NOT BETWEEN 1 AND 524288
       OR NEW.payload_digest <>
          pg_catalog.encode(pg_catalog.sha256(NEW.request_payload),'hex')
       OR pg_catalog.length(COALESCE(NEW.configuration_snapshot->>'destination','')) NOT BETWEEN 1 AND 256
       OR pg_catalog.length(COALESCE(NEW.configuration_snapshot->>'wire_version','')) NOT BETWEEN 1 AND 256 THEN
        RAISE EXCEPTION 'pipeline dispatch is not a prepared call'
            USING ERRCODE = '23514';
    END IF;
    SELECT true INTO config_ok FROM public.advisory_workspace_config AS cfg
     WHERE (cfg.tenant_id,cfg.workspace_id)=(NEW.tenant_id,NEW.workspace_id)
       AND cfg.revision=opportunity.config_revision AND cfg.mode='optional'
       AND cfg.provider_profile_ref IS NOT NULL AND cfg.model_configuration IS NOT NULL
       AND cfg.provider_profile_ref=NEW.configuration_snapshot->>'provider_profile_ref'
       AND cfg.model_configuration->>'model'=NEW.model FOR SHARE;
    IF config_ok IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline dispatch configuration changed'
            USING ERRCODE = '42501';
    END IF;
    SELECT true INTO current_path
      FROM public.pipeline_advice_contexts AS context
      JOIN public.slice_candidate_sets AS candidate
        ON (candidate.tenant_id,candidate.workspace_id,candidate.id)=
           (context.tenant_id,context.workspace_id,context.candidate_set_id)
      JOIN public.slice_planning_snapshots AS snapshot
        ON (snapshot.tenant_id,snapshot.workspace_id,snapshot.candidate_set_id,snapshot.id)=
           (context.tenant_id,context.workspace_id,context.candidate_set_id,context.planning_snapshot_id)
      JOIN public.slice_candidate_reviews AS review
        ON (review.tenant_id,review.workspace_id,review.candidate_set_id,review.set_revision)=
           (candidate.tenant_id,candidate.workspace_id,candidate.id,candidate.revision)
      JOIN public.matrix_planning_effect_attestations AS effect
        ON (effect.tenant_id,effect.workspace_id,effect.id)=
           (context.tenant_id,context.workspace_id,context.match_effect_attestation_id)
      JOIN public.matrix_planning_selection_links AS selection
        ON (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,selection.caller_request_id)=
           (effect.tenant_id,effect.workspace_id,effect.candidate_set_id,effect.caller_request_id)
      JOIN public.advisory_matrix_disposition AS disposition
        ON (disposition.tenant_id,disposition.workspace_id,disposition.disposition_id)=
           (context.tenant_id,context.workspace_id,context.matrix_disposition_id)
      JOIN public.matrix_tasks AS task
        ON (task.tenant_id,task.workspace_id,task.id)=
           (selection.tenant_id,selection.workspace_id,selection.task_id)
      JOIN public.slice_candidate_drafts AS draft
        ON (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,draft.set_revision)=
           (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,selection.result_revision)
      JOIN public.native_planning_receipts AS receipt
        ON (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,receipt.operation,receipt.request_id)=
           (selection.tenant_id,selection.workspace_id,selection.candidate_set_id,
            selection.operation,selection.caller_request_id)
      JOIN public.agent_sessions AS session
        ON (session.tenant_id,session.workspace_id,session.id)=
           (opportunity.tenant_id,opportunity.workspace_id,opportunity.session_id)
      JOIN public.hosts AS host ON (host.tenant_id,host.id)=(session.tenant_id,session.host_id)
      JOIN public.principals AS actor ON (actor.tenant_id,actor.id)=(host.tenant_id,host.principal_id)
      JOIN public.memberships AS membership
        ON (membership.tenant_id,membership.workspace_id,membership.principal_id)=
           (opportunity.tenant_id,opportunity.workspace_id,actor.id)
     WHERE (context.tenant_id,context.workspace_id,context.opportunity_id)=
           (NEW.tenant_id,NEW.workspace_id,NEW.opportunity_id)
       AND context.manifest_digest=NEW.material_digest
       AND context.verification_contract_digest=NEW.material_digest
       AND pg_catalog.cardinality(context.eligible_kind_ids)>0
       AND candidate.status='ready' AND candidate.revision=context.candidate_set_revision
       AND candidate.current_snapshot_id=context.planning_snapshot_id
       AND snapshot.source_snapshot_id=context.source_snapshot_id
       AND context.work_node_id=opportunity.work_item_id
       AND candidate.scope_id=opportunity.scope_id
       AND review.payload IS NOT NULL AND NOT review.payload_erased
       AND review.payload->>'verdict'='ready'
       AND draft.set_revision<candidate.revision
       AND draft.payload IS NOT NULL AND NOT draft.payload_erased
       AND NOT EXISTS (
           SELECT 1 FROM public.slice_candidate_drafts AS newer
           WHERE (newer.tenant_id,newer.workspace_id,newer.candidate_set_id)=
                 (draft.tenant_id,draft.workspace_id,draft.candidate_set_id)
             AND newer.set_revision>draft.set_revision)
       AND effect.verdict='match' AND effect.candidate_set_id=candidate.id
       AND effect.result_revision=draft.set_revision
       AND selection.result_revision=draft.set_revision
       AND selection.disposition_id=disposition.disposition_id
       AND disposition.outcome='selected'
       AND disposition.selected_choice_id=selection.selected_choice_id
       AND disposition.task_id=selection.task_id
       AND disposition.matrix_task_revision=selection.task_revision
       AND task.current_revision=selection.task_revision
       AND NOT receipt.payload_erased
       AND receipt.request_payload IS NOT NULL AND receipt.result_payload IS NOT NULL
       AND pg_catalog.jsonb_typeof(selection.mapped_nodes)='array'
       AND EXISTS (
           SELECT 1 FROM pg_catalog.jsonb_array_elements(selection.mapped_nodes) AS node
           WHERE node->>'node_id'=context.work_node_id::text
             AND node->>'node_revision'=context.work_node_revision::text)
       AND pg_catalog.jsonb_typeof(draft.payload->'nodes')='array'
       AND EXISTS (
           SELECT 1 FROM pg_catalog.jsonb_array_elements(draft.payload->'nodes') AS node
           WHERE node->>'kind'='work'
             AND node->>'id'=context.work_node_id::text
             AND node->>'revision'=context.work_node_revision::text)
       AND NOT session.revoked AND NOT host.revoked
       AND actor.id=opportunity.authorized_actor_id AND actor.role='owner'
     FOR SHARE OF context,candidate,snapshot,review,draft,effect,selection,disposition,task,receipt,session,host,actor,membership;
    IF current_path IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'pipeline dispatch context is stale'
            USING ERRCODE = '42501';
    END IF;
    RETURN NEW;
END
$pipeline_dispatch$;

CREATE TRIGGER advisory_dispatch_pipeline_guard
    BEFORE INSERT OR UPDATE ON advisory_dispatch FOR EACH ROW
    EXECUTE FUNCTION pipeline_advice_dispatch_guard();
REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_dispatch_guard() FROM PUBLIC;
