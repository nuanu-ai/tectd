-- Promotion Slices are owned by the Knowledge Change engine and retain a
-- separate cursor and result provenance from ordinary Slice pipeline runs.
ALTER TABLE native_slices DROP CONSTRAINT native_slices_pipeline_check;
ALTER TABLE native_slices ADD CONSTRAINT native_slices_pipeline_check CHECK (pipeline IN (
    'slice.lightweight-tdd-development','slice.full-design-to-execution','slice.debug-root-cause',
    'slice.operational-preparation','slice.operational-execution','slice.research-to-durable-knowledge',
    'slice.custom-procedure-capture','slice.promote-to-durable-knowledge'
));

ALTER TABLE knowledge_change_runs ADD CONSTRAINT knowledge_change_runs_owner_unique
    UNIQUE (tenant_id,workspace_id,change_id,id);
ALTER TABLE knowledge_change_attempts ADD CONSTRAINT knowledge_change_attempts_run_id_unique
    UNIQUE (tenant_id,workspace_id,run_id,id);

ALTER TABLE native_slices
    ADD COLUMN knowledge_change_id uuid,
    ADD COLUMN knowledge_run_id uuid,
    ADD CONSTRAINT native_slices_knowledge_change_fk
        FOREIGN KEY (tenant_id,workspace_id,knowledge_change_id)
        REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
    ADD CONSTRAINT native_slices_knowledge_run_fk
        FOREIGN KEY (tenant_id,workspace_id,knowledge_change_id,knowledge_run_id)
        REFERENCES knowledge_change_runs(tenant_id,workspace_id,change_id,id),
    ADD CONSTRAINT native_slices_knowledge_owner_check CHECK (
        (knowledge_change_id IS NULL AND knowledge_run_id IS NULL)
        OR (pipeline='slice.promote-to-durable-knowledge'
            AND knowledge_change_id IS NOT NULL AND knowledge_run_id IS NOT NULL)
    );
CREATE UNIQUE INDEX native_slices_knowledge_change_unique
    ON native_slices(tenant_id,workspace_id,knowledge_change_id)
    WHERE knowledge_change_id IS NOT NULL;
CREATE UNIQUE INDEX native_slices_knowledge_run_unique
    ON native_slices(tenant_id,workspace_id,knowledge_run_id)
    WHERE knowledge_run_id IS NOT NULL;

ALTER TABLE slice_results DROP CONSTRAINT slice_results_provenance_check;
ALTER TABLE slice_results
    ADD CONSTRAINT slice_results_provenance_check CHECK (
        provenance IN ('externally_reported','knowledge_change_managed')
    ),
    ADD COLUMN knowledge_change_id uuid,
    ADD COLUMN knowledge_run_id uuid,
    ADD COLUMN knowledge_definition_version text,
    ADD COLUMN knowledge_definition_digest text,
    ADD COLUMN knowledge_final_attempt_id uuid,
    ADD COLUMN knowledge_publisher_receipt_id uuid,
    ADD COLUMN knowledge_publisher_receipt_digest text,
    ADD COLUMN knowledge_result_origin text,
    ADD CONSTRAINT slice_results_knowledge_change_fk
        FOREIGN KEY (tenant_id,workspace_id,knowledge_change_id)
        REFERENCES knowledge_lifecycle_changes(tenant_id,workspace_id,id),
    ADD CONSTRAINT slice_results_knowledge_run_fk
        FOREIGN KEY (tenant_id,workspace_id,knowledge_change_id,knowledge_run_id)
        REFERENCES knowledge_change_runs(tenant_id,workspace_id,change_id,id),
    ADD CONSTRAINT slice_results_knowledge_attempt_fk
        FOREIGN KEY (tenant_id,workspace_id,knowledge_run_id,knowledge_final_attempt_id)
        REFERENCES knowledge_change_attempts(tenant_id,workspace_id,run_id,id),
    ADD CONSTRAINT slice_results_knowledge_binding_check CHECK (
        (knowledge_change_id IS NULL AND knowledge_run_id IS NULL
            AND knowledge_definition_version IS NULL AND knowledge_definition_digest IS NULL
            AND knowledge_final_attempt_id IS NULL AND knowledge_publisher_receipt_id IS NULL
            AND knowledge_publisher_receipt_digest IS NULL
            AND knowledge_result_origin IS NULL)
        OR
        (provenance='knowledge_change_managed'
            AND pipeline_run_id IS NULL AND knowledge_change_id IS NOT NULL
            AND knowledge_run_id IS NOT NULL AND knowledge_final_attempt_id IS NOT NULL
            AND (
                (payload_erased AND knowledge_definition_version IS NULL
                    AND knowledge_definition_digest IS NULL
                    AND knowledge_publisher_receipt_digest IS NULL
                    AND knowledge_result_origin IS NULL)
                OR
                (NOT payload_erased AND knowledge_definition_version IS NOT NULL
                    AND pg_catalog.btrim(knowledge_definition_version)<>''
                    AND knowledge_definition_digest IS NOT NULL
                    AND pg_catalog.btrim(knowledge_definition_digest)<>''
                    AND ((knowledge_result_origin='applied'
                            AND knowledge_publisher_receipt_id IS NOT NULL
                            AND knowledge_publisher_receipt_digest IS NOT NULL
                            AND pg_catalog.btrim(knowledge_publisher_receipt_digest)<>'')
                        OR (knowledge_result_origin='applied_erased'
                            AND knowledge_publisher_receipt_id IS NOT NULL
                            AND knowledge_publisher_receipt_digest IS NULL)
                        OR (knowledge_result_origin='no_change'
                            AND knowledge_publisher_receipt_id IS NULL
                            AND knowledge_publisher_receipt_digest IS NULL)))
            ))
    );
CREATE UNIQUE INDEX slice_results_knowledge_run_unique
    ON slice_results(tenant_id,workspace_id,knowledge_run_id)
    WHERE knowledge_run_id IS NOT NULL;
