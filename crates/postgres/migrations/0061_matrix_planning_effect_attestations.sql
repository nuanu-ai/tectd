-- An independent verifier attests the effect of one selected Matrix save.
-- This records an observation only; neither verdict changes planning readiness.
CREATE TABLE matrix_planning_effect_attestations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    candidate_set_id uuid NOT NULL,
    caller_request_id uuid NOT NULL,
    operation text NOT NULL DEFAULT 'save_slice_draft'
        CHECK (operation = 'save_slice_draft'),
    result_revision bigint NOT NULL CHECK (result_revision >= 1),
    effect_digest text NOT NULL CHECK (effect_digest ~ '^[0-9a-f]{64}$'),
    verifier_principal_id uuid NOT NULL,
    verifier_session_id uuid NOT NULL,
    verdict text NOT NULL CHECK (verdict IN ('match', 'reject')),
    summary text NOT NULL CHECK (
        pg_catalog.length(pg_catalog.btrim(summary)) >= 1
        AND pg_catalog.length(summary) <= 4096),
    verifier_request_id uuid NOT NULL,
    verified_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, id),
    CONSTRAINT matrix_planning_effect_request_unique UNIQUE
        (tenant_id, workspace_id, verifier_request_id),
    CONSTRAINT matrix_planning_effect_link_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id, caller_request_id)
        REFERENCES matrix_planning_selection_links
        (tenant_id, workspace_id, candidate_set_id, caller_request_id),
    CONSTRAINT matrix_planning_effect_receipt_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id, operation, caller_request_id)
        REFERENCES native_planning_receipts
        (tenant_id, workspace_id, entity_id, operation, request_id),
    CONSTRAINT matrix_planning_effect_verifier_fk FOREIGN KEY
        (tenant_id, verifier_principal_id) REFERENCES principals (tenant_id, id),
    CONSTRAINT matrix_planning_effect_session_fk FOREIGN KEY
        (tenant_id, workspace_id, verifier_session_id)
        REFERENCES agent_sessions (tenant_id, workspace_id, id)
);

CREATE INDEX matrix_planning_effect_link_lookup_idx ON matrix_planning_effect_attestations
    (tenant_id, workspace_id, candidate_set_id, caller_request_id, verified_at DESC);

-- Recheck the current saved set and private identity rows while holding locks.
-- Legacy links have NULL mapped_nodes and cannot be attested. The application
-- verifies the canonical effect digest against the actual saved node payload.
CREATE FUNCTION matrix_planning_effect_require_active_verifier() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public
AS $active_verifier$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM
        NULLIF(pg_catalog.current_setting('tect.tenant_id', true), '')::uuid THEN
        RAISE EXCEPTION 'matrix planning effect tenant does not match session'
            USING ERRCODE = '42501';
    END IF;

    SELECT true INTO authorized
    FROM public.matrix_planning_selection_links AS l
    JOIN public.slice_candidate_sets AS c
      ON (c.tenant_id,c.workspace_id,c.id)=
         (l.tenant_id,l.workspace_id,l.candidate_set_id)
    JOIN public.native_planning_receipts AS receipt
      ON (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,
          receipt.operation,receipt.request_id)=
         (l.tenant_id,l.workspace_id,l.candidate_set_id,
          l.operation,l.caller_request_id)
    JOIN public.matrix_task_revisions AS r
      ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=
         (l.tenant_id,l.workspace_id,l.task_id,l.task_revision)
    JOIN public.agent_sessions AS s
      ON (s.tenant_id,s.workspace_id,s.id)=
         (l.tenant_id,l.workspace_id,NEW.verifier_session_id)
    JOIN public.hosts AS h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id)
    JOIN public.principals AS p
      ON (p.tenant_id,p.id)=(h.tenant_id,h.principal_id)
    JOIN public.memberships AS m
      ON (m.tenant_id,m.workspace_id,m.principal_id)=
         (l.tenant_id,l.workspace_id,p.id)
    WHERE l.tenant_id=NEW.tenant_id AND l.workspace_id=NEW.workspace_id
      AND l.candidate_set_id=NEW.candidate_set_id
      AND l.caller_request_id=NEW.caller_request_id
      AND l.operation=NEW.operation
      AND l.result_revision=NEW.result_revision
      AND c.revision=NEW.result_revision
      AND NOT receipt.payload_erased
      AND receipt.request_payload IS NOT NULL
      AND receipt.result_payload IS NOT NULL
      AND pg_catalog.jsonb_typeof(l.mapped_nodes)='array'
      AND pg_catalog.jsonb_array_length(l.mapped_nodes)>0
      AND p.id=NEW.verifier_principal_id AND p.role='verifier'
      AND p.id<>l.caller_principal_id
      AND p.id<>r.recorded_by_principal_id
      AND NOT s.revoked AND NOT h.revoked
    FOR SHARE OF l,c,receipt,r,s,h,p,m;

    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'matrix planning effect requires current save and independent verifier'
            USING ERRCODE = '42501';
    END IF;
    NEW.verified_at := pg_catalog.clock_timestamp();
    RETURN NEW;
END
$active_verifier$;

CREATE TRIGGER matrix_planning_effect_active_verifier
    BEFORE INSERT ON matrix_planning_effect_attestations
    FOR EACH ROW EXECUTE FUNCTION matrix_planning_effect_require_active_verifier();
CREATE TRIGGER matrix_planning_effect_immutable
    BEFORE UPDATE OR DELETE ON matrix_planning_effect_attestations
    FOR EACH ROW EXECUTE FUNCTION matrix_verification_deny_mutation();

ALTER TABLE matrix_planning_effect_attestations ENABLE ROW LEVEL SECURITY;
ALTER TABLE matrix_planning_effect_attestations FORCE ROW LEVEL SECURITY;
CREATE POLICY matrix_planning_effect_attestations_tenant_scope
    ON matrix_planning_effect_attestations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='matrix_planning_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='matrix_planning_effect_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE matrix_planning_effect_attestations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION matrix_planning_effect_require_active_verifier() FROM PUBLIC;
