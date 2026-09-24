-- Owner-authored choices belong to the exact accepted Matrix task revision.
-- Existing revisions remain without choices; later revisions may supply the
-- complete versioned choice set when they are inserted.
ALTER TABLE matrix_task_revisions
    ADD COLUMN choice_set_schema text,
    ADD COLUMN choice_set jsonb,
    ADD COLUMN choice_set_digest text,
    ADD CONSTRAINT matrix_task_revisions_choice_set_check CHECK (
        (choice_set_schema IS NULL AND choice_set IS NULL AND choice_set_digest IS NULL)
        OR (
            choice_set_schema IS NOT NULL
            AND choice_set_schema = 'tect.matrix-choice-set/1'
            AND choice_set IS NOT NULL
            AND pg_catalog.jsonb_typeof(choice_set) = 'object'
            AND choice_set_digest IS NOT NULL
            AND choice_set_digest ~ '^[0-9a-f]{64}$'
        )
    );
