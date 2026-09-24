-- Keep historical rows intact while recognizing a post-send verification drift.
ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled', 'session_skip', 'request_skip',
        'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
        'budget_policy_invalid', 'choice_set_not_applicable', 'matrix_evidence_unresolved',
        'matrix_source_unverified', 'configuration_changed', 'matrix_task_revision_changed',
        'matrix_verification_stale', 'dispatch_authorized', 'provider_response',
        'provider_failure', 'send_unknown'
    )) NOT VALID;

ALTER TABLE advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_state_reason_check,
    ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state = 'no_call' AND primary_reason IN (
            'workspace_disabled', 'session_skip', 'request_skip',
            'deterministic_input_invalid', 'capability_unavailable', 'provider_unconfigured',
            'budget_policy_invalid', 'choice_set_not_applicable'
        ))
        OR (state = 'no_call' AND primary_reason IN (
            'matrix_evidence_unresolved', 'matrix_source_unverified'
        ) AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task')
        OR (state = 'prepared' AND primary_reason = 'dispatch_authorized')
        OR (state = 'awaiting_response' AND primary_reason IN ('dispatch_authorized', 'send_unknown'))
        OR (state = 'advised' AND primary_reason = 'provider_response')
        OR (state = 'invalidated' AND primary_reason = 'configuration_changed')
        OR (state = 'invalidated' AND primary_reason IN (
            'matrix_task_revision_changed', 'matrix_verification_stale'
        ) AND capability = 'engineering_profile' AND work_item_kind = 'matrix_task')
        OR (state = 'failed' AND primary_reason = 'provider_failure')
        OR (state = 'unresolved' AND primary_reason = 'send_unknown')
    ) NOT VALID;
