-- Preserve historical unresolved observations while allowing a distinct,
-- authenticated verifier to record an independent pass or fail.
ALTER TABLE advisory_scope_selected_save_observation
    DROP CONSTRAINT advisory_scope_selected_save_observation_qualification_check;
ALTER TABLE advisory_scope_selected_save_observation
    ADD CONSTRAINT advisory_scope_selected_save_observation_qualification_check
    CHECK (qualification IN ('unresolved', 'independently_observed'));
