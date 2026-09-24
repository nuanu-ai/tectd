-- Legacy links from migration 0059 have no attested node mapping. A new link
-- must carry a nonempty mapping in the same immutable row as its receipt FK.
ALTER TABLE matrix_planning_selection_links
    ADD COLUMN mapped_nodes jsonb;

-- NOT VALID preserves old links as distinguishable, unattestable NULL rows,
-- while PostgreSQL enforces this constraint on every future INSERT.
ALTER TABLE matrix_planning_selection_links
    ADD CONSTRAINT matrix_planning_selection_mapped_nodes_required
    CHECK (CASE WHEN pg_catalog.jsonb_typeof(mapped_nodes) = 'array'
        THEN pg_catalog.jsonb_array_length(mapped_nodes) > 0
        ELSE false END) NOT VALID;
