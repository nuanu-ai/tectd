-- Durable, caller-reported native Slice pipeline orchestration.
CREATE TABLE slice_pipeline_runs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    scope_id uuid NOT NULL,
    slice_id uuid NOT NULL,
    slice_revision bigint NOT NULL CHECK (slice_revision >= 1),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    definition_kind text NOT NULL,
    definition_version text NOT NULL CHECK (pg_catalog.btrim(definition_version) <> ''),
    definition_digest text NOT NULL CHECK (pg_catalog.btrim(definition_digest) <> ''),
    definition jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(definition) = 'object'),
    delivery_mode text NOT NULL CHECK (delivery_mode IN ('whole','phasewise')),
    qualification_reason text NOT NULL CHECK (pg_catalog.btrim(qualification_reason) <> ''),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active','waiting_input','blocked','completed','escalated')),
    current_phase_id text,
    current_phase_ordinal integer CHECK (current_phase_ordinal IS NULL OR current_phase_ordinal >= 1),
    origin_request_id uuid NOT NULL,
    origin_payload jsonb NOT NULL,
    origin_result jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_pipeline_runs_slice_fk FOREIGN KEY (tenant_id,workspace_id,slice_id) REFERENCES native_slices(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_runs_scope_fk FOREIGN KEY (tenant_id,workspace_id,scope_id) REFERENCES native_scopes(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_runs_run_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_runs_slice_unique UNIQUE (tenant_id,workspace_id,slice_id),
    CONSTRAINT slice_pipeline_runs_request_unique UNIQUE (tenant_id,workspace_id,origin_request_id),
    CONSTRAINT slice_pipeline_runs_phase_pair CHECK ((current_phase_id IS NULL) = (current_phase_ordinal IS NULL))
);

CREATE TABLE slice_pipeline_phase_attempts (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    phase_id text NOT NULL CHECK (pg_catalog.btrim(phase_id) <> ''),
    phase_ordinal integer NOT NULL CHECK (phase_ordinal >= 1),
    attempt bigint NOT NULL CHECK (attempt >= 1),
    outcome text NOT NULL CHECK (outcome IN ('completed','waiting_input','blocked')),
    transition text NOT NULL CHECK (transition IN ('continue','complete','block','escalate')),
    revisit_phase_id text,
    escalation_target text,
    actor_session_id uuid NOT NULL,
    reviewer_context jsonb CHECK (reviewer_context IS NULL OR pg_catalog.jsonb_typeof(reviewer_context) = 'object'),
    request_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_pipeline_attempts_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_attempts_session_fk FOREIGN KEY (tenant_id,workspace_id,actor_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_attempts_attempt_unique UNIQUE (tenant_id,workspace_id,run_id,phase_id,attempt),
    CONSTRAINT slice_pipeline_attempts_attempt_id_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_attempts_run_id_unique UNIQUE (tenant_id,workspace_id,run_id,id),
    CONSTRAINT slice_pipeline_attempts_request_unique UNIQUE (tenant_id,workspace_id,request_id)
);

CREATE TABLE slice_pipeline_phase_outputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    attempt_id uuid NOT NULL,
    phase_id text NOT NULL CHECK (pg_catalog.btrim(phase_id) <> ''),
    phase_ordinal integer NOT NULL CHECK (phase_ordinal >= 1),
    revision bigint NOT NULL CHECK (revision >= 1),
    body text NOT NULL CHECK (pg_catalog.btrim(body) <> '' AND pg_catalog.octet_length(body) <= 2097152),
    producer_context_id text NOT NULL CHECK (pg_catalog.btrim(producer_context_id) <> '' AND pg_catalog.octet_length(producer_context_id) <= 1024),
    body_digest text NOT NULL CHECK (pg_catalog.btrim(body_digest) <> ''),
    reference text,
    fields jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(fields) = 'object'),
    verdict text,
    dispositions jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(dispositions) = 'array'),
    skill_reads jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(skill_reads) = 'array'),
    resource_reads jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(resource_reads) = 'array'),
    artifacts jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(artifacts) = 'array' AND pg_catalog.octet_length(artifacts::text) <= 2097152),
    validator_receipts jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(validator_receipts) = 'array'),
    followup_proposal jsonb CHECK (followup_proposal IS NULL OR pg_catalog.jsonb_typeof(followup_proposal) = 'object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_pipeline_outputs_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_outputs_attempt_fk FOREIGN KEY (tenant_id,workspace_id,run_id,attempt_id) REFERENCES slice_pipeline_phase_attempts(tenant_id,workspace_id,run_id,id),
    CONSTRAINT slice_pipeline_outputs_attempt_unique UNIQUE (tenant_id,workspace_id,attempt_id),
    CONSTRAINT slice_pipeline_outputs_revision_unique UNIQUE (tenant_id,workspace_id,run_id,phase_id,revision),
    CONSTRAINT slice_pipeline_outputs_output_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_outputs_run_id_unique UNIQUE (tenant_id,workspace_id,run_id,id)
);

CREATE TABLE slice_pipeline_output_bindings (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    phase_id text NOT NULL,
    phase_ordinal integer NOT NULL CHECK (phase_ordinal >= 1),
    output_id uuid NOT NULL,
    output_revision bigint NOT NULL CHECK (output_revision >= 1),
    stale boolean NOT NULL DEFAULT false,
    stale_reason text,
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,run_id,phase_id),
    CONSTRAINT slice_pipeline_bindings_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_bindings_output_fk FOREIGN KEY (tenant_id,workspace_id,run_id,output_id) REFERENCES slice_pipeline_phase_outputs(tenant_id,workspace_id,run_id,id),
    CONSTRAINT slice_pipeline_bindings_stale_reason CHECK (stale OR stale_reason IS NULL)
);

CREATE TABLE slice_pipeline_inputs (
    id uuid PRIMARY KEY DEFAULT pg_catalog.gen_random_uuid(),
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    sequence bigint NOT NULL CHECK (sequence >= 1),
    phase_id text NOT NULL CHECK (pg_catalog.btrim(phase_id) <> ''),
    input text NOT NULL CHECK (pg_catalog.btrim(input) <> '' AND pg_catalog.octet_length(input) <= 65536),
    input_digest text NOT NULL CHECK (pg_catalog.btrim(input_digest) <> ''),
    actor_session_id uuid NOT NULL,
    request_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT slice_pipeline_inputs_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_inputs_session_fk FOREIGN KEY (tenant_id,workspace_id,actor_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_inputs_sequence_unique UNIQUE (tenant_id,workspace_id,run_id,sequence),
    CONSTRAINT slice_pipeline_inputs_request_unique UNIQUE (tenant_id,workspace_id,request_id)
);

CREATE TABLE slice_pipeline_receipts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('delivery_escalate')),
    request_id uuid NOT NULL,
    actor_session_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,run_id,operation,request_id),
    CONSTRAINT slice_pipeline_receipts_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT slice_pipeline_receipts_session_fk FOREIGN KEY (tenant_id,workspace_id,actor_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id)
);

ALTER TABLE slice_results ADD COLUMN pipeline_run_id uuid;
ALTER TABLE slice_results ADD COLUMN pipeline_definition_version text;
ALTER TABLE slice_results ADD COLUMN pipeline_definition_digest text;
ALTER TABLE slice_results ADD COLUMN pipeline_final_attempt_id uuid;
ALTER TABLE slice_results ADD COLUMN pipeline_result_origin text;
ALTER TABLE slice_results ADD CONSTRAINT slice_results_pipeline_run_fk
    FOREIGN KEY (tenant_id,workspace_id,pipeline_run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id);
ALTER TABLE slice_results ADD CONSTRAINT slice_results_pipeline_attempt_fk
    FOREIGN KEY (tenant_id,workspace_id,pipeline_run_id,pipeline_final_attempt_id) REFERENCES slice_pipeline_phase_attempts(tenant_id,workspace_id,run_id,id);
ALTER TABLE slice_results ADD CONSTRAINT slice_results_pipeline_binding_check CHECK (
    (pipeline_run_id IS NULL AND pipeline_definition_version IS NULL AND pipeline_definition_digest IS NULL
        AND pipeline_final_attempt_id IS NULL AND pipeline_result_origin IS NULL)
    OR
    (pipeline_run_id IS NOT NULL AND pipeline_definition_version IS NOT NULL
        AND pg_catalog.btrim(pipeline_definition_version) <> ''
        AND pipeline_definition_digest IS NOT NULL
        AND pg_catalog.btrim(pipeline_definition_digest) <> '' AND pipeline_final_attempt_id IS NOT NULL
        AND pipeline_result_origin IS NOT NULL
        AND pipeline_result_origin IN ('managed_completed','managed_blocked','managed_escalated'))
);

CREATE INDEX slice_pipeline_runs_epoch_idx ON slice_pipeline_runs(tenant_id,workspace_id,epoch_month,created_at,id);
CREATE INDEX slice_pipeline_attempts_epoch_idx ON slice_pipeline_phase_attempts(tenant_id,workspace_id,epoch_month,created_at,run_id,phase_ordinal,attempt);
CREATE INDEX slice_pipeline_outputs_epoch_idx ON slice_pipeline_phase_outputs(tenant_id,workspace_id,epoch_month,created_at,run_id,phase_ordinal,revision);
CREATE INDEX slice_pipeline_inputs_epoch_idx ON slice_pipeline_inputs(tenant_id,workspace_id,epoch_month,created_at,run_id,sequence);
CREATE INDEX slice_pipeline_receipts_epoch_idx ON slice_pipeline_receipts(tenant_id,workspace_id,epoch_month,created_at,run_id);

ALTER TABLE slice_pipeline_runs ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_runs FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_phase_attempts ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_phase_attempts FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_phase_outputs ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_phase_outputs FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_output_bindings ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_output_bindings FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_inputs ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_inputs FORCE ROW LEVEL SECURITY;
ALTER TABLE slice_pipeline_receipts ENABLE ROW LEVEL SECURITY; ALTER TABLE slice_pipeline_receipts FORCE ROW LEVEL SECURITY;

CREATE POLICY slice_pipeline_runs_tenant_scope ON slice_pipeline_runs USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_runs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_runs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_pipeline_attempts_tenant_scope ON slice_pipeline_phase_attempts USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_phase_attempts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_phase_attempts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_pipeline_outputs_tenant_scope ON slice_pipeline_phase_outputs USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_phase_outputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_phase_outputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_pipeline_bindings_tenant_scope ON slice_pipeline_output_bindings USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_output_bindings'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_output_bindings'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_pipeline_inputs_tenant_scope ON slice_pipeline_inputs USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_inputs'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY slice_pipeline_receipts_tenant_scope ON slice_pipeline_receipts USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='slice_pipeline_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

CREATE TRIGGER slice_pipeline_runs_created_at_immutable BEFORE UPDATE ON slice_pipeline_runs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_pipeline_attempts_created_at_immutable BEFORE UPDATE ON slice_pipeline_phase_attempts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_pipeline_outputs_created_at_immutable BEFORE UPDATE ON slice_pipeline_phase_outputs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_pipeline_inputs_created_at_immutable BEFORE UPDATE ON slice_pipeline_inputs FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();
CREATE TRIGGER slice_pipeline_receipts_created_at_immutable BEFORE UPDATE ON slice_pipeline_receipts FOR EACH ROW EXECUTE FUNCTION tect_preserve_created_at();

REVOKE ALL PRIVILEGES ON TABLE slice_pipeline_runs,slice_pipeline_phase_attempts,
    slice_pipeline_phase_outputs,slice_pipeline_output_bindings,slice_pipeline_inputs,
    slice_pipeline_receipts FROM PUBLIC;
