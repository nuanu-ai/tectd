-- Immutable revision lookups for ACL-filtered operational history. These
-- indexes contain only canonical reviewed payloads; no second writer exists.
CREATE INDEX knowledge_revisions_ops_entity_history_idx
  ON knowledge_revisions(tenant_id, workspace_id,
    (document_payload->'operational_refs'->'entity'->>'iri'))
  WHERE contract_version='dk-2' AND NOT payload_erased
    AND document_payload->'operational_refs'->'entity' IS NOT NULL;

CREATE INDEX knowledge_revisions_ops_assertion_history_idx
  ON knowledge_revisions USING gin
    ((document_payload->'operational_refs'->'assertions') jsonb_path_ops)
  WHERE contract_version='dk-2' AND NOT payload_erased
    AND document_payload->'operational_refs'->'assertions' IS NOT NULL;
