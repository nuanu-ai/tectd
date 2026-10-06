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
