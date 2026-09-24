-- Distinguish a post-send Matrix task edit from a workspace configuration change.
-- The new invalidation reason is limited to Matrix engineering opportunities.
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled', 'session_skip', 'request_skip',
        'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
        'budget_policy_invalid', 'choice_set_not_applicable', 'configuration_changed',
        'matrix_task_revision_changed', 'dispatch_authorized', 'provider_response',
        'provider_failure', 'send_unknown'
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
        OR (state = 'invalidated' AND primary_reason = 'matrix_task_revision_changed'
            AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task')
        OR (state = 'failed' AND primary_reason = 'provider_failure')
        OR (state = 'unresolved' AND primary_reason = 'send_unknown')
    );
