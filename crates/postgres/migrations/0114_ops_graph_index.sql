-- Rebuildable, current operational graph projection. Only the reviewed
-- Knowledge Change publisher may populate these rows.
CREATE TABLE ops_nodes (
  id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  iri text NOT NULL CHECK (pg_catalog.length(iri) BETWEEN 1 AND 512),
  owner_unit_id uuid NOT NULL,
  owner_revision bigint NOT NULL CHECK (owner_revision >= 1),
  kind text NOT NULL CHECK (kind IN (
    'taxonomy_concept','knowledge_resource','project_context','environment','deployment_surface',
    'host','access_route','network_endpoint','credential_locator',
    'access_procedure'
  )),
  label text NOT NULL CHECK (pg_catalog.length(label) BETWEEN 1 AND 512),
  label_locale text NOT NULL DEFAULT 'und' CHECK (pg_catalog.length(label_locale) BETWEEN 2 AND 35),
  normalized_label text NOT NULL CHECK (pg_catalog.length(normalized_label) BETWEEN 1 AND 512),
  access_scope text NOT NULL CHECK (access_scope IN ('workspace_members','owners_only')),
  workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,iri),
  UNIQUE (tenant_id,workspace_id,id),
  FOREIGN KEY (tenant_id,workspace_id,owner_unit_id,owner_revision)
    REFERENCES knowledge_revisions(tenant_id,workspace_id,unit_id,revision)
    DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX ops_nodes_label_idx
  ON ops_nodes(tenant_id,workspace_id,normalized_label,label_locale,iri);
CREATE INDEX ops_nodes_owner_idx
  ON ops_nodes(tenant_id,workspace_id,owner_unit_id,owner_revision);
CREATE TABLE ops_aliases (
  id uuid NOT NULL DEFAULT pg_catalog.gen_random_uuid(),
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  node_iri text NOT NULL,
  locale text NOT NULL DEFAULT 'und' CHECK (pg_catalog.length(locale) BETWEEN 2 AND 35),
  alias text NOT NULL CHECK (pg_catalog.length(alias) BETWEEN 1 AND 512),
  normalized_alias text NOT NULL CHECK (pg_catalog.length(normalized_alias) BETWEEN 1 AND 512),
  is_preferred boolean NOT NULL DEFAULT false,
  PRIMARY KEY (tenant_id,workspace_id,node_iri,locale,normalized_alias),
  UNIQUE (tenant_id,workspace_id,id),
  FOREIGN KEY (tenant_id,workspace_id,node_iri)
    REFERENCES ops_nodes(tenant_id,workspace_id,iri) ON DELETE CASCADE
    DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX ops_aliases_lookup_idx
  ON ops_aliases(tenant_id,workspace_id,normalized_alias,locale,node_iri);
-- Every preferred label and alias, including the legacy primary label, is
-- projected here. A lookup term can identify only one concept per locale.
CREATE UNIQUE INDEX ops_aliases_taxonomy_term_idx
  ON ops_aliases(tenant_id,workspace_id,locale,normalized_alias);

CREATE TABLE ops_edges (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  assertion_id uuid NOT NULL,
  id uuid GENERATED ALWAYS AS (assertion_id) STORED,
  owner_unit_id uuid NOT NULL,
  owner_revision bigint NOT NULL CHECK (owner_revision >= 1),
  ordinal integer NOT NULL CHECK (ordinal >= 0),
  subject_iri text NOT NULL,
  predicate text NOT NULL CHECK (predicate IN (
    'has_environment','has_deployment_surface','targets_host','runs_on_host',
    'has_endpoint','reachable_through','uses_connector_host','access_via',
    'applies_to_route','uses_credential_locator','related_to_project_context',
    'replaces_host','supported_by','broader_concept','classified_as','applies_to'
  )),
  object_iri text NOT NULL,
  assertion_state text NOT NULL CHECK (assertion_state IN ('accepted','historical','proposed','retracted')),
  access_scope text NOT NULL CHECK (access_scope IN ('workspace_members','owners_only')),
  source_fragment_iri text NOT NULL,
  source_digest text NOT NULL CHECK (source_digest ~ '^[0-9A-Fa-f]{64}$'),
  source_refs jsonb NOT NULL CHECK (
    pg_catalog.jsonb_typeof(source_refs)='array' AND pg_catalog.jsonb_array_length(source_refs)>0
  ),
  reviewer_ref text NOT NULL CHECK (pg_catalog.length(reviewer_ref) BETWEEN 1 AND 1024),
  review_receipt text NOT NULL CHECK (pg_catalog.length(review_receipt) BETWEEN 1 AND 1024),
  authority_basis text NOT NULL CHECK (pg_catalog.length(authority_basis) BETWEEN 1 AND 4096),
  asserted_at timestamptz NOT NULL,
  observed_at timestamptz,
  valid_from timestamptz,
  valid_until timestamptz,
  review_due_at timestamptz,
  workspace_generation bigint NOT NULL CHECK (workspace_generation >= 0),
  updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),
  PRIMARY KEY (tenant_id,workspace_id,assertion_id),
  UNIQUE (tenant_id,workspace_id,id),
  UNIQUE (tenant_id,workspace_id,owner_unit_id,owner_revision,ordinal),
  CHECK (valid_from IS NULL OR valid_until IS NULL OR valid_until >= valid_from),
  CHECK (observed_at IS NULL OR review_due_at IS NULL OR review_due_at >= observed_at),
  FOREIGN KEY (tenant_id,workspace_id,owner_unit_id,owner_revision)
    REFERENCES knowledge_revisions(tenant_id,workspace_id,unit_id,revision)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (tenant_id,workspace_id,subject_iri)
    REFERENCES ops_nodes(tenant_id,workspace_id,iri)
    DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (tenant_id,workspace_id,object_iri)
    REFERENCES ops_nodes(tenant_id,workspace_id,iri)
    DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX ops_edges_forward_idx
  ON ops_edges(tenant_id,workspace_id,subject_iri,predicate,object_iri,assertion_id);
CREATE INDEX ops_edges_reverse_idx
  ON ops_edges(tenant_id,workspace_id,object_iri,predicate,subject_iri,assertion_id);
CREATE INDEX ops_edges_owner_idx
  ON ops_edges(tenant_id,workspace_id,owner_unit_id,owner_revision);

DO $policies$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY['ops_nodes','ops_aliases','ops_edges'] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
    EXECUTE format('CREATE POLICY %I ON %I USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid) WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid=%L::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting(''tect.tenant_id'',true),'''')::uuid)',table_name||'_tenant',table_name,table_name,table_name);
    EXECUTE format('REVOKE ALL PRIVILEGES ON TABLE %I FROM PUBLIC',table_name);
  END LOOP;
END
$policies$;

ALTER TABLE knowledge_owned_copies DROP CONSTRAINT knowledge_owned_copies_relation_check;
ALTER TABLE knowledge_owned_copies ADD CONSTRAINT knowledge_owned_copies_relation_check CHECK (
  relation_name IN (
    'knowledge_changes','knowledge_command_receipts','knowledge_lifecycle_changes',
    'knowledge_change_runs','knowledge_change_operations','knowledge_change_outputs',
    'knowledge_change_attempts','knowledge_change_inputs','knowledge_lifecycle_command_receipts',
    'pipeline_knowledge_manifests','slice_pipeline_runs','slice_pipeline_phase_attempts',
    'slice_pipeline_phase_outputs','slice_pipeline_inputs','slice_pipeline_receipts',
    'slice_results','slice_planning_inputs','slice_planning_snapshots',
    'scope_candidate_drafts','scope_candidate_reviews','scope_candidate_receipts',
    'slice_candidate_drafts','slice_candidate_reviews','native_planning_receipts',
    'knowledge_search_resources','knowledge_search_embedding_jobs','knowledge_search_vectors',
    'knowledge_maintenance_signals','knowledge_maintenance_tasks',
    'knowledge_maintenance_command_receipts','knowledge_maintenance_consumers',
    'planning_knowledge_manifests','program_knowledge_refresh_receipts',
    'planning_knowledge_consumptions','programs','native_scopes',
    'pipeline_research_checkpoints','pipeline_checkpoint_receipts',
    'ops_nodes','ops_aliases','ops_edges'
  )
);
