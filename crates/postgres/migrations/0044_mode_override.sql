-- The product level of one opened Scope or one opened Slice where the user permitted
-- another than the one it would inherit. Two columns each: the level, and the id of
-- the record the permission was quoted from, which is a source reference of the
-- Scope candidate snapshot for a Scope and a Slice planning input for a Slice. The
-- user's words are not copied here; they stay in that input and in the candidate.
-- Existing rows stay NULL: there is no default and nothing is backfilled. Both
-- columns are set together or not at all.
ALTER TABLE native_scopes
    ADD COLUMN mode_override text,
    ADD COLUMN mode_override_source_ref_id uuid,
    ADD CONSTRAINT native_scopes_mode_override_check CHECK (
        (mode_override IS NULL AND mode_override_source_ref_id IS NULL)
        OR (mode_override IS NOT NULL AND mode_override_source_ref_id IS NOT NULL
            AND mode_override IN ('demo', 'mvp', 'prod', 'serious_prod'))
    );
ALTER TABLE native_slices
    ADD COLUMN mode_override text,
    ADD COLUMN mode_override_input_id uuid,
    ADD CONSTRAINT native_slices_mode_override_check CHECK (
        (mode_override IS NULL AND mode_override_input_id IS NULL)
        OR (mode_override IS NOT NULL AND mode_override_input_id IS NOT NULL
            AND mode_override IN ('demo', 'mvp', 'prod', 'serious_prod'))
    );
