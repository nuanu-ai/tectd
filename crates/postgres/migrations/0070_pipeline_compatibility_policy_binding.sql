-- Preserve the host-owned compatibility decision in each immutable advice
-- context. Historical schema/1 rows remain readable but cannot become new
-- schema/2 material by changing their saved payload.
ALTER TABLE pipeline_advice_contexts
    ADD COLUMN compatibility_policy_digest text,
    ADD CONSTRAINT pipeline_advice_compatibility_policy_digest_check CHECK (
        (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
            AND compatibility_policy_digest IS NULL)
        OR (manifest_payload->>'schema' = 'tect.pipeline-recommendation/2'
            AND compatibility_policy_digest ~ '^[0-9a-f]{64}$'
            AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest)
    ) NOT VALID,
    DROP CONSTRAINT pipeline_advice_context_manifest_shape_check,
    ADD CONSTRAINT pipeline_advice_context_manifest_shape_check CHECK (COALESCE((
        manifest_payload IS NOT NULL
        AND pg_catalog.jsonb_typeof(manifest_payload) = 'object'
        AND manifest_payload->>'schema' IN
            ('tect.pipeline-recommendation/1', 'tect.pipeline-recommendation/2')
        AND manifest_payload->>'work_id' = work_node_id::text
        AND manifest_payload->>'work_revision' = work_node_revision::text
        AND manifest_payload->>'catalogue_revision' = catalogue_revision
        AND manifest_payload->>'catalogue_digest' = catalogue_digest
        AND pg_catalog.jsonb_typeof(manifest_payload->'options') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'options') =
            pg_catalog.cardinality(eligible_kind_ids)
        AND pg_catalog.jsonb_typeof(manifest_payload->'mandatory_card_ids') = 'array'
        AND pg_catalog.jsonb_array_length(manifest_payload->'mandatory_card_ids') > 0
        AND (manifest_payload->>'schema' = 'tect.pipeline-recommendation/1'
             OR (manifest_payload->>'matrix_input_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'selected_candidate_digest' ~ '^[0-9a-f]{64}$'
                 AND manifest_payload->>'compatibility_policy_digest' = compatibility_policy_digest
                 AND pg_catalog.jsonb_typeof(manifest_payload->'excluded') = 'array'))
    ), false)) NOT VALID;
