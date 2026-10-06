-- Independent observation of the actual native anti-bloat saved draft.
CREATE TABLE scope_anti_bloat_preservation_attestations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    request_id uuid NOT NULL,
    review_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    from_revision bigint NOT NULL,
    to_revision bigint NOT NULL,
    caller_operation text NOT NULL DEFAULT 'anti_bloat_narrow' CHECK (caller_operation='anti_bloat_narrow'),
    caller_request_id uuid NOT NULL,
    verifier_principal_id uuid NOT NULL,
    verifier_session_id uuid NOT NULL,
    verdict text NOT NULL CHECK (verdict IN ('pass','fail','unknown')),
    reason text NOT NULL CHECK (reason IN
        ('full_graph_preserved','graph_or_receipt_mismatch','source_evidence_unavailable')),
    evidence_digest text NOT NULL CHECK (evidence_digest ~ '^[0-9a-f]{64}$'),
    source_digest text NOT NULL CHECK (source_digest ~ '^[0-9a-f]{64}$'),
    before_material_digest text NOT NULL CHECK (before_material_digest ~ '^[0-9a-f]{64}$'),
    after_material_digest text NOT NULL CHECK (after_material_digest ~ '^[0-9a-f]{64}$'),
    verified_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,request_id),
    CHECK (to_revision=from_revision+1),
    CHECK ((verdict='pass' AND reason='full_graph_preserved') OR
           (verdict='fail' AND reason='graph_or_receipt_mismatch') OR
           (verdict='unknown' AND reason='source_evidence_unavailable')),
    FOREIGN KEY (tenant_id,workspace_id,review_id)
        REFERENCES scope_anti_bloat_caller_links (tenant_id,workspace_id,review_id),
    FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,caller_operation,caller_request_id,to_revision)
        REFERENCES scope_candidate_receipts
        (tenant_id,workspace_id,candidate_set_id,operation,request_id,result_revision),
    FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,from_revision)
        REFERENCES scope_candidate_drafts (tenant_id,workspace_id,candidate_set_id,set_revision),
    FOREIGN KEY (tenant_id,workspace_id,candidate_set_id,to_revision)
        REFERENCES scope_candidate_drafts (tenant_id,workspace_id,candidate_set_id,set_revision),
    FOREIGN KEY (tenant_id,verifier_principal_id)
        REFERENCES principals (tenant_id,id),
    FOREIGN KEY (tenant_id,workspace_id,verifier_session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);
CREATE INDEX scope_anti_bloat_preservation_review_idx
    ON scope_anti_bloat_preservation_attestations
    (tenant_id,workspace_id,review_id,verified_at DESC);

CREATE FUNCTION scope_anti_bloat_preservation_attestation_guard() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $guard$
DECLARE authorized boolean;
BEGIN
    IF NEW.tenant_id IS DISTINCT FROM NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid THEN
        RAISE EXCEPTION 'anti-bloat attestation tenant mismatch' USING ERRCODE='42501';
    END IF;
    SELECT true INTO authorized
    FROM public.scope_anti_bloat_caller_links l
    JOIN public.scope_anti_bloat_reviews r ON (r.tenant_id,r.workspace_id,r.review_id)=
        (l.tenant_id,l.workspace_id,l.review_id)
    JOIN public.scope_anti_bloat_bindings b ON (b.tenant_id,b.workspace_id,b.candidate_set_id,b.candidate_set_revision)=
        (r.tenant_id,r.workspace_id,r.candidate_set_id,r.candidate_set_revision)
    JOIN public.advisory_scope_caller_link c ON (c.tenant_id,c.workspace_id,c.link_id)=
        (b.tenant_id,b.workspace_id,b.selected_caller_link_id)
    JOIN public.advisory_scope_disposition d ON (d.tenant_id,d.workspace_id,d.disposition_id)=
        (c.tenant_id,c.workspace_id,c.disposition_id)
    JOIN public.scope_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)=
        (l.tenant_id,l.workspace_id,l.candidate_set_id)
    JOIN public.agent_sessions vs ON (vs.tenant_id,vs.workspace_id,vs.id)=
        (l.tenant_id,l.workspace_id,NEW.verifier_session_id)
    JOIN public.hosts vh ON (vh.tenant_id,vh.id)=(vs.tenant_id,vs.host_id)
    JOIN public.principals vp ON (vp.tenant_id,vp.id)=(vh.tenant_id,vh.principal_id)
    JOIN public.memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)=
        (l.tenant_id,l.workspace_id,vp.id)
    WHERE (l.tenant_id,l.workspace_id,l.review_id)=
          (NEW.tenant_id,NEW.workspace_id,NEW.review_id)
      AND (l.candidate_set_id,l.from_revision,l.to_revision,l.caller_operation,l.caller_request_id)=
          (NEW.candidate_set_id,NEW.from_revision,NEW.to_revision,NEW.caller_operation,NEW.caller_request_id)
      AND s.revision=NEW.to_revision AND b.selected_draft_revision=NEW.from_revision
      AND l.source_digest=NEW.source_digest
      AND l.before_material_digest=NEW.before_material_digest
      AND l.after_material_digest=NEW.after_material_digest
      AND vp.id=NEW.verifier_principal_id AND vp.role='verifier'
      AND NOT vs.revoked AND NOT vh.revoked
      AND vp.id<>r.actor_id AND vp.id<>c.actor_id AND vp.id<>d.actor_id
      AND vs.id<>c.session_id
    FOR SHARE OF l,r,b,c,d,s,vs,vh,vp,m;
    IF authorized IS DISTINCT FROM true THEN
        RAISE EXCEPTION 'anti-bloat attestation requires independent exact saved draft'
            USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END
$guard$;
REVOKE ALL PRIVILEGES ON FUNCTION scope_anti_bloat_preservation_attestation_guard() FROM PUBLIC;
CREATE TRIGGER scope_anti_bloat_preservation_attestation_insert_guard
    BEFORE INSERT ON scope_anti_bloat_preservation_attestations FOR EACH ROW
    EXECUTE FUNCTION scope_anti_bloat_preservation_attestation_guard();
CREATE TRIGGER scope_anti_bloat_preservation_attestation_immutable
    BEFORE UPDATE OR DELETE ON scope_anti_bloat_preservation_attestations FOR EACH ROW
    EXECUTE FUNCTION scope_anti_bloat_immutable();
ALTER TABLE scope_anti_bloat_preservation_attestations ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_anti_bloat_preservation_attestations FORCE ROW LEVEL SECURITY;
CREATE POLICY scope_anti_bloat_preservation_tenant_scope ON scope_anti_bloat_preservation_attestations
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_preservation_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='scope_anti_bloat_preservation_attestations'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_preservation_attestations FROM PUBLIC;
