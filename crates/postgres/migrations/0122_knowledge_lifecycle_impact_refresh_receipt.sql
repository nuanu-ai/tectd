ALTER TABLE knowledge_lifecycle_command_receipts
  DROP CONSTRAINT knowledge_lifecycle_command_receipts_operation_check;

ALTER TABLE knowledge_lifecycle_command_receipts
  ADD CONSTRAINT knowledge_lifecycle_command_receipts_operation_check
  CHECK (operation IN ('begin','phase_complete','record_input','impact_refresh','commit','settle_effects'));
