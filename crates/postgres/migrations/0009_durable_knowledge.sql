-- DK-1 metadata and narrow native RDF adapter. Extension activation is operator-only.
CREATE TABLE durable_knowledge_capability (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    capability_ready boolean NOT NULL DEFAULT false,
    pgrdf_version text,
    activated_at timestamptz
);
INSERT INTO durable_knowledge_capability(singleton) VALUES(true);
REVOKE ALL PRIVILEGES ON TABLE durable_knowledge_capability FROM PUBLIC;

CREATE TABLE workspace_knowledge_state (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    generation bigint NOT NULL DEFAULT 0 CHECK (generation >= 0),
    capability_ready boolean NOT NULL DEFAULT false,
    profile_id text NOT NULL DEFAULT 'tect:durable-knowledge:general-constraint',
    profile_version text NOT NULL DEFAULT 'dk-1',
    pgrdf_version text,
    activated_at timestamptz,
    PRIMARY KEY (tenant_id,workspace_id),
    CONSTRAINT workspace_knowledge_state_workspace_fk FOREIGN KEY (tenant_id,workspace_id)
        REFERENCES workspaces(tenant_id,id)
);

CREATE TABLE knowledge_changes (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    change_revision bigint NOT NULL DEFAULT 1 CHECK (change_revision >= 1),
    operation text NOT NULL CHECK (operation IN ('create','revise','retract')),
    stage text NOT NULL CHECK (stage IN ('review_required','ready_to_publish','rejected','committed')),
    expected_generation bigint NOT NULL CHECK (expected_generation >= 0),
    expected_unit_revision bigint CHECK (expected_unit_revision IS NULL OR expected_unit_revision >= 1),
    proposed_unit_revision bigint NOT NULL CHECK (proposed_unit_revision >= 1),
    proposal_digest text NOT NULL,
    proposal_fingerprint text,
    source_sha256 text,
    semantic_diff text NOT NULL,
    baseline jsonb,
    proposal jsonb,
    binding_provenance jsonb,
    preparation_method jsonb NOT NULL,
    review_method jsonb NOT NULL,
    reason text NOT NULL,
    authority_basis text NOT NULL,
    review jsonb,
    publication_receipt jsonb,
    prepared_principal_id uuid NOT NULL,
    prepared_session_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT knowledge_changes_workspace_fk FOREIGN KEY (tenant_id,workspace_id) REFERENCES workspaces(tenant_id,id),
    CONSTRAINT knowledge_changes_session_fk FOREIGN KEY (tenant_id,workspace_id,prepared_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id),
    CONSTRAINT knowledge_changes_unique UNIQUE (tenant_id,workspace_id,id),
    CONSTRAINT knowledge_changes_proposal_shape CHECK ((operation='retract') = (proposal IS NULL))
);

CREATE TABLE knowledge_unit_heads (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    accepted_revision bigint NOT NULL CHECK (accepted_revision >= 1),
    active boolean NOT NULL,
    proposal_fingerprint text NOT NULL,
    last_event_id uuid NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,unit_id),
    CONSTRAINT knowledge_unit_heads_workspace_fk FOREIGN KEY (tenant_id,workspace_id) REFERENCES workspaces(tenant_id,id)
);

CREATE TABLE knowledge_publication_events (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    unit_revision bigint NOT NULL CHECK (unit_revision >= 1),
    change_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('create','revise','retract')),
    actor_principal_id uuid NOT NULL,
    actor_session_id uuid NOT NULL,
    rdf_digest text NOT NULL,
    rdf_digest_method text NOT NULL CHECK (rdf_digest_method='rdfc-1.0-sha256'),
    rdf_digest_scope text NOT NULL CHECK (rdf_digest_scope IN ('revision_publication_payload','lifecycle_event_payload')),
    unit_iri text NOT NULL,
    revision_iri text NOT NULL,
    event_iri text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    epoch_month date GENERATED ALWAYS AS (pg_catalog.date_trunc('month',created_at AT TIME ZONE 'UTC')::date) STORED NOT NULL,
    CONSTRAINT knowledge_events_change_fk FOREIGN KEY (tenant_id,workspace_id,change_id) REFERENCES knowledge_changes(tenant_id,workspace_id,id),
    CONSTRAINT knowledge_events_session_fk FOREIGN KEY (tenant_id,workspace_id,actor_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id),
    CONSTRAINT knowledge_events_unique UNIQUE (tenant_id,workspace_id,id)
);

CREATE TABLE knowledge_revisions (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision >= 1),
    constraint_payload jsonb NOT NULL,
    source_sha256 text NOT NULL,
    rdf_digest text NOT NULL,
    rdf_digest_method text NOT NULL CHECK (rdf_digest_method='rdfc-1.0-sha256'),
    rdf_digest_scope text NOT NULL CHECK (rdf_digest_scope='revision_publication_payload'),
    publication_event_id uuid NOT NULL,
    unit_iri text NOT NULL,
    revision_iri text NOT NULL,
    source_iri text NOT NULL,
    publication_event_iri text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,unit_id,revision),
    CONSTRAINT knowledge_revisions_event_fk FOREIGN KEY (tenant_id,workspace_id,publication_event_id) REFERENCES knowledge_publication_events(tenant_id,workspace_id,id)
);

CREATE TABLE knowledge_bindings (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    revision bigint NOT NULL,
    binding_kind text NOT NULL CHECK (binding_kind IN ('workspace','slice_phase')),
    scope_id uuid,
    slice_id uuid,
    phase_id text,
    definition_kind text,
    definition_version text,
    definition_digest text,
    active boolean NOT NULL,
    PRIMARY KEY (tenant_id,workspace_id,unit_id,revision),
    CONSTRAINT knowledge_bindings_revision_fk FOREIGN KEY (tenant_id,workspace_id,unit_id,revision) REFERENCES knowledge_revisions(tenant_id,workspace_id,unit_id,revision),
    CONSTRAINT knowledge_bindings_shape CHECK (
        (binding_kind='workspace' AND scope_id IS NULL AND slice_id IS NULL AND phase_id IS NULL AND definition_kind IS NULL AND definition_version IS NULL AND definition_digest IS NULL)
        OR (binding_kind='slice_phase' AND scope_id IS NOT NULL AND slice_id IS NOT NULL AND phase_id IS NOT NULL AND definition_kind IS NOT NULL AND definition_version IS NOT NULL AND definition_digest IS NOT NULL)
    )
);

CREATE TABLE knowledge_command_receipts (
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('prepare','review','publish','refresh')),
    request_id uuid NOT NULL,
    actor_session_id uuid NOT NULL,
    request_payload jsonb NOT NULL,
    result_payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    PRIMARY KEY (tenant_id,workspace_id,operation,request_id),
    CONSTRAINT knowledge_receipts_session_fk FOREIGN KEY (tenant_id,workspace_id,actor_session_id) REFERENCES agent_sessions(tenant_id,workspace_id,id)
);

CREATE TABLE pipeline_knowledge_manifests (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    run_id uuid NOT NULL,
    run_revision bigint NOT NULL CHECK (run_revision >= 1),
    phase_id text NOT NULL,
    workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
    digest text NOT NULL,
    semantic_digest text NOT NULL,
    selected jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(selected)='array'),
    unresolved_needs jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(unresolved_needs)='array'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT pipeline_knowledge_manifest_run_fk FOREIGN KEY (tenant_id,workspace_id,run_id) REFERENCES slice_pipeline_runs(tenant_id,workspace_id,id),
    CONSTRAINT pipeline_knowledge_manifest_unique UNIQUE (tenant_id,workspace_id,id)
);

ALTER TABLE slice_pipeline_runs ADD COLUMN knowledge_manifest_id uuid;
ALTER TABLE slice_pipeline_runs ADD COLUMN knowledge_manifest_digest text;
ALTER TABLE slice_pipeline_runs ADD CONSTRAINT slice_pipeline_runs_knowledge_manifest_fk
    FOREIGN KEY (tenant_id,workspace_id,knowledge_manifest_id) REFERENCES pipeline_knowledge_manifests(tenant_id,workspace_id,id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE knowledge_effect_outbox (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    workspace_id uuid NOT NULL,
    publication_event_id uuid NOT NULL,
    effect text NOT NULL CHECK (effect='invalidate_phase_context'),
    status text NOT NULL DEFAULT 'recorded' CHECK (status='recorded'),
    created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
    CONSTRAINT knowledge_effect_event_fk FOREIGN KEY (tenant_id,workspace_id,publication_event_id) REFERENCES knowledge_publication_events(tenant_id,workspace_id,id)
);

CREATE INDEX knowledge_active_fingerprint_idx ON knowledge_unit_heads(tenant_id,workspace_id,proposal_fingerprint) WHERE active;
CREATE INDEX knowledge_bindings_phase_idx ON knowledge_bindings(tenant_id,workspace_id,scope_id,slice_id,phase_id) WHERE active;
CREATE INDEX pipeline_knowledge_manifests_run_idx ON pipeline_knowledge_manifests(tenant_id,workspace_id,run_id,created_at);

ALTER TABLE workspace_knowledge_state ENABLE ROW LEVEL SECURITY; ALTER TABLE workspace_knowledge_state FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_changes ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_changes FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_unit_heads ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_unit_heads FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_publication_events ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_publication_events FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_revisions ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_bindings ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_bindings FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_command_receipts ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_command_receipts FORCE ROW LEVEL SECURITY;
ALTER TABLE pipeline_knowledge_manifests ENABLE ROW LEVEL SECURITY; ALTER TABLE pipeline_knowledge_manifests FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_effect_outbox ENABLE ROW LEVEL SECURITY; ALTER TABLE knowledge_effect_outbox FORCE ROW LEVEL SECURITY;

CREATE POLICY workspace_knowledge_state_tenant ON workspace_knowledge_state USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='workspace_knowledge_state'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='workspace_knowledge_state'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_changes_tenant ON knowledge_changes USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_changes'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_changes'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_unit_heads_tenant ON knowledge_unit_heads USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_unit_heads'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_unit_heads'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_events_tenant ON knowledge_publication_events USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_publication_events'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_publication_events'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_revisions_tenant ON knowledge_revisions USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_revisions'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_revisions'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_bindings_tenant ON knowledge_bindings USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_bindings'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_bindings'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_receipts_tenant ON knowledge_command_receipts USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_command_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_command_receipts'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY pipeline_knowledge_manifests_tenant ON pipeline_knowledge_manifests USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_knowledge_manifests'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_knowledge_manifests'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
CREATE POLICY knowledge_effect_outbox_tenant ON knowledge_effect_outbox USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_effect_outbox'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_effect_outbox'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);

REVOKE ALL PRIVILEGES ON TABLE workspace_knowledge_state,knowledge_changes,knowledge_unit_heads,
    knowledge_publication_events,knowledge_revisions,knowledge_bindings,knowledge_command_receipts,
    pipeline_knowledge_manifests,knowledge_effect_outbox FROM PUBLIC;

CREATE FUNCTION tect_dk_native_publish(p_tenant uuid,p_workspace uuid,p_event uuid,p_operation text,p_payload text,p_stable_payload text)
RETURNS text LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $fn$
DECLARE scratch text; stable text; scratch_id bigint; stable_id bigint; shapes_id bigint; report jsonb; result text; typed_count bigint;
BEGIN
  IF NULLIF(current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS (SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace)
     OR NOT EXISTS (SELECT 1 FROM public.workspace_knowledge_state WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND capability_ready) THEN
    RAISE EXCEPTION USING ERRCODE='42501', MESSAGE='durable knowledge unavailable';
  END IF;
  IF octet_length(p_payload)>262144 OR btrim(p_payload)='' OR octet_length(p_stable_payload)>262144 OR btrim(p_stable_payload)='' THEN RAISE EXCEPTION USING ERRCODE='22023', MESSAGE='invalid durable knowledge payload'; END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended('tect-dk-native-publisher',0));
  IF p_operation NOT IN ('create','revise','retract') THEN RAISE EXCEPTION USING ERRCODE='22023', MESSAGE='invalid durable knowledge operation'; END IF;
  scratch := 'urn:tect:dk:scratch:'||p_tenant||':'||p_workspace||':'||p_event;
  stable := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  EXECUTE 'SELECT pgrdf.add_graph($1)' INTO scratch_id USING scratch;
  EXECUTE 'SELECT pgrdf.clear_graph($1)' USING scratch_id;
  EXECUTE 'SELECT pgrdf.parse_turtle($1,$2)' USING p_payload,scratch_id;
  EXECUTE 'SELECT pgrdf.graph_id($1)' INTO shapes_id USING 'urn:tect:dk:shapes:dk-1';
  EXECUTE 'SELECT pgrdf.validate($1,$2,''native'',true)' INTO report USING scratch_id,shapes_id;
  EXECUTE 'SELECT count(*) FROM pgrdf.construct($1)'
    INTO typed_count USING 'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||scratch||'> { ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:tect:dk:'||CASE WHEN p_operation='retract' THEN 'PublicationEvent' ELSE 'ConstraintRevision' END||'> ; ?p ?o } }';
  IF report->>'conforms' IS DISTINCT FROM 'true' OR typed_count=0 THEN RAISE EXCEPTION USING ERRCODE='23514', MESSAGE='durable knowledge shape rejected'; END IF;
  EXECUTE 'SELECT pgrdf.graph_digest($1)' INTO result USING scratch_id;
  EXECUTE 'SELECT pgrdf.add_graph($1)' INTO stable_id USING stable;
  EXECUTE 'SELECT pgrdf.parse_turtle($1,$2)' USING p_stable_payload,stable_id;
  EXECUTE 'SELECT pgrdf.drop_graph($1,true)' USING scratch_id;
  RETURN result;
END $fn$;

CREATE FUNCTION tect_dk_native_read(p_tenant uuid,p_workspace uuid,p_unit uuid,p_revision bigint,p_event uuid)
RETURNS SETOF jsonb LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, public AS $fn$
DECLARE stable text; unit_iri text; revision_iri text; source_iri text; event_iri text; query text;
BEGIN
  IF NULLIF(current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS (SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace)
     OR NOT EXISTS (SELECT 1 FROM public.workspace_knowledge_state WHERE tenant_id=p_tenant AND workspace_id=p_workspace AND capability_ready) THEN
    RAISE EXCEPTION USING ERRCODE='42501', MESSAGE='durable knowledge unavailable';
  END IF;
  stable := 'urn:tect:dk:workspace:'||p_tenant||':'||p_workspace;
  unit_iri := 'urn:tect:dk:unit:'||p_tenant||':'||p_workspace||':'||p_unit;
  revision_iri := unit_iri||':revision:'||p_revision;
  source_iri := revision_iri||':source';
  event_iri := 'urn:tect:dk:event:'||p_tenant||':'||p_workspace||':'||p_event;
  query := 'CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <'||stable||'> { VALUES ?s { <'||revision_iri||'> <'||source_iri||'> <'||event_iri||'> <'||unit_iri||'> } ?s ?p ?o FILTER(?s != <'||unit_iri||'> || ?p = <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> || (?p = <urn:tect:dk:hasRevision> && ?o = <'||revision_iri||'>)) } }';
  RETURN QUERY EXECUTE 'SELECT * FROM pgrdf.construct($1)' USING query;
END $fn$;

REVOKE ALL PRIVILEGES ON FUNCTION tect_dk_native_publish(uuid,uuid,uuid,text,text,text),tect_dk_native_read(uuid,uuid,uuid,bigint,uuid) FROM PUBLIC;

CREATE FUNCTION tect_dk_session_principal(p_session uuid) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT h.principal_id FROM public.agent_sessions s JOIN public.hosts h ON h.tenant_id=s.tenant_id AND h.id=s.host_id
  WHERE s.tenant_id=NULLIF(current_setting('tect.tenant_id',true),'')::uuid AND s.id=p_session AND NOT s.revoked AND NOT h.revoked
$$;
CREATE FUNCTION tect_dk_is_owner(p_principal uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT EXISTS(SELECT 1 FROM public.principals WHERE tenant_id=NULLIF(current_setting('tect.tenant_id',true),'')::uuid AND id=p_principal AND role='owner')
$$;
REVOKE ALL PRIVILEGES ON FUNCTION tect_dk_session_principal(uuid),tect_dk_is_owner(uuid) FROM PUBLIC;

CREATE FUNCTION tect_dk_ensure_workspace_state(p_tenant uuid,p_workspace uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,public AS $$
DECLARE capability record;
BEGIN
  IF NULLIF(current_setting('tect.tenant_id',true),'')::uuid IS DISTINCT FROM p_tenant
     OR NOT EXISTS(SELECT 1 FROM public.workspaces WHERE tenant_id=p_tenant AND id=p_workspace) THEN
    RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='invalid durable knowledge workspace';
  END IF;
  SELECT capability_ready,pgrdf_version,activated_at INTO capability
    FROM public.durable_knowledge_capability WHERE singleton FOR SHARE;
  INSERT INTO public.workspace_knowledge_state(tenant_id,workspace_id,capability_ready,pgrdf_version,activated_at)
    VALUES(p_tenant,p_workspace,capability.capability_ready,capability.pgrdf_version,capability.activated_at)
    ON CONFLICT(tenant_id,workspace_id) DO NOTHING;
END $$;
REVOKE ALL PRIVILEGES ON FUNCTION tect_dk_ensure_workspace_state(uuid,uuid) FROM PUBLIC;

CREATE FUNCTION tect_dk_capability() RETURNS TABLE(capability_ready boolean,pgrdf_version text)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,public AS $$
  SELECT c.capability_ready,c.pgrdf_version FROM public.durable_knowledge_capability c WHERE c.singleton
$$;
REVOKE ALL PRIVILEGES ON FUNCTION tect_dk_capability() FROM PUBLIC;
