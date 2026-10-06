-- Guarded Matrix advice is evidence about owner-authored choices. It cannot
-- select a choice. The agent's later disposition is a separate append-only fact.
-- The sentinel is outside the 64-hex digest domain, so this key makes a NULL
-- (no-choice) binding compare exactly in composite foreign keys.
ALTER TABLE advisory_opportunity
    ADD COLUMN matrix_choice_binding_key text GENERATED ALWAYS AS
        (COALESCE(matrix_choice_set_digest, 'no-choice')) STORED,
    ADD CONSTRAINT advisory_opportunity_matrix_exact_binding_unique UNIQUE
        (tenant_id, workspace_id, id, work_item_id, matrix_task_revision,
         matrix_choice_binding_key);

CREATE TABLE advisory_matrix_advice (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    opportunity_id uuid NOT NULL,
    task_id uuid NOT NULL,
    matrix_task_revision bigint NOT NULL,
    matrix_choice_set_digest text NOT NULL,
    advice_id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
    dispatch_id uuid NOT NULL,
    kind text NOT NULL,
    ranked_choice_ids jsonb,
    reason text,
    advice_digest text NOT NULL,
    provider_profile_ref text NOT NULL,
    model_configuration jsonb NOT NULL,
    response_payload_sha256 text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id, workspace_id, advice_id),
    CONSTRAINT advisory_matrix_advice_opportunity_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id),
    CONSTRAINT advisory_matrix_advice_dispatch_unique UNIQUE
        (tenant_id, workspace_id, dispatch_id),
    CONSTRAINT advisory_matrix_advice_binding_unique UNIQUE
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest, advice_id),
    CONSTRAINT advisory_matrix_advice_digest_check CHECK (
        matrix_choice_set_digest ~ '^[0-9a-f]{64}$'
        AND advice_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT advisory_matrix_advice_revision_check CHECK (matrix_task_revision >= 1),
    CONSTRAINT advisory_matrix_advice_kind_check CHECK (
        (kind = 'ranked' AND ranked_choice_ids IS NOT NULL
         AND pg_catalog.jsonb_typeof(ranked_choice_ids) = 'array'
         AND pg_catalog.jsonb_array_length(ranked_choice_ids) > 0)
        OR (kind IN ('abstained', 'rejected') AND ranked_choice_ids IS NULL)),
    CONSTRAINT advisory_matrix_advice_reason_check CHECK (
        reason IS NULL OR pg_catalog.btrim(reason) <> ''),
    CONSTRAINT advisory_matrix_advice_opportunity_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, task_id,
         matrix_task_revision, matrix_choice_set_digest)
        REFERENCES advisory_opportunity
        (tenant_id, workspace_id, id, work_item_id,
         matrix_task_revision, matrix_choice_binding_key),
    CONSTRAINT advisory_matrix_advice_dispatch_fk FOREIGN KEY
        (tenant_id, workspace_id, opportunity_id, dispatch_id)
        REFERENCES advisory_dispatch
        (tenant_id, workspace_id, opportunity_id, id),
    CONSTRAINT advisory_matrix_advice_revision_fk FOREIGN KEY
        (tenant_id, workspace_id, task_id, matrix_task_revision,
         matrix_choice_set_digest)
        REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision, choice_set_digest),
    CONSTRAINT advisory_matrix_advice_provider_profile_ref_check CHECK (
        pg_catalog.btrim(provider_profile_ref) <> ''
        AND pg_catalog.btrim(provider_profile_ref) = provider_profile_ref
        AND pg_catalog.length(provider_profile_ref) <= 256
    ),
    CONSTRAINT advisory_matrix_advice_model_configuration_check CHECK (
        pg_catalog.jsonb_typeof(model_configuration) = 'object'
        AND model_configuration ? 'model'
        AND pg_catalog.jsonb_typeof(model_configuration->'model') = 'string'
        AND pg_catalog.btrim(model_configuration->>'model') <> ''
        AND pg_catalog.btrim(model_configuration->>'model') = model_configuration->>'model'
        AND pg_catalog.length(model_configuration->>'model') <= 256
        AND model_configuration - 'model' = '{}'::jsonb
    ),
    CONSTRAINT advisory_matrix_advice_response_payload_sha256_check CHECK (
        response_payload_sha256 ~ '^[0-9a-f]{64}$'
    )
);

ALTER TABLE advisory_matrix_advice ENABLE ROW LEVEL SECURITY;
ALTER TABLE advisory_matrix_advice FORCE ROW LEVEL SECURITY;
CREATE POLICY advisory_matrix_advice_tenant_scope ON advisory_matrix_advice
    USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_matrix_advice'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
    WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='advisory_matrix_advice'::regclass))
        OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_advice FROM PUBLIC;
