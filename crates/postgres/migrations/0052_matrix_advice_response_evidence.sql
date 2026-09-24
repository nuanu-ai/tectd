-- Advice must retain the configuration and response evidence that produced it.
-- Existing rows cannot be backfilled honestly from the current advice record.
DO $preflight$
BEGIN
    IF EXISTS (SELECT 1 FROM advisory_matrix_advice) THEN
        RAISE EXCEPTION
            'cannot require Matrix advice response evidence: advisory_matrix_advice already contains rows';
    END IF;
END
$preflight$;

ALTER TABLE advisory_matrix_advice
    ADD COLUMN provider_profile_ref text NOT NULL,
    ADD COLUMN model_configuration jsonb NOT NULL,
    ADD COLUMN response_payload_sha256 text NOT NULL,
    ADD CONSTRAINT advisory_matrix_advice_provider_profile_ref_check CHECK (
        pg_catalog.btrim(provider_profile_ref) <> ''
        AND pg_catalog.btrim(provider_profile_ref) = provider_profile_ref
        AND pg_catalog.length(provider_profile_ref) <= 256
    ),
    ADD CONSTRAINT advisory_matrix_advice_model_configuration_check CHECK (
        pg_catalog.jsonb_typeof(model_configuration) = 'object'
        AND model_configuration ? 'model'
        AND pg_catalog.jsonb_typeof(model_configuration->'model') = 'string'
        AND pg_catalog.btrim(model_configuration->>'model') <> ''
        AND pg_catalog.btrim(model_configuration->>'model') = model_configuration->>'model'
        AND pg_catalog.length(model_configuration->>'model') <= 256
        AND model_configuration - 'model' = '{}'::jsonb
    ),
    ADD CONSTRAINT advisory_matrix_advice_response_payload_sha256_check CHECK (
        response_payload_sha256 ~ '^[0-9a-f]{64}$'
    );
