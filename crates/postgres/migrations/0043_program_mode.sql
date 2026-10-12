-- The product level of a Program, its seventh required PRD field. Existing rows stay
-- NULL: there is no default level and nothing is backfilled. A Program without a
-- level must state one in its next save.
ALTER TABLE programs
    ADD COLUMN mode text,
    ADD CONSTRAINT programs_mode_check CHECK (
        mode IS NULL OR mode IN ('demo', 'mvp', 'prod', 'serious_prod')
    );
