-- Bind the exact caller-authored request to the prepared manifest. Existing
-- manifests remain NULL and represent legacy requests.
ALTER TABLE advisory_scope_manifest
    ADD COLUMN authored_request_digest text;

ALTER TABLE advisory_scope_manifest
    DROP CONSTRAINT advisory_scope_manifest_digest_check;

ALTER TABLE advisory_scope_manifest
    ADD CONSTRAINT advisory_scope_manifest_digest_check CHECK
        (source_digest ~ '^[0-9a-f]{64}$' AND constructor_digest ~ '^[0-9a-f]{64}$'
         AND baseline_alternative_id ~ '^[0-9a-f]{64}$' AND eligible_set_digest ~ '^[0-9a-f]{64}$'
         AND whole_set_digest ~ '^[0-9a-f]{64}$'
         AND (authored_request_digest IS NULL OR authored_request_digest ~ '^[0-9a-f]{64}$'));
