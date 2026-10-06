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

-- Preserve all established advice states while allowing explicit V2 no-call
-- outcomes only for Matrix engineering-profile requests.
ALTER TABLE public.advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled','session_skip','request_skip',
        'deterministic_input_invalid','capability_unavailable','provider_unconfigured',
        'budget_policy_invalid','budget_exhausted_before_dispatch','choice_set_not_applicable','matrix_evidence_unresolved',
        'matrix_source_unverified','configuration_changed','matrix_task_revision_changed',
        'matrix_verification_stale','dispatch_authorized',
        'provider_response','provider_failure','send_unknown',
        'budget_exhausted_after_response',
        'matrix_task_unbound','matrix_snapshot_missing','matrix_binding_mismatch',
        'matrix_context_unresolved','matrix_context_stale',
        'matrix_authority_schema_unsupported','matrix_operating_evidence_unresolved'
    )) NOT VALID;
ALTER TABLE public.advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_state_reason_check,
    ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (
        (state='no_call' AND primary_reason IN (
            'workspace_disabled','session_skip','request_skip','deterministic_input_invalid',
            'capability_unavailable','provider_unconfigured','budget_policy_invalid','budget_exhausted_before_dispatch',
            'choice_set_not_applicable'))
        OR (state='no_call' AND primary_reason IN
            ('matrix_evidence_unresolved','matrix_source_unverified')
            AND capability='engineering_profile' AND work_item_kind='matrix_task')
        OR (state='no_call' AND primary_reason IN (
            'matrix_task_unbound','matrix_snapshot_missing','matrix_binding_mismatch',
            'matrix_context_unresolved','matrix_context_stale',
            'matrix_authority_schema_unsupported','matrix_operating_evidence_unresolved')
            AND capability='engineering_profile' AND work_item_kind='matrix_task')
        OR (state='prepared' AND primary_reason='dispatch_authorized')
        OR (state='awaiting_response' AND primary_reason IN ('dispatch_authorized','send_unknown'))
        OR (state='advised' AND primary_reason='provider_response')
        OR (state='invalidated' AND primary_reason='configuration_changed')
        OR (state='invalidated' AND primary_reason IN
            ('matrix_task_revision_changed','matrix_verification_stale')
            AND capability='engineering_profile' AND work_item_kind='matrix_task')
        OR (state='failed' AND primary_reason IN
            ('provider_failure','budget_exhausted_after_response'))
        OR (state='unresolved' AND primary_reason='send_unknown')
    ) NOT VALID;
