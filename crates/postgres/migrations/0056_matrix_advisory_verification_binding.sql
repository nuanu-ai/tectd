-- Positive Matrix advice is bound to one immutable verification header.
-- Scope rows and terminal Matrix no-call rows retain NULL. NOT VALID keeps
-- historical positive rows readable while enforcing the rule on new writes.
ALTER TABLE advisory_opportunity
    ADD COLUMN matrix_verification_digest text,
    ADD CONSTRAINT advisory_opportunity_matrix_verification_digest_check
        CHECK (matrix_verification_digest IS NULL OR
               matrix_verification_digest ~ '^[0-9a-f]{64}$'),
    ADD CONSTRAINT advisory_opportunity_matrix_verification_capability_check
        CHECK ((capability = 'scope_decomposition' AND matrix_verification_digest IS NULL)
               OR (capability = 'engineering_profile' AND
                   (state NOT IN ('prepared', 'awaiting_response', 'advised')
                    OR matrix_verification_digest IS NOT NULL))) NOT VALID,
    ADD CONSTRAINT advisory_opportunity_matrix_verification_fk
        FOREIGN KEY (tenant_id, workspace_id, work_item_id,
                     matrix_task_revision, matrix_verification_digest)
        REFERENCES matrix_verifications
            (tenant_id, workspace_id, task_id, task_revision, record_digest);
