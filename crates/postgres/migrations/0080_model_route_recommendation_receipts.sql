-- Recommendation-only Slice 05 receipts. All facts are tied to an exact
-- Matrix-selected native save; execution observations are not accepted here.
CREATE TABLE model_route_preparations (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    request_key text NOT NULL,
    disposition_id uuid NOT NULL,
    candidate_set_id uuid NOT NULL,
    caller_request_id uuid NOT NULL,
    work_node_id uuid NOT NULL,
    work_node_revision bigint NOT NULL CHECK (work_node_revision >= 1),
    task_id uuid NOT NULL,
    task_revision bigint NOT NULL CHECK (task_revision >= 1),
    advisory_mode text NOT NULL CHECK (advisory_mode IN ('disabled','optional')),
    advisory_config_revision bigint NOT NULL CHECK (advisory_config_revision >= 0),
    work_digest text NOT NULL CHECK (work_digest ~ '^[0-9a-f]{64}$'),
    catalogue_digest text CHECK (catalogue_digest ~ '^[0-9a-f]{64}$'),
    host_capability_evidence_ref text,
    prepared_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(prepared_payload)='object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,request_key),
    CONSTRAINT model_route_preparation_link_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,caller_request_id)
        REFERENCES matrix_planning_selection_links
        (tenant_id,workspace_id,candidate_set_id,caller_request_id),
    CONSTRAINT model_route_preparation_shape CHECK (
        pg_catalog.length(request_key) BETWEEN 1 AND 256
        AND pg_catalog.btrim(request_key)=request_key
        AND prepared_payload->'routes'->'observed_actual' IS NOT DISTINCT FROM 'null'::jsonb
        AND prepared_payload->'routes'->'recommended_route_id' IS NOT DISTINCT FROM 'null'::jsonb)
);

CREATE TABLE model_route_decisions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL,
    preparation_request_key text NOT NULL,
    outcome_kind text NOT NULL CHECK (outcome_kind IN ('recommended','abstained','no_route')),
    requested_route_id text,
    recommended_route_id text,
    reason text,
    ranking jsonb,
    decision_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(decision_payload)='object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,preparation_request_key),
    CONSTRAINT model_route_decision_preparation_fk FOREIGN KEY
        (tenant_id,workspace_id,preparation_request_key)
        REFERENCES model_route_preparations (tenant_id,workspace_id,request_key),
    CONSTRAINT model_route_decision_shape CHECK (
        (outcome_kind='recommended' AND recommended_route_id IS NOT NULL AND reason IS NULL)
        OR (outcome_kind IN ('abstained','no_route') AND recommended_route_id IS NULL AND reason IS NOT NULL)),
    CONSTRAINT model_route_decision_no_actual CHECK
        (decision_payload->'routes'->'observed_actual' IS NOT DISTINCT FROM 'null'::jsonb)
);

CREATE TABLE model_route_dispositions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    id uuid NOT NULL,
    decision_id uuid NOT NULL,
    actor_id uuid NOT NULL,
    action text NOT NULL CHECK (action IN ('accept','reject')),
    rationale text NOT NULL CHECK
        (pg_catalog.length(rationale) BETWEEN 1 AND 4096 AND pg_catalog.btrim(rationale)<>''),
    disposition_payload jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(disposition_payload)='object'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,id),
    UNIQUE (tenant_id,workspace_id,decision_id),
    CONSTRAINT model_route_disposition_decision_fk FOREIGN KEY
        (tenant_id,workspace_id,decision_id)
        REFERENCES model_route_decisions (tenant_id,workspace_id,id),
    CONSTRAINT model_route_disposition_actor_fk FOREIGN KEY
        (tenant_id,actor_id) REFERENCES principals (tenant_id,id)
);

CREATE FUNCTION model_route_receipt_immutable() RETURNS trigger LANGUAGE plpgsql AS $guard$
BEGIN
    RAISE EXCEPTION 'model-route receipt is immutable' USING ERRCODE='23514';
END
$guard$;
CREATE TRIGGER model_route_preparation_immutable BEFORE UPDATE OR DELETE ON model_route_preparations
    FOR EACH ROW EXECUTE FUNCTION model_route_receipt_immutable();
CREATE TRIGGER model_route_decision_immutable BEFORE UPDATE OR DELETE ON model_route_decisions
    FOR EACH ROW EXECUTE FUNCTION model_route_receipt_immutable();
CREATE TRIGGER model_route_disposition_immutable BEFORE UPDATE OR DELETE ON model_route_dispositions
    FOR EACH ROW EXECUTE FUNCTION model_route_receipt_immutable();

DO $policy$
DECLARE relation_name text;
BEGIN
    FOREACH relation_name IN ARRAY ARRAY[
        'model_route_preparations','model_route_decisions','model_route_dispositions'
    ] LOOP
        EXECUTE pg_catalog.format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',relation_name);
        EXECUTE pg_catalog.format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',relation_name||'_tenant_scope',relation_name,relation_name,relation_name);
    END LOOP;
END
$policy$;
REVOKE ALL PRIVILEGES ON TABLE model_route_preparations,model_route_decisions,model_route_dispositions FROM PUBLIC;
REVOKE ALL PRIVILEGES ON FUNCTION model_route_receipt_immutable() FROM PUBLIC;
