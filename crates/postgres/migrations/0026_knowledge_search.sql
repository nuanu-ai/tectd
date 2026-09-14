-- DK-3 rebuildable lexical projection and durable title embedding queue.
CREATE TABLE knowledge_search_capability (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  vector_ready boolean NOT NULL DEFAULT false,
  pgvector_version text,
  model_name text,
  model_revision text,
  dimensions integer,
  recipe text,
  qualified_system_identifier text,
  qualified_database_oid oid,
  activated_at timestamptz,
  CHECK ((NOT vector_ready AND pgvector_version IS NULL AND model_name IS NULL
      AND model_revision IS NULL AND dimensions IS NULL AND recipe IS NULL
      AND qualified_system_identifier IS NULL AND qualified_database_oid IS NULL
      AND activated_at IS NULL)
    OR (vector_ready AND pgvector_version='0.8.6'
      AND model_name='intfloat/multilingual-e5-small'
      AND model_revision='614241f622f53c4eeff9890bdc4f31cfecc418b3'
      AND dimensions=384 AND recipe='title_v1'
      AND qualified_system_identifier IS NOT NULL
      AND qualified_database_oid IS NOT NULL AND activated_at IS NOT NULL))
);
INSERT INTO knowledge_search_capability(singleton) VALUES(true);
REVOKE ALL PRIVILEGES ON TABLE knowledge_search_capability FROM PUBLIC;
CREATE FUNCTION tect_dk_search_vector_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path=pg_catalog,public AS $$
  SELECT COALESCE((SELECT vector_ready
    AND qualified_system_identifier=(SELECT system_identifier::text FROM pg_catalog.pg_control_system())
    AND qualified_database_oid=(SELECT oid FROM pg_catalog.pg_database
      WHERE datname=pg_catalog.current_database())
    FROM public.knowledge_search_capability WHERE singleton),false)
    AND public.tect_dk_database_identity_ready()
$$;
REVOKE ALL ON FUNCTION tect_dk_search_vector_ready() FROM PUBLIC;

CREATE TABLE knowledge_search_resources (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision >= 1),
  resource_iri text NOT NULL,
  revision_iri text NOT NULL,
  title text NOT NULL,
  canonical_text text NOT NULL,
  knowledge_kind text NOT NULL CHECK (knowledge_kind IN (
    'constraint','claim','decision','hypothesis','procedure','protocol','infrastructure',
    'operating_model','product_research','security')),
  lifecycle text NOT NULL CHECK (lifecycle IN ('active','retracted','superseded')),
  access_scope text NOT NULL CHECK (access_scope IN ('workspace_members','owners_only')),
  source_digests jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(source_digests)='array'),
  freshness_warnings jsonb NOT NULL CHECK (pg_catalog.jsonb_typeof(freshness_warnings)='array'),
  contract_version text NOT NULL CHECK (contract_version IN ('dk-1','dk-2')),
  workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
  embedding_input_digest text NOT NULL,
  verified_payload_bytes bigint NOT NULL CHECK (verified_payload_bytes > 0),
  english_document tsvector GENERATED ALWAYS AS
    (pg_catalog.to_tsvector('pg_catalog.english'::pg_catalog.regconfig,title||' '||canonical_text)) STORED,
  russian_document tsvector GENERATED ALWAYS AS
    (pg_catalog.to_tsvector('pg_catalog.russian'::pg_catalog.regconfig,title||' '||canonical_text)) STORED,
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,unit_id),
  UNIQUE (tenant_id,workspace_id,resource_iri),
  FOREIGN KEY (tenant_id,workspace_id,unit_id)
    REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id) ON DELETE CASCADE
);
CREATE INDEX knowledge_search_resources_stable_idx
  ON knowledge_search_resources(tenant_id,workspace_id,resource_iri);
CREATE INDEX knowledge_search_resources_english_idx
  ON knowledge_search_resources USING gin(english_document);
CREATE INDEX knowledge_search_resources_russian_idx
  ON knowledge_search_resources USING gin(russian_document);

CREATE TABLE knowledge_search_embedding_jobs (
  id uuid PRIMARY KEY,
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  unit_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision >= 1),
  access_scope text NOT NULL CHECK (access_scope IN ('workspace_members','owners_only')),
  workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
  model_name text NOT NULL,
  model_revision text NOT NULL,
  dimensions integer NOT NULL CHECK (dimensions=384),
  recipe text NOT NULL,
  input_digest text NOT NULL,
  state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','leased')),
  lease_token uuid,
  lease_expires_at timestamptz,
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  available_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  created_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  UNIQUE (tenant_id,workspace_id,unit_id,access_scope,model_name,model_revision,recipe,input_digest),
  CHECK ((state='pending' AND lease_token IS NULL AND lease_expires_at IS NULL)
    OR (state='leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)),
  FOREIGN KEY (tenant_id,workspace_id,unit_id)
    REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id) ON DELETE CASCADE
);
CREATE INDEX knowledge_search_embedding_jobs_claim_idx
  ON knowledge_search_embedding_jobs(tenant_id,workspace_id,state,available_at,id);

DO $policies$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY[
    'knowledge_search_resources','knowledge_search_embedding_jobs'
  ] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',table_name||'_tenant',table_name,table_name,table_name);
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE %I FROM PUBLIC',table_name);
  END LOOP;
END
$policies$;
REVOKE ALL ON FUNCTION tect_dk_search_vector_ready() FROM PUBLIC;
