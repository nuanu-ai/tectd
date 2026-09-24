-- Bind an engineering-profile opportunity to the exact owner-recorded Matrix
-- revision and its choice set. Existing Scope opportunities keep null bindings.
ALTER TABLE matrix_task_revisions
    ADD CONSTRAINT matrix_task_revisions_choice_binding_unique
    UNIQUE (tenant_id, workspace_id, task_id, revision, choice_set_digest);

ALTER TABLE advisory_opportunity
    ADD COLUMN matrix_task_revision bigint,
    ADD COLUMN matrix_choice_set_digest text;

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_decision_check,
    ADD CONSTRAINT advisory_opportunity_decision_check CHECK (
        decision_point IN (
            'scope.decomposition.before_selection',
            'engineering.profile.before_selection'
        )
        AND pg_catalog.btrim(policy_version) <> ''
        AND pg_catalog.btrim(request_key) <> ''
        AND config_revision >= 0
    ),
    DROP CONSTRAINT advisory_opportunity_decision_capability_check,
    ADD CONSTRAINT advisory_opportunity_decision_capability_check CHECK (
        (capability = 'scope_decomposition'
         AND decision_point = 'scope.decomposition.before_selection')
        OR (capability = 'engineering_profile'
            AND decision_point = 'engineering.profile.before_selection')
    ),
    ADD CONSTRAINT advisory_opportunity_matrix_binding_check CHECK (
        (capability = 'scope_decomposition'
         AND decision_point = 'scope.decomposition.before_selection'
         AND matrix_task_revision IS NULL
         AND matrix_choice_set_digest IS NULL)
        OR (capability = 'engineering_profile'
            AND decision_point = 'engineering.profile.before_selection'
            AND work_item_kind = 'matrix_task'
            AND work_item_id IS NOT NULL
            AND scope_id IS NULL
            AND matrix_task_revision IS NOT NULL
            AND matrix_task_revision >= 1
            AND matrix_choice_set_digest IS NOT NULL
            AND matrix_choice_set_digest ~ '^[0-9a-f]{64}$')
    ),
    ADD CONSTRAINT advisory_opportunity_matrix_choice_fk
    FOREIGN KEY (tenant_id, workspace_id, work_item_id,
                 matrix_task_revision, matrix_choice_set_digest)
    REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision, choice_set_digest);
