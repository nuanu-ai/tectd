-- A terminal Promotion Change always resolves its bound Slice. Successful
-- outcomes complete it; rejected or partial outcomes retain one managed
-- blocked Result and advance planning with the exact remaining work.
ALTER TABLE slice_results DROP CONSTRAINT slice_results_knowledge_binding_check;
ALTER TABLE slice_results ADD CONSTRAINT slice_results_knowledge_binding_check CHECK (
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
                    OR (knowledge_result_origin IN ('no_change','rejected')
                        AND knowledge_publisher_receipt_id IS NULL
                        AND knowledge_publisher_receipt_digest IS NULL)))
        ))
);
