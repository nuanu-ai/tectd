-- Immutable original transport facts. Interpretation never rewrites this receipt.
ALTER TABLE advisory_provider_observations
    ADD COLUMN original_transport_context jsonb
        CHECK (original_transport_context IS NULL OR
            pg_catalog.jsonb_typeof(original_transport_context)='object');
