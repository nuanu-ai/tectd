-- Freeze the native source-ref kind partition independently of goal links.
-- Existing bindings remain fail-closed at read time unless their exact
-- persisted kind partition is empty; no historical source interpretation is
-- inferred by this migration.
ALTER TABLE scope_anti_bloat_bindings
    ADD COLUMN non_goal_source_obligation_ids jsonb NOT NULL DEFAULT '[]'::jsonb
    CHECK (pg_catalog.jsonb_typeof(non_goal_source_obligation_ids) = 'array');
