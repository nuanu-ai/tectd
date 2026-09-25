-- Slice 04: immutable source/plan binding and one-use review dispatch ledger.
-- An authoritative caller must supply an explicit obligation graph. A missing
-- binding is a refusal to prepare, never an inferred empty obligation set.
CREATE TABLE scope_anti_bloat_bindings (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    candidate_set_revision bigint NOT NULL CHECK (candidate_set_revision >= 1),
    source_digest text NOT NULL CHECK (source_digest ~ '^[0-9a-f]{64}$'),
    dependency_digest text NOT NULL CHECK (dependency_digest ~ '^[0-9a-f]{64}$'),
    obligation_links jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(obligation_links) = 'array'),
    mandatory_policy_obligation_ids jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(mandatory_policy_obligation_ids) = 'array'),
    provenance text NOT NULL CHECK (pg_catalog.length(pg_catalog.btrim(provenance)) > 0),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,candidate_set_id,candidate_set_revision),
    CONSTRAINT scope_anti_bloat_binding_manifest_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest)
        REFERENCES advisory_scope_manifest
        (tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest),
    CONSTRAINT scope_anti_bloat_binding_set_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id,workspace_id,id)
);

CREATE TABLE scope_anti_bloat_reviews (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    review_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    candidate_set_revision bigint NOT NULL,
    actor_id uuid NOT NULL,
    input_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(input_payload) = 'object'),
    review_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(review_payload) = 'object'),
    state text NOT NULL CHECK (state IN ('disabled','skipped','no_eligible','prepared','sending','ranked','send_unknown')),
    eligible_ids jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(eligible_ids) = 'array'),
    ranked_ids jsonb CHECK (ranked_ids IS NULL OR pg_catalog.jsonb_typeof(ranked_ids) = 'array'),
    request_bytes bytea,
    request_sha256 text CHECK (request_sha256 IS NULL OR request_sha256 ~ '^[0-9a-f]{64}$'),
    raw_response bytea,
    response_sha256 text CHECK (response_sha256 IS NULL OR response_sha256 ~ '^[0-9a-f]{64}$'),
    send_started_at timestamptz,
    sealed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,review_id),
    CONSTRAINT scope_anti_bloat_review_binding_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,candidate_set_revision)
        REFERENCES scope_anti_bloat_bindings
        (tenant_id,workspace_id,candidate_set_id,candidate_set_revision),
    CONSTRAINT scope_anti_bloat_review_actor_fk FOREIGN KEY (tenant_id,actor_id)
        REFERENCES principals (tenant_id,id),
    CONSTRAINT scope_anti_bloat_review_send_check CHECK
        ((state IN ('sending','ranked','send_unknown')) = (send_started_at IS NOT NULL)),
    CONSTRAINT scope_anti_bloat_review_request_check CHECK
        ((request_bytes IS NULL AND request_sha256 IS NULL) OR
         (request_bytes IS NOT NULL AND request_sha256 IS NOT NULL))
);

CREATE TABLE scope_anti_bloat_caller_links (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    review_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    finding_id text NOT NULL CHECK (finding_id ~ '^[0-9a-f]{64}$'),
    disposition text NOT NULL CHECK (disposition = 'narrow'),
    preservation_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(preservation_payload) = 'object'),
    delta_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(delta_payload) = 'object'),
    caller_idempotency_key text NOT NULL,
    caller_receipt jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(caller_receipt) = 'object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,review_id),
    CONSTRAINT scope_anti_bloat_caller_review_fk FOREIGN KEY (tenant_id,workspace_id,review_id)
        REFERENCES scope_anti_bloat_reviews (tenant_id,workspace_id,review_id),
    CONSTRAINT scope_anti_bloat_caller_delta_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,caller_idempotency_key)
        REFERENCES scope_candidate_delta_receipts
        (tenant_id,workspace_id,candidate_set_id,idempotency_key)
);

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY[
        'scope_anti_bloat_bindings','scope_anti_bloat_reviews','scope_anti_bloat_caller_links'
    ] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',relation_name||'_tenant_scope',relation_name,relation_name,relation_name);
    END LOOP;
END
$policy$;

REVOKE ALL PRIVILEGES ON TABLE
    scope_anti_bloat_bindings,scope_anti_bloat_reviews,scope_anti_bloat_caller_links
FROM PUBLIC;
