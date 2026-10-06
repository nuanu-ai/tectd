-- The prior anti-bloat caller link pointed at the additive candidate-delta
-- graph. New applications must link the actual saved-draft mutation instead.
ALTER TABLE scope_candidate_receipts
    DROP CONSTRAINT scope_candidate_receipts_operation_check,
    ADD CONSTRAINT scope_candidate_receipts_operation_check CHECK
        (operation IN ('save_draft','save_review','record_input','refresh','anti_bloat_narrow')),
    ADD CONSTRAINT scope_candidate_receipts_native_revision_unique UNIQUE
        (tenant_id,workspace_id,candidate_set_id,operation,request_id,result_revision);

ALTER TABLE scope_anti_bloat_caller_links
    DROP CONSTRAINT scope_anti_bloat_caller_delta_fk,
    ADD COLUMN caller_operation text,
    ADD COLUMN caller_request_id uuid,
    ADD COLUMN from_revision bigint,
    ADD COLUMN to_revision bigint,
    ADD COLUMN source_digest text,
    ADD COLUMN before_material_digest text,
    ADD CONSTRAINT scope_anti_bloat_native_caller_complete CHECK (
        (caller_operation IS NULL AND caller_request_id IS NULL
         AND from_revision IS NULL AND to_revision IS NULL
         AND source_digest IS NULL AND before_material_digest IS NULL)
        OR
        (caller_operation IS NOT NULL AND caller_operation = 'anti_bloat_narrow'
         AND caller_request_id IS NOT NULL
         AND from_revision IS NOT NULL AND to_revision IS NOT NULL
         AND to_revision = from_revision + 1
         AND source_digest IS NOT NULL AND source_digest ~ '^[0-9a-f]{64}$'
         AND before_material_digest IS NOT NULL
         AND before_material_digest ~ '^[0-9a-f]{64}$')
    ),
    ADD CONSTRAINT scope_anti_bloat_native_caller_receipt_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,caller_operation,caller_request_id,to_revision)
        REFERENCES scope_candidate_receipts
        (tenant_id,workspace_id,candidate_set_id,operation,request_id,result_revision),
    ADD CONSTRAINT scope_anti_bloat_native_before_draft_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,from_revision)
        REFERENCES scope_candidate_drafts
        (tenant_id,workspace_id,candidate_set_id,set_revision),
    ADD CONSTRAINT scope_anti_bloat_native_after_draft_fk FOREIGN KEY
        (tenant_id,workspace_id,candidate_set_id,to_revision)
        REFERENCES scope_candidate_drafts
        (tenant_id,workspace_id,candidate_set_id,set_revision);

CREATE UNIQUE INDEX scope_anti_bloat_native_idempotency_unique
    ON scope_anti_bloat_caller_links
        (tenant_id,workspace_id,candidate_set_id,caller_idempotency_key)
    WHERE caller_operation = 'anti_bloat_narrow';
