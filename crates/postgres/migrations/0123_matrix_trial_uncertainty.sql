-- Additive DEV/TEST uncertainty evidence. Historical strict receipts remain
-- NULL/NULL and retain their original digest. Trial Ranked writes are still
-- blocked by the store until the public readback contract is implemented.
ALTER TABLE advisory_matrix_advice
    ADD COLUMN ranking_policy_version text,
    ADD COLUMN trial_uncertainty jsonb,
    ADD CONSTRAINT advisory_matrix_advice_trial_uncertainty_pair_check CHECK (
        (ranking_policy_version IS NULL AND trial_uncertainty IS NULL)
        OR (ranking_policy_version = 'tect.matrix-native-ranking-policy/robust-trial-v1'
            AND kind = 'ranked'
            AND trial_uncertainty IS NOT NULL
            AND pg_catalog.jsonb_typeof(trial_uncertainty) = 'object'
            AND trial_uncertainty->>'policy_id' = 'tect.matrix-native-ranking-policy'
            AND trial_uncertainty->>'policy_version' = ranking_policy_version
            AND trial_uncertainty->>'policy_digest' ~ '^[0-9a-f]{64}$'
            AND pg_catalog.jsonb_typeof(trial_uncertainty->'scores') = 'array'
            AND pg_catalog.jsonb_array_length(trial_uncertainty->'scores') = 2)
    );
