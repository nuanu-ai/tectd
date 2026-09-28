-- Support the three previously unindexed arms of the lifecycle erasure
-- receipt lookup. The begin/request_id arm uses the existing primary key.
CREATE INDEX knowledge_lifecycle_receipts_erased_change_lookup
  ON knowledge_lifecycle_command_receipts (tenant_id, workspace_id, erased_change_id);

CREATE INDEX knowledge_lifecycle_receipts_request_change_lookup
  ON knowledge_lifecycle_command_receipts (tenant_id, workspace_id, (request_payload->>'change_id'));

CREATE INDEX knowledge_lifecycle_receipts_request_run_lookup
  ON knowledge_lifecycle_command_receipts (tenant_id, workspace_id, (request_payload->>'run_id'));
