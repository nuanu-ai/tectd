-- One manifest identity captures legacy constraints and typed DK-2 resources.
ALTER TABLE pipeline_knowledge_manifests
  ADD COLUMN contract_version text NOT NULL DEFAULT 'dk-1'
    CHECK (contract_version IN ('dk-1','dk-2')),
  ADD COLUMN definition_version text,
  ADD COLUMN definition_digest text,
  ADD COLUMN method_requirements jsonb,
  ADD COLUMN selected_resources jsonb,
  ADD COLUMN resource_unresolved_needs jsonb,
  ADD COLUMN freshness_warnings jsonb,
  ADD COLUMN resource_semantic_digest text;

ALTER TABLE pipeline_knowledge_manifests
  DROP CONSTRAINT pipeline_knowledge_manifests_erased_shape,
  ADD CONSTRAINT pipeline_knowledge_manifests_erased_shape CHECK (
    (payload_erased
      AND digest IS NULL AND semantic_digest IS NULL
      AND selected IS NULL AND unresolved_needs IS NULL
      AND definition_version IS NULL AND definition_digest IS NULL
      AND method_requirements IS NULL AND selected_resources IS NULL
      AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
      AND resource_semantic_digest IS NULL)
    OR
    (NOT payload_erased
      AND digest IS NOT NULL AND semantic_digest IS NOT NULL
      AND selected IS NOT NULL AND unresolved_needs IS NOT NULL
      AND (
        (contract_version='dk-1'
          AND definition_version IS NULL AND definition_digest IS NULL
          AND method_requirements IS NULL AND selected_resources IS NULL
          AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL
          AND resource_semantic_digest IS NULL)
        OR
        (contract_version='dk-2'
          AND definition_version IS NOT NULL AND definition_digest IS NOT NULL
          AND method_requirements IS NOT NULL
          AND pg_catalog.jsonb_typeof(method_requirements)='array'
          AND selected_resources IS NOT NULL
          AND pg_catalog.jsonb_typeof(selected_resources)='array'
          AND resource_unresolved_needs IS NOT NULL
          AND pg_catalog.jsonb_typeof(resource_unresolved_needs)='array'
          AND freshness_warnings IS NOT NULL
          AND pg_catalog.jsonb_typeof(freshness_warnings)='array'
          AND resource_semantic_digest IS NOT NULL)
      ))
  );
