-- A selected Matrix disposition is provenance for a real native draft save,
-- never a draft or caller effect by itself. Multiple explicit save receipts may
-- refer to one disposition; each receipt has at most one immutable link.
CREATE TABLE matrix_planning_selection_links (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    caller_request_id uuid NOT NULL,
    scope_id uuid NOT NULL,
    disposition_id uuid NOT NULL,
    task_id uuid NOT NULL,
    task_revision bigint NOT NULL,
    selected_choice_id text NOT NULL,
    input_digest text NOT NULL,
    choice_set_digest text NOT NULL,
    verification_digest text NOT NULL,
    evaluation_digest text NOT NULL,
    catalogue_version text NOT NULL,
    caller_principal_id uuid NOT NULL,
    caller_session_id uuid NOT NULL,
    result_revision bigint NOT NULL,
    operation text NOT NULL DEFAULT 'save_slice_draft'
        CHECK (operation = 'save_slice_draft'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, caller_request_id),
    CONSTRAINT matrix_planning_selection_caller_request_unique UNIQUE
        (tenant_id, workspace_id, caller_request_id),
    CONSTRAINT matrix_planning_selection_receipt_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id, operation, caller_request_id)
        REFERENCES native_planning_receipts
        (tenant_id, workspace_id, entity_id, operation, request_id),
    CONSTRAINT matrix_planning_selection_disposition_fk FOREIGN KEY
        (tenant_id, workspace_id, disposition_id)
        REFERENCES advisory_matrix_disposition
        (tenant_id, workspace_id, disposition_id),
    CONSTRAINT matrix_planning_selection_set_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id)
        REFERENCES slice_candidate_sets (tenant_id, workspace_id, id),
    CONSTRAINT matrix_planning_selection_scope_fk FOREIGN KEY
        (tenant_id, workspace_id, scope_id)
        REFERENCES native_scopes (tenant_id, workspace_id, id),
    CONSTRAINT matrix_planning_selection_caller_fk FOREIGN KEY
        (tenant_id, caller_principal_id) REFERENCES principals (tenant_id, id),
    CONSTRAINT matrix_planning_selection_caller_session_fk FOREIGN KEY
        (tenant_id, workspace_id, caller_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id),
    CONSTRAINT matrix_planning_selection_shape CHECK (
        task_revision >= 1 AND result_revision >= 1
        AND pg_catalog.btrim(selected_choice_id) <> ''
        AND pg_catalog.btrim(catalogue_version) <> ''
        AND input_digest ~ '^[0-9a-f]{64}$'
        AND choice_set_digest ~ '^[0-9a-f]{64}$'
        AND verification_digest ~ '^[0-9a-f]{64}$'
        AND evaluation_digest ~ '^[0-9a-f]{64}$')
);

CREATE INDEX matrix_planning_selection_disposition_idx ON matrix_planning_selection_links
    (tenant_id, workspace_id, disposition_id, created_at);

-- The runtime role cannot lock private identity rows. Recheck and lock the
-- caller's live session, host, principal and membership at INSERT.
CREATE FUNCTION matrix_planning_selection_require_active_owner() RETURNS trigger
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, public, pg_temp
AS $active_owner$
DECLARE authorized boolean; tenant_setting text;
BEGIN
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
    SELECT true INTO authorized
    FROM public.agent_sessions AS s
    JOIN public.hosts AS h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
    JOIN public.principals AS p ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id)
    JOIN public.memberships AS m
        ON (m.tenant_id,m.workspace_id,m.principal_id)=(s.tenant_id,s.workspace_id,p.id)
    WHERE s.tenant_id=NEW.tenant_id AND s.workspace_id=NEW.workspace_id
      AND s.id=NEW.caller_session_id AND h.principal_id=NEW.caller_principal_id
      AND NOT s.revoked AND NOT h.revoked AND p.role='owner'
    FOR SHARE OF s,h,p,m;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix planning link requires an active caller owner session'
            USING ERRCODE = '42501';
    END IF;
    -- The exact native receipt, draft, selected disposition and captured
    -- opportunity are evidence bindings, not a SQL re-composition of Matrix.
    SELECT true INTO authorized
    FROM public.slice_candidate_sets AS c
    JOIN public.native_planning_receipts AS receipt
      ON (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,
          receipt.operation,receipt.request_id)=
         (c.tenant_id,c.workspace_id,c.id,NEW.operation,NEW.caller_request_id)
    JOIN public.slice_candidate_drafts AS draft
      ON (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,draft.set_revision)=
         (c.tenant_id,c.workspace_id,c.id,NEW.result_revision)
    JOIN public.advisory_matrix_disposition AS disposition
      ON (disposition.tenant_id,disposition.workspace_id,disposition.disposition_id)=
         (c.tenant_id,c.workspace_id,NEW.disposition_id)
    JOIN public.advisory_opportunity AS opportunity
      ON (opportunity.tenant_id,opportunity.workspace_id,opportunity.id)=
         (disposition.tenant_id,disposition.workspace_id,disposition.opportunity_id)
    WHERE (c.tenant_id,c.workspace_id,c.id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.candidate_set_id)
      AND c.scope_id=NEW.scope_id AND c.revision=NEW.result_revision
      AND NOT receipt.payload_erased AND NOT draft.payload_erased
      AND receipt.request_payload IS NOT NULL AND receipt.result_payload IS NOT NULL
      AND draft.payload IS NOT NULL AND draft.payload=receipt.result_payload->'draft'
      AND receipt.request_payload->'request_id'=pg_catalog.to_jsonb(NEW.caller_request_id)
      AND receipt.request_payload->'candidate_set_id'=pg_catalog.to_jsonb(NEW.candidate_set_id)
      AND receipt.request_payload->'scope_id'=pg_catalog.to_jsonb(NEW.scope_id)
      AND receipt.result_payload#>'{candidate_set,revision}'=pg_catalog.to_jsonb(NEW.result_revision)
      AND (receipt.request_payload->'matrix_selection') - 'mapped_draft_node_indices'=
          pg_catalog.jsonb_build_object(
              'task_id',NEW.task_id,'task_revision',NEW.task_revision,
              'disposition_id',NEW.disposition_id,'selected_choice_id',NEW.selected_choice_id,
              'expected_input_digest',NEW.input_digest,
              'expected_choice_set_digest',NEW.choice_set_digest,
              'expected_verification_digest',NEW.verification_digest)
      AND disposition.task_id=NEW.task_id
      AND disposition.matrix_task_revision=NEW.task_revision
      AND disposition.matrix_choice_set_digest=NEW.choice_set_digest
      AND disposition.outcome='selected' AND disposition.selected_choice_id=NEW.selected_choice_id
      AND disposition.actor_id=NEW.caller_principal_id
      AND opportunity.work_item_kind='matrix_task' AND opportunity.work_item_id=NEW.task_id
      AND opportunity.matrix_task_revision=NEW.task_revision
      AND opportunity.matrix_choice_set_digest=NEW.choice_set_digest
      AND opportunity.matrix_verification_digest=NEW.verification_digest
      AND opportunity.material_digest=NEW.evaluation_digest
      AND opportunity.authorized_actor_id=disposition.actor_id
      AND opportunity.session_id=disposition.session_id
    FOR SHARE OF c,receipt,draft,disposition,opportunity;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'Matrix selection requires exact saved receipt and material binding'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$active_owner$;

CREATE TRIGGER matrix_planning_selection_active_owner
    BEFORE INSERT ON matrix_planning_selection_links
    FOR EACH ROW EXECUTE FUNCTION matrix_planning_selection_require_active_owner();
CREATE TRIGGER matrix_planning_selection_immutable
    BEFORE UPDATE OR DELETE ON matrix_planning_selection_links
    FOR EACH ROW EXECUTE FUNCTION matrix_verification_deny_mutation();

ALTER TABLE matrix_planning_selection_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE matrix_planning_selection_links FORCE ROW LEVEL SECURITY;
CREATE POLICY matrix_planning_selection_links_tenant_scope ON matrix_planning_selection_links
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='matrix_planning_selection_links'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='matrix_planning_selection_links'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE matrix_planning_selection_links FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION matrix_planning_selection_require_active_owner() FROM PUBLIC;
