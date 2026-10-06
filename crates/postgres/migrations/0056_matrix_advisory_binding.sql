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
            AND source_revision IS NOT NULL
            AND source_revision = matrix_task_revision::text
            AND ((matrix_choice_set_digest IS NULL AND state = 'no_call')
                 OR (matrix_choice_set_digest IS NOT NULL
                     AND matrix_choice_set_digest ~ '^[0-9a-f]{64}$')))
    ),
    ADD CONSTRAINT advisory_opportunity_matrix_choice_fk
    FOREIGN KEY (tenant_id, workspace_id, work_item_id,
                 matrix_task_revision, matrix_choice_set_digest)
    REFERENCES matrix_task_revisions
        (tenant_id, workspace_id, task_id, revision, choice_set_digest);

ALTER TABLE advisory_opportunity
    ADD CONSTRAINT advisory_opportunity_matrix_revision_fk
    FOREIGN KEY (tenant_id, workspace_id, work_item_id, matrix_task_revision)
    REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision);


-- Guard the dispatch boundary even if an administrative caller inserts directly.
CREATE FUNCTION advisory_dispatch_require_matrix_choice() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, public
AS $matrix_choice$
DECLARE missing_choice boolean;
BEGIN
    SELECT o.capability = 'engineering_profile'
           AND o.matrix_choice_set_digest IS NULL
      INTO missing_choice
      FROM public.advisory_opportunity AS o
     WHERE o.tenant_id = NEW.tenant_id
       AND o.workspace_id = NEW.workspace_id
       AND o.id = NEW.opportunity_id
     FOR SHARE;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'advisory dispatch opportunity is unavailable'
            USING ERRCODE = '23503';
    END IF;
    IF missing_choice IS TRUE THEN
        RAISE EXCEPTION 'engineering profile dispatch requires a Matrix choice set'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END
$matrix_choice$;

CREATE TRIGGER advisory_dispatch_matrix_choice_guard
    BEFORE INSERT ON advisory_dispatch
    FOR EACH ROW EXECUTE FUNCTION advisory_dispatch_require_matrix_choice();

REVOKE ALL PRIVILEGES ON FUNCTION advisory_dispatch_require_matrix_choice() FROM PUBLIC;
