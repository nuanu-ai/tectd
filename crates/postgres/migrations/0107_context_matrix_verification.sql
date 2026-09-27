-- Extend the existing verification identity for declaration-bound Matrix tasks.
-- Historical V1 rows and their digests remain unchanged and readable.
ALTER TABLE public.matrix_verifications
    ADD COLUMN frozen_snapshot_id uuid,
    ADD COLUMN requirements_semantic_digest text,
    ADD COLUMN authority_schema text,
    DROP CONSTRAINT matrix_verifications_schema_check,
    DROP CONSTRAINT matrix_verifications_reason_check,
    ADD CONSTRAINT matrix_verifications_version_check CHECK (
        (schema='tect.matrix-verification/1'
         AND verification_reason='matrix_facts_verified'
         AND frozen_snapshot_id IS NULL
         AND requirements_semantic_digest IS NULL
         AND authority_schema IS NULL)
        OR
        (schema='tect.context-matrix-verification/1'
         AND verification_reason='operating_facts_verified'
         AND frozen_snapshot_id IS NOT NULL
         AND frozen_snapshot_id<>'00000000-0000-0000-0000-000000000000'::uuid
         AND requirements_semantic_digest IS NOT NULL
         AND requirements_semantic_digest ~ '^[0-9a-f]{64}$'
         AND authority_schema IS NOT NULL
         AND authority_schema='tect.matrix-requirements/1'));

-- The revision binding exists before verification. This key checks all V2
-- provenance fields together; a snapshot from another task cannot be used.
ALTER TABLE public.matrix_task_requirements_bindings
    ADD CONSTRAINT matrix_task_requirements_verification_key UNIQUE
        (tenant_id,workspace_id,task_id,revision,snapshot_id,semantic_digest,authority_schema);

ALTER TABLE public.matrix_verifications
    ADD CONSTRAINT matrix_verifications_frozen_snapshot_fk
        FOREIGN KEY (tenant_id,workspace_id,frozen_snapshot_id)
        REFERENCES public.matrix_requirements_snapshots(tenant_id,workspace_id,id),
    ADD CONSTRAINT matrix_verifications_requirements_binding_fk
        FOREIGN KEY (tenant_id,workspace_id,task_id,task_revision,
                     frozen_snapshot_id,requirements_semantic_digest,authority_schema)
        REFERENCES public.matrix_task_requirements_bindings
            (tenant_id,workspace_id,task_id,revision,snapshot_id,semantic_digest,authority_schema);

CREATE INDEX matrix_verifications_context_lookup_idx ON public.matrix_verifications
    (tenant_id,workspace_id,task_id,task_revision,frozen_snapshot_id,input_digest,verified_at DESC)
    WHERE schema='tect.context-matrix-verification/1';

-- Preserve all established advice states while allowing explicit V2 no-call
-- outcomes only for Matrix engineering-profile requests.
ALTER TABLE public.advisory_opportunity
    DROP CONSTRAINT advisory_opportunity_reason_check,
    ADD CONSTRAINT advisory_opportunity_reason_check CHECK (primary_reason IN (
        'workspace_disabled','session_skip','request_skip',
        'deterministic_input_invalid','capability_unavailable','provider_unconfigured',
        'budget_policy_invalid','choice_set_not_applicable','matrix_evidence_unresolved',
        'matrix_source_unverified','configuration_changed','matrix_task_revision_changed',
        'matrix_verification_stale','dispatch_authorized','recommendation_prepared',
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
            'capability_unavailable','provider_unconfigured','budget_policy_invalid',
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
        OR (state='prepared' AND primary_reason='recommendation_prepared'
            AND capability='pipeline_recommendation'
            AND decision_point='pipeline_recommendation_before_slice_open')
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
