-- Server-computed, append-only observations of a selected candidate save.
-- Target foreign keys are intentionally absent so missing target records can
-- produce durable failed observations rather than disappearing as errors.
CREATE TABLE advisory_scope_selected_save_observation (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL,
    observation_id uuid NOT NULL, request_id uuid NOT NULL,
    opportunity_id uuid NOT NULL, candidate_set_id uuid NOT NULL,
    caller_link_id uuid NOT NULL, caller_receipt_request_id uuid NOT NULL,
    target_revision bigint NOT NULL,
    actor_id uuid NOT NULL, session_id uuid NOT NULL,
    request_fingerprint text NOT NULL,
    status text NOT NULL, reason_codes text[] NOT NULL,
    evidence_digest text NOT NULL, evidence_payload jsonb NOT NULL,
    qualification text NOT NULL DEFAULT 'unresolved',
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,observation_id),
    CONSTRAINT advisory_scope_selected_save_observation_request_unique
        UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT advisory_scope_selected_save_observation_revision_check CHECK (target_revision>=1),
    CONSTRAINT advisory_scope_selected_save_observation_status_check CHECK (status IN ('passed','failed')),
    CONSTRAINT advisory_scope_selected_save_observation_qualification_check CHECK (qualification='unresolved'),
    CONSTRAINT advisory_scope_selected_save_observation_digest_check CHECK
        (request_fingerprint ~ '^[0-9a-f]{64}$' AND evidence_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_scope_selected_save_observation_payload_check CHECK
        (pg_catalog.jsonb_typeof(evidence_payload)='object'),
    CONSTRAINT advisory_scope_selected_save_observation_actor_fk FOREIGN KEY (tenant_id,actor_id)
        REFERENCES principals (tenant_id,id),
    CONSTRAINT advisory_scope_selected_save_observation_session_fk FOREIGN KEY (tenant_id,workspace_id,session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);

ALTER TABLE advisory_scope_selected_save_observation ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_scope_selected_save_observation FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_scope_selected_save_observation_tenant_scope
    ON advisory_scope_selected_save_observation
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_scope_selected_save_observation'::regclass))
           OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_scope_selected_save_observation'::regclass))
           OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_scope_selected_save_observation FROM PUBLIC;
