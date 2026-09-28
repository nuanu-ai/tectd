-- Compact, versioned resource commitments. Existing inline manifests are untouched.
ALTER TABLE pipeline_knowledge_manifests
  DROP CONSTRAINT pipeline_knowledge_manifests_contract_version_check,
  ADD CONSTRAINT pipeline_knowledge_manifests_contract_version_check
    CHECK (contract_version IN ('dk-1','dk-2','dk-2-paged')),
  ADD COLUMN resource_count bigint,
  ADD COLUMN total_resource_bytes bigint,
  ADD COLUMN resource_digest_algorithm text,
  ADD CONSTRAINT pipeline_knowledge_manifests_paged_counts CHECK (
    (contract_version <> 'dk-2-paged'
      AND resource_count IS NULL AND total_resource_bytes IS NULL
      AND resource_digest_algorithm IS NULL)
    OR (contract_version = 'dk-2-paged' AND
      ((payload_erased AND resource_count IS NULL AND total_resource_bytes IS NULL
        AND resource_digest_algorithm IS NULL)
       OR (NOT payload_erased
        AND resource_count IS NOT NULL AND total_resource_bytes IS NOT NULL
        AND resource_digest_algorithm IS NOT NULL
        AND resource_count >= 0 AND total_resource_bytes >= 0
        AND (resource_count > 0 OR total_resource_bytes = 0)
        AND resource_digest_algorithm = 'resource-json-v1-sha256')))
  );

ALTER TABLE pipeline_knowledge_manifests
  DROP CONSTRAINT pipeline_knowledge_manifests_erased_shape,
  ADD CONSTRAINT pipeline_knowledge_manifests_erased_shape CHECK (
    (payload_erased
      AND digest IS NULL AND semantic_digest IS NULL AND selected IS NULL AND unresolved_needs IS NULL
      AND definition_version IS NULL AND definition_digest IS NULL AND method_requirements IS NULL
      AND selected_resources IS NULL AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
      AND resource_semantic_digest IS NULL AND resource_inquiry IS NULL
      AND resource_projection_policy IS NULL)
    OR (NOT payload_erased AND digest IS NOT NULL AND semantic_digest IS NOT NULL
      AND selected IS NOT NULL AND pg_catalog.jsonb_typeof(selected)='array'
      AND unresolved_needs IS NOT NULL AND pg_catalog.jsonb_typeof(unresolved_needs)='array'
      AND ((contract_version='dk-1' AND definition_version IS NULL AND definition_digest IS NULL
        AND method_requirements IS NULL AND selected_resources IS NULL
        AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
        AND resource_semantic_digest IS NULL AND resource_inquiry IS NULL
        AND resource_projection_policy IS NULL)
      OR (contract_version='dk-2' AND definition_version IS NOT NULL AND definition_digest IS NOT NULL
        AND method_requirements IS NOT NULL AND pg_catalog.jsonb_typeof(method_requirements)='array'
        AND selected_resources IS NOT NULL AND pg_catalog.jsonb_typeof(selected_resources)='array'
        AND resource_unresolved_needs IS NOT NULL AND pg_catalog.jsonb_typeof(resource_unresolved_needs)='array'
        AND freshness_warnings IS NOT NULL AND pg_catalog.jsonb_typeof(freshness_warnings)='array'
        AND resource_semantic_digest IS NOT NULL)
      OR (contract_version='dk-2-paged' AND definition_version IS NOT NULL AND definition_digest IS NOT NULL
        AND method_requirements IS NOT NULL AND pg_catalog.jsonb_typeof(method_requirements)='array'
        AND selected_resources IS NULL
        AND resource_unresolved_needs IS NOT NULL AND pg_catalog.jsonb_typeof(resource_unresolved_needs)='array'
        AND freshness_warnings IS NOT NULL AND pg_catalog.jsonb_typeof(freshness_warnings)='array'
        AND resource_semantic_digest IS NOT NULL)))
  );

-- The fixed contract_version also makes the FK reject child rows on legacy headers.
ALTER TABLE pipeline_knowledge_manifests
  ADD CONSTRAINT pipeline_knowledge_manifests_version_identity
    UNIQUE (tenant_id, workspace_id, id, contract_version);

CREATE TABLE pipeline_knowledge_manifest_resources (
  tenant_id uuid NOT NULL,
  workspace_id uuid NOT NULL,
  manifest_id uuid NOT NULL,
  contract_version text NOT NULL DEFAULT 'dk-2-paged' CHECK (contract_version='dk-2-paged'),
  ordinal bigint NOT NULL CHECK (ordinal >= 0),
  entry_kind text NOT NULL CHECK (entry_kind IN ('dk2_event','dk1_legacy')),
  unit_id uuid NOT NULL,
  revision bigint NOT NULL CHECK (revision >= 1),
  publication_event_id uuid,
  rdf_digest text NOT NULL CHECK (rdf_digest ~ '^[0-9a-f]{64}$'),
  binding_id uuid NOT NULL,
  binding_pin jsonb NOT NULL CHECK (
    pg_catalog.jsonb_typeof(binding_pin)='object'
    AND pg_catalog.octet_length(binding_pin::text) BETWEEN 2 AND 8192
    AND binding_pin ?& ARRAY['binding_iri','target','purpose','version_resolution']
    AND binding_pin - ARRAY['binding_iri','target','purpose','version_resolution',
      'definition_kind','definition_version','definition_digest'] = '{}'::jsonb),
  lifecycle text NOT NULL CHECK (lifecycle IN ('active','retracted','superseded','erasure_pending','erased')),
  access_scope text NOT NULL CHECK (access_scope IN ('workspace_members','owners_only')),
  validation_event_id uuid,
  validation_event_digest text,
  projection jsonb NOT NULL CHECK (
    pg_catalog.jsonb_typeof(projection)='object'
    AND pg_catalog.octet_length(projection::text) BETWEEN 2 AND 65536
    AND projection->>'policy' IN ('full_resources','program_planning_briefs','scope_planning_briefs')
    AND pg_catalog.jsonb_typeof(projection->'inquiry_briefs')='array'
    AND projection - ARRAY['policy','inquiry_briefs'] = '{}'::jsonb
    AND (projection->>'policy'<>'full_resources' OR projection->'inquiry_briefs'='[]'::jsonb)),
  resource_digest text NOT NULL CHECK (resource_digest ~ '^[0-9a-f]{64}$'),
  resource_bytes bigint NOT NULL CHECK (resource_bytes > 0),
  PRIMARY KEY (tenant_id,workspace_id,manifest_id,ordinal),
  CONSTRAINT pipeline_manifest_resources_one_row_per_binding
    UNIQUE (tenant_id,workspace_id,manifest_id,binding_id),
  CONSTRAINT pipeline_manifest_resources_header_fk
    FOREIGN KEY (tenant_id,workspace_id,manifest_id,contract_version)
    REFERENCES pipeline_knowledge_manifests(tenant_id,workspace_id,id,contract_version)
    ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
  CONSTRAINT pipeline_manifest_resources_revision_fk
    FOREIGN KEY (tenant_id,workspace_id,unit_id,revision)
    REFERENCES knowledge_revisions(tenant_id,workspace_id,unit_id,revision),
  CONSTRAINT pipeline_manifest_resources_event_fk
    FOREIGN KEY (tenant_id,workspace_id,publication_event_id)
    REFERENCES knowledge_publication_events(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_manifest_resources_binding_fk
    FOREIGN KEY (tenant_id,workspace_id,binding_id)
    REFERENCES knowledge_bindings(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_manifest_resources_validation_fk
    FOREIGN KEY (tenant_id,workspace_id,validation_event_id)
    REFERENCES knowledge_validation_events(tenant_id,workspace_id,id),
  CONSTRAINT pipeline_manifest_resources_kind_shape CHECK (
    (entry_kind='dk2_event' AND publication_event_id IS NOT NULL)
    OR (entry_kind='dk1_legacy' AND publication_event_id IS NULL)),
  CONSTRAINT pipeline_manifest_resources_validation_shape CHECK (
    (validation_event_id IS NULL) = (validation_event_digest IS NULL)
    AND (validation_event_digest IS NULL OR validation_event_digest ~ '^[0-9a-f]{64}$'))
);
CREATE INDEX pipeline_manifest_resources_unit_idx ON pipeline_knowledge_manifest_resources
  (tenant_id,workspace_id,unit_id,manifest_id);
CREATE INDEX pipeline_manifest_resources_binding_idx ON pipeline_knowledge_manifest_resources
  (tenant_id,workspace_id,binding_id);
ALTER TABLE pipeline_knowledge_manifest_resources ENABLE ROW LEVEL SECURITY;
ALTER TABLE pipeline_knowledge_manifest_resources FORCE ROW LEVEL SECURITY;
CREATE POLICY pipeline_knowledge_manifest_resources_tenant ON pipeline_knowledge_manifest_resources
  USING (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_knowledge_manifest_resources'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)
  WITH CHECK (CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='pipeline_knowledge_manifest_resources'::regclass))
    OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid);
REVOKE ALL PRIVILEGES ON TABLE pipeline_knowledge_manifest_resources FROM PUBLIC;
