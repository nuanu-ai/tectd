-- Additive WP5 mutation receipt/checkpoint surface. Legacy snapshot writes remain
-- authoritative and are intentionally untouched by this migration.
CREATE TABLE scope_candidate_delta_receipts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    request_payload jsonb NOT NULL,
    expected_revision bigint NOT NULL,
    from_revision bigint NOT NULL,
    to_revision bigint NOT NULL,
    status text NOT NULL DEFAULT 'committed',
    stale_reasons jsonb NOT NULL DEFAULT '[]'::jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, idempotency_key),
    CONSTRAINT scope_candidate_delta_receipts_set_fk FOREIGN KEY
        (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id),
    CONSTRAINT scope_candidate_delta_receipts_key_check CHECK
        (pg_catalog.length(idempotency_key) BETWEEN 1 AND 128),
    CONSTRAINT scope_candidate_delta_receipts_payload_check CHECK
        (pg_catalog.jsonb_typeof(request_payload) = 'object'),
    CONSTRAINT scope_candidate_delta_receipts_revision_check CHECK
        (expected_revision >= 1 AND from_revision >= 1 AND to_revision >= from_revision),
    CONSTRAINT scope_candidate_delta_receipts_status_check CHECK
        (status IN ('committed', 'replayed', 'rejected')),
    CONSTRAINT scope_candidate_delta_receipts_stale_check CHECK
        (pg_catalog.jsonb_typeof(stale_reasons) = 'array')
);

CREATE INDEX scope_candidate_delta_receipts_status_idx
    ON scope_candidate_delta_receipts
       (tenant_id, workspace_id, candidate_set_id, to_revision, created_at);

CREATE TABLE scope_candidate_delta_operations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    operation_index integer NOT NULL,
    operation jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, idempotency_key, operation_index),
    FOREIGN KEY (tenant_id, workspace_id, candidate_set_id, idempotency_key)
        REFERENCES scope_candidate_delta_receipts
        (tenant_id, workspace_id, candidate_set_id, idempotency_key)
        ON DELETE CASCADE,
    CONSTRAINT scope_candidate_delta_operations_index_check CHECK
        (operation_index >= 0 AND operation_index < 100),
    CONSTRAINT scope_candidate_delta_operations_payload_check CHECK
        (pg_catalog.jsonb_typeof(operation) = 'object')
);

CREATE TABLE scope_candidate_delta_candidates (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    candidate_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    payload jsonb NOT NULL,
    deleted boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, candidate_id),
    FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id),
    CHECK (revision >= 1 AND pg_catalog.jsonb_typeof(payload) = 'object')
);

CREATE TABLE scope_candidate_delta_goals (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    revision bigint NOT NULL DEFAULT 1,
    payload jsonb NOT NULL,
    deleted boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, goal_id),
    FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id),
    CHECK (revision >= 1 AND pg_catalog.jsonb_typeof(payload) = 'object')
);

CREATE TABLE scope_candidate_delta_coverage (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    candidate_id uuid NOT NULL,
    goal_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, candidate_id, goal_id),
    FOREIGN KEY (tenant_id, workspace_id, candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id, workspace_id, id)
);

ALTER TABLE scope_candidate_delta_receipts ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_receipts FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_operations ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_operations FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_candidates ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_candidates FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_goals ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_goals FORCE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_coverage ENABLE ROW LEVEL SECURITY;
ALTER TABLE scope_candidate_delta_coverage FORCE ROW LEVEL SECURITY;

CREATE POLICY scope_candidate_delta_receipts_tenant_scope
    ON scope_candidate_delta_receipts
    USING (
        CURRENT_USER=pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='scope_candidate_delta_receipts'::regclass)
        )
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
    )
    WITH CHECK (
        CURRENT_USER=pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='scope_candidate_delta_receipts'::regclass)
        )
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
    );

CREATE POLICY scope_candidate_delta_operations_tenant_scope
    ON scope_candidate_delta_operations
    USING (
        CURRENT_USER=pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='scope_candidate_delta_operations'::regclass)
        )
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
    )
    WITH CHECK (
        CURRENT_USER=pg_catalog.pg_get_userbyid(
            (SELECT relowner FROM pg_catalog.pg_class
             WHERE oid='scope_candidate_delta_operations'::regclass)
        )
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid
    );

CREATE POLICY scope_candidate_delta_candidates_tenant_scope ON scope_candidate_delta_candidates
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_delta_goals_tenant_scope ON scope_candidate_delta_goals
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY scope_candidate_delta_coverage_tenant_scope ON scope_candidate_delta_coverage
    USING (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

CREATE TRIGGER scope_candidate_delta_receipts_created_at_immutable
    BEFORE UPDATE ON scope_candidate_delta_receipts
    FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE scope_candidate_delta_receipts FROM PUBLIC;
REVOKE ALL PRIVILEGES ON TABLE scope_candidate_delta_operations FROM PUBLIC;
REVOKE ALL PRIVILEGES ON TABLE scope_candidate_delta_candidates, scope_candidate_delta_goals, scope_candidate_delta_coverage FROM PUBLIC;
