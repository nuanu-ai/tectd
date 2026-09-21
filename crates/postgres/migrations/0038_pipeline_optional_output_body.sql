-- v0.7 compact phase outputs may carry their complete semantic result in
-- typed fields and omit the legacy free-form body. Keep the column non-null
-- for old readers and retain the existing two-MiB storage bound.
ALTER TABLE slice_pipeline_phase_outputs
    DROP CONSTRAINT slice_pipeline_phase_outputs_body_check,
    ADD CONSTRAINT slice_pipeline_phase_outputs_body_check
        CHECK (pg_catalog.octet_length(body) <= 2097152);
