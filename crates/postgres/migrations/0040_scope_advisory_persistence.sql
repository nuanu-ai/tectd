-- Slice 01 Packet B: seven immutable aggregate records. Runtime writes only
-- through the validated repository; provider effects and budget reservation
-- belong to later packets.

ALTER TABLE advisory_opportunity ADD CONSTRAINT advisory_opportunity_scope_material_unique
    UNIQUE (tenant_id,workspace_id,id,scope_id,config_revision,material_digest);
ALTER TABLE advisory_dispatch ADD CONSTRAINT advisory_dispatch_material_unique
    UNIQUE (tenant_id,workspace_id,opportunity_id,id,material_digest);

CREATE TABLE advisory_scope_source_snapshot (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, config_revision bigint NOT NULL, opportunity_material_digest text NOT NULL,
    candidate_set_id uuid NOT NULL, candidate_set_revision bigint NOT NULL, snapshot_id uuid NOT NULL,
    source_digest text NOT NULL, aggregate_schema text NOT NULL, aggregate_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,opportunity_id),
    CONSTRAINT advisory_scope_source_identity_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest),
    CONSTRAINT advisory_scope_source_revision_check CHECK (candidate_set_revision>=1),
    CONSTRAINT advisory_scope_source_digest_check CHECK
        (opportunity_material_digest ~ '^[0-9a-f]{64}$' AND source_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_scope_source_schema_check CHECK (aggregate_schema='tect.scope-source-obligations/1'),
    CONSTRAINT advisory_scope_source_payload_check CHECK (pg_catalog.jsonb_typeof(aggregate_payload)='object'),
    CONSTRAINT advisory_scope_source_opportunity_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,config_revision,opportunity_material_digest)
        REFERENCES advisory_opportunity (tenant_id,workspace_id,id,scope_id,config_revision,material_digest),
    CONSTRAINT advisory_scope_source_case_fk FOREIGN KEY (tenant_id,workspace_id,case_id)
        REFERENCES native_scopes (tenant_id,workspace_id,id),
    CONSTRAINT advisory_scope_source_candidate_fk FOREIGN KEY (tenant_id,workspace_id,candidate_set_id)
        REFERENCES scope_candidate_sets (tenant_id,workspace_id,id),
    CONSTRAINT advisory_scope_source_snapshot_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,snapshot_id)
        REFERENCES scope_candidate_snapshots (tenant_id,workspace_id,candidate_set_id,id)
);

CREATE TABLE advisory_scope_manifest (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, source_digest text NOT NULL, constructor_id text NOT NULL,
    constructor_version text NOT NULL, constructor_digest text NOT NULL,
    baseline_alternative_id text NOT NULL, eligible_set_digest text NOT NULL, whole_set_digest text NOT NULL,
    aggregate_schema text NOT NULL, aggregate_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,opportunity_id),
    CONSTRAINT advisory_scope_manifest_identity_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest,whole_set_digest,eligible_set_digest),
    CONSTRAINT advisory_scope_manifest_digest_check CHECK
        (source_digest ~ '^[0-9a-f]{64}$' AND constructor_digest ~ '^[0-9a-f]{64}$'
         AND baseline_alternative_id ~ '^[0-9a-f]{64}$' AND eligible_set_digest ~ '^[0-9a-f]{64}$'
         AND whole_set_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_scope_manifest_schema_check CHECK (aggregate_schema='tect.scope-constructor-manifest/2'),
    CONSTRAINT advisory_scope_manifest_payload_check CHECK (pg_catalog.jsonb_typeof(aggregate_payload)='object'),
    CONSTRAINT advisory_scope_manifest_source_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest)
        REFERENCES advisory_scope_source_snapshot
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest)
);

CREATE TABLE advisory_scope_advice (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, advice_id text NOT NULL, dispatch_id uuid NOT NULL,
    dispatch_material_digest text NOT NULL, config_revision bigint NOT NULL,
    source_digest text NOT NULL, manifest_digest text NOT NULL, eligible_set_digest text NOT NULL,
    request_digest text NOT NULL, normalized_answers_digest text NOT NULL,
    aggregate_schema text NOT NULL, aggregate_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,advice_id),
    CONSTRAINT advisory_scope_advice_opportunity_unique UNIQUE (tenant_id,workspace_id,opportunity_id),
    CONSTRAINT advisory_scope_advice_case_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id),
    CONSTRAINT advisory_scope_advice_digest_check CHECK
        (advice_id ~ '^[0-9a-f]{64}$' AND dispatch_material_digest ~ '^[0-9a-f]{64}$'
         AND source_digest ~ '^[0-9a-f]{64}$' AND manifest_digest ~ '^[0-9a-f]{64}$'
         AND eligible_set_digest ~ '^[0-9a-f]{64}$' AND request_digest ~ '^[0-9a-f]{64}$'
         AND normalized_answers_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_scope_advice_schema_check CHECK (aggregate_schema='tect.guarded-scope-advice/1'),
    CONSTRAINT advisory_scope_advice_payload_check CHECK (pg_catalog.jsonb_typeof(aggregate_payload)='object'),
    CONSTRAINT advisory_scope_advice_manifest_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest,manifest_digest,eligible_set_digest)
        REFERENCES advisory_scope_manifest
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest,whole_set_digest,eligible_set_digest),
    CONSTRAINT advisory_scope_advice_dispatch_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,dispatch_id,dispatch_material_digest)
        REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id,material_digest)
);

CREATE TABLE advisory_scope_disposition (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, disposition_id uuid NOT NULL, request_id uuid NOT NULL,
    advice_id text NOT NULL, revision bigint NOT NULL, predecessor_id uuid,
    predecessor_revision bigint, actor_id uuid NOT NULL, session_id uuid NOT NULL,
    action text NOT NULL, selected_alternative_id text,
    aggregate_schema text NOT NULL, aggregate_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,disposition_id),
    CONSTRAINT advisory_scope_disposition_request_unique UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT advisory_scope_disposition_revision_unique UNIQUE (tenant_id,workspace_id,advice_id,revision),
    CONSTRAINT advisory_scope_disposition_chain_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id,disposition_id,revision),
    CONSTRAINT advisory_scope_disposition_one_successor_unique UNIQUE
        (tenant_id,workspace_id,advice_id,predecessor_id),
    CONSTRAINT advisory_scope_disposition_revision_check CHECK
        ((revision=1 AND predecessor_id IS NULL AND predecessor_revision IS NULL)
         OR (revision>1 AND predecessor_id IS NOT NULL AND predecessor_revision=revision-1)),
    CONSTRAINT advisory_scope_disposition_action_check CHECK
        (action IN ('accept','reject_all','supersede_with_deterministic_choice')),
    CONSTRAINT advisory_scope_disposition_schema_check CHECK (aggregate_schema='tect.scope-disposition-revision/1'),
    CONSTRAINT advisory_scope_disposition_payload_check CHECK (pg_catalog.jsonb_typeof(aggregate_payload)='object'),
    CONSTRAINT advisory_scope_disposition_advice_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id)
        REFERENCES advisory_scope_advice (tenant_id,workspace_id,opportunity_id,case_id,advice_id),
    CONSTRAINT advisory_scope_disposition_predecessor_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id,predecessor_id,predecessor_revision)
        REFERENCES advisory_scope_disposition
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id,disposition_id,revision),
    CONSTRAINT advisory_scope_disposition_actor_fk FOREIGN KEY (tenant_id,actor_id)
        REFERENCES principals (tenant_id,id),
    CONSTRAINT advisory_scope_disposition_session_fk FOREIGN KEY (tenant_id,workspace_id,session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);

CREATE TABLE advisory_scope_preservation_receipt (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, receipt_id uuid NOT NULL, request_id uuid NOT NULL,
    advice_id text NOT NULL, disposition_id uuid NOT NULL, disposition_revision bigint NOT NULL,
    source_digest text NOT NULL, manifest_digest text NOT NULL, eligible_set_digest text NOT NULL,
    observed_candidate_set_revision bigint NOT NULL, status text NOT NULL,
    aggregate_schema text NOT NULL, observation_payload jsonb NOT NULL, result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,receipt_id),
    CONSTRAINT advisory_scope_preservation_request_unique UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT advisory_scope_preservation_case_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,receipt_id,status),
    CONSTRAINT advisory_scope_preservation_revision_check CHECK
        (disposition_revision>=1 AND observed_candidate_set_revision>=1),
    CONSTRAINT advisory_scope_preservation_status_check CHECK (status IN ('passed','failed')),
    CONSTRAINT advisory_scope_preservation_schema_check CHECK (aggregate_schema='tect.scope-preservation/1'),
    CONSTRAINT advisory_scope_preservation_payload_check CHECK
        (pg_catalog.jsonb_typeof(observation_payload)='object' AND pg_catalog.jsonb_typeof(result_payload)='object'),
    CONSTRAINT advisory_scope_preservation_disposition_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id,disposition_id,disposition_revision)
        REFERENCES advisory_scope_disposition
        (tenant_id,workspace_id,opportunity_id,case_id,advice_id,disposition_id,revision),
    CONSTRAINT advisory_scope_preservation_manifest_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest,manifest_digest,eligible_set_digest)
        REFERENCES advisory_scope_manifest
        (tenant_id,workspace_id,opportunity_id,case_id,source_digest,whole_set_digest,eligible_set_digest)
);

CREATE TABLE advisory_scope_caller_link (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, link_id uuid NOT NULL, request_id uuid NOT NULL,
    disposition_id uuid NOT NULL, preservation_receipt_id uuid NOT NULL,
    preservation_status text NOT NULL DEFAULT 'passed', candidate_set_id uuid NOT NULL,
    caller_operation text NOT NULL, caller_request_id uuid NOT NULL, caller_result_revision bigint NOT NULL,
    actor_id uuid NOT NULL, session_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,link_id),
    CONSTRAINT advisory_scope_caller_request_unique UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT advisory_scope_caller_case_unique UNIQUE
        (tenant_id,workspace_id,opportunity_id,case_id,link_id,candidate_set_id,caller_result_revision),
    CONSTRAINT advisory_scope_caller_passed_check CHECK (preservation_status='passed'),
    CONSTRAINT advisory_scope_caller_revision_check CHECK (caller_result_revision>=1),
    CONSTRAINT advisory_scope_caller_preservation_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,preservation_receipt_id,preservation_status)
        REFERENCES advisory_scope_preservation_receipt
        (tenant_id,workspace_id,opportunity_id,case_id,receipt_id,status),
    CONSTRAINT advisory_scope_caller_receipt_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,caller_operation,caller_request_id)
        REFERENCES scope_candidate_receipts (tenant_id,workspace_id,candidate_set_id,operation,request_id),
    CONSTRAINT advisory_scope_caller_actor_fk FOREIGN KEY (tenant_id,actor_id)
        REFERENCES principals (tenant_id,id),
    CONSTRAINT advisory_scope_caller_session_fk FOREIGN KEY (tenant_id,workspace_id,session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);

CREATE TABLE advisory_scope_verifier_receipt (
    tenant_id uuid NOT NULL, workspace_id uuid NOT NULL, opportunity_id uuid NOT NULL,
    case_id uuid NOT NULL, receipt_id uuid NOT NULL, request_id uuid NOT NULL,
    caller_link_id uuid NOT NULL, candidate_set_id uuid NOT NULL,
    actor_id uuid NOT NULL, session_id uuid NOT NULL, verified_revision bigint NOT NULL,
    verifier_digest text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,receipt_id),
    CONSTRAINT advisory_scope_verifier_request_unique UNIQUE (tenant_id,workspace_id,request_id),
    CONSTRAINT advisory_scope_verifier_revision_check CHECK (verified_revision>=1),
    CONSTRAINT advisory_scope_verifier_digest_check CHECK (verifier_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_scope_verifier_caller_fk FOREIGN KEY
        (tenant_id,workspace_id,opportunity_id,case_id,caller_link_id,candidate_set_id,verified_revision)
        REFERENCES advisory_scope_caller_link
        (tenant_id,workspace_id,opportunity_id,case_id,link_id,candidate_set_id,caller_result_revision),
    CONSTRAINT advisory_scope_verifier_actor_fk FOREIGN KEY (tenant_id,actor_id)
        REFERENCES principals (tenant_id,id),
    CONSTRAINT advisory_scope_verifier_session_fk FOREIGN KEY (tenant_id,workspace_id,session_id)
        REFERENCES agent_sessions (tenant_id,workspace_id,id)
);

CREATE INDEX advisory_scope_case_lookup_idx ON advisory_scope_source_snapshot
    (tenant_id,workspace_id,case_id,created_at,opportunity_id);
CREATE INDEX advisory_scope_disposition_lookup_idx ON advisory_scope_disposition
    (tenant_id,workspace_id,advice_id,revision DESC);
CREATE UNIQUE INDEX advisory_scope_disposition_one_root_unique ON advisory_scope_disposition
    (tenant_id,workspace_id,advice_id) WHERE predecessor_id IS NULL;
CREATE INDEX advisory_scope_caller_lookup_idx ON advisory_scope_caller_link
    (tenant_id,workspace_id,candidate_set_id,caller_operation,caller_request_id);

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY[
        'advisory_scope_source_snapshot','advisory_scope_manifest','advisory_scope_advice',
        'advisory_scope_disposition','advisory_scope_preservation_receipt',
        'advisory_scope_caller_link','advisory_scope_verifier_receipt'
    ] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',relation_name||'_tenant_scope',relation_name,relation_name,relation_name);
    END LOOP;
END
$policy$;

REVOKE ALL PRIVILEGES ON TABLE
    advisory_scope_source_snapshot,advisory_scope_manifest,advisory_scope_advice,
    advisory_scope_disposition,advisory_scope_preservation_receipt,
    advisory_scope_caller_link,advisory_scope_verifier_receipt
FROM PUBLIC;
