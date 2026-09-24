-- A Matrix revision can have no applicable choice set for a no-call outcome.
-- The separate revision FK still binds that outcome when the digest is null.
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_matrix_binding_check,
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
    ADD CONSTRAINT advisory_opportunity_matrix_revision_fk
    FOREIGN KEY (tenant_id, workspace_id, work_item_id, matrix_task_revision)
    REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision);

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled', 'session_skip', 'request_skip',
        'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
        'budget_policy_invalid', 'choice_set_not_applicable', 'configuration_changed',
        'dispatch_authorized', 'provider_response', 'provider_failure', 'send_unknown'
    ));

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_state_reason_check,
    ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state = 'no_call' AND primary_reason IN (
            'workspace_disabled', 'session_skip', 'request_skip',
            'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
            'budget_policy_invalid', 'choice_set_not_applicable'
        ))
        OR (state = 'prepared' AND primary_reason = 'dispatch_authorized')
        OR (state = 'awaiting_response' AND primary_reason IN ('dispatch_authorized', 'send_unknown'))
        OR (state = 'advised' AND primary_reason = 'provider_response')
        OR (state = 'invalidated' AND primary_reason = 'configuration_changed')
        OR (state = 'failed' AND primary_reason = 'provider_failure')
        OR (state = 'unresolved' AND primary_reason = 'send_unknown')
    );

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
