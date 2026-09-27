# One-shot real JEV Pipeline S03 harness

`s03_live.rs` is an ignored, test-only extension of the public S03 fixture. It
creates a synthetic source, Program, Scope, task, Matrix context, independent
Verifier, matched planning effect, and agent-authored compatibility policy. It
uses the production Pipeline preparation and dispatch path without changing
production defaults. The only external destination configured for `send` is
`https://api.typesafe.ai/v1/systemone` with model `jev-1.13.0` and no HTTP
retries.

The operator supplies a newly initialized, dedicated PostgreSQL 18 database
and its exact system ID and database OID. The test checks both URLs resolve to
the same loopback host, port and database, checks the admin and runtime roles,
requires no non-system user schema or catalog objects, applies migrations 1 through 110, and
checks source checksums for migrations 105 through 110. Keep the cluster and
database after a run. A second invocation needs another **empty** database
with its own OID; a preflight database cannot be reused for `send`.

Set `TECT_TEST_DISPOSABLE_PG=1`, `TECT_TEST_EXPECTED_PG_SYSTEM_ID`,
`TECT_TEST_EXPECTED_DB_OID`, `TECT_TEST_ADMIN_URL`, `TECT_TEST_RUNTIME_URL`,
`TECT_TEST_RUNTIME_ROLE`, and `JEV_PIPELINE_PROFILE_ID` in the process
environment. The admin URL must use `postgres` and the runtime URL must use the
named, distinct login role. Keep credential-bearing URLs out of shared logs.
The profile ID is a local test identity and is mirrored into the workspace
advisory configuration and provider identity. It is not an external account.

Dry run:

```sh
JEV_PIPELINE_ONE_SHOT_MODE=preflight cargo test -p tect-cli --test matrix_context_advisory_native_mcp one_shot_real_s03_pipeline -- --ignored --nocapture
```

Preflight verifies the owner-signed two-call budget using the runtime trust
key, the Matrix and Verifier bindings, exact test compatibility policy digest,
eligible schema/4 manifest, serialized native request and SHA-256, and zero
dispatch rows for the Pipeline opportunity. The synthetic Matrix advice has
its own local fixture dispatch, which durably reserves and consumes exactly
one call. The policy has `provider_calls=2`, `input_tokens=24000`, and
`output_tokens=2000`, leaving at most one Pipeline call after the synthetic
Matrix call. The token ceilings are agent-authored test policy based on the
observed HTTP 200 usage of 14,114 input and 287 output tokens, with margin;
they are not product approval. The pre-marker check compares the exact frozen
request byte length with remaining byte headroom and requires positive signed
input/output token headroom. It does not predict how many tokens the provider
will use. It also checks elapsed time and policy window. Its
`retry_dispatches=1` field is the database schema minimum, and this harness
has no retry path. A separate
ignored `one_call_budget_rejected_before_marker` test proves that a one-call
policy stops before marker creation or Pipeline dispatch. Preflight does not
read `TYPESAFE_API_KEY`, construct a credential-bearing provider, create a
marker, or invoke a provider.

The test fixture admits exactly Lightweight TDD and Deep Brainstorming for
selected Matrix choice `b`; the other six current SliceRun kinds are recorded
as `missing_rule` exclusions. Both admitted options retain their complete
pinned definitions and phase obligations, with exact Matrix and obligation
digests. Preflight asserts that the complete frozen request is below 45,000
bytes. This is a fixture policy choice, not a production serializer change.

The `send` mode exists for a later, separately reviewed live invocation. It
requires `JEV_PIPELINE_ONE_SHOT_ARTIFACT_DIR` to name an existing owner-only
directory outside the checkout, and a nonempty process-level
`TYPESAFE_API_KEY`. It repeats the whole preflight on a new empty database,
creates the exact request JSON with exclusive owner-only permissions and
fsync, and prompts for `SEND JEV PIPELINE <printed-sha256>` followed by a
newline. Only then does it exclusively create and fsync the fixed one-use
marker `tectd-jev-pipeline-s03-2026-09-28-4.used`, before dispatch. This is a
fresh call ID; the prior `-3` marker is consumed and is never reused. An absent
or mismatched line does not mark or send. A process failure after marking
does not authorize a retry; retain the marker, request JSON and database.

The live readback checks the exact marker, exactly one initial dispatch, exact
request bytes and digest, one call reservation with zero retry reservation,
the raw observation, sealed response bytes/hash/reference, accounting and
interpretation according to the reported outcome. It prints the observed
status and IDs without printing raw response bytes. An abstention, transport
uncertainty, timeout, or other failure retains the audit and fails the test;
only durable ranked advice passes.
Inspect persisted evidence privately by the printed opportunity ID:

```sql
SELECT id, state, primary_reason FROM advisory_opportunity WHERE id = '<opportunity-id>';
SELECT id, attempt_number, state, send_certainty, outcome, payload_digest,
       octet_length(request_payload) AS request_bytes, input_tokens,
       output_tokens, raw_response_ref
FROM advisory_dispatch WHERE opportunity_id = '<opportunity-id>'
ORDER BY attempt_number;
SELECT dispatch_id, reserved_calls, reserved_retry_dispatches
FROM advisory_budget_reservations WHERE dispatch_id = '<dispatch-id>';
```

Do not print raw provider response, API key, or credential-bearing URLs to
shared logs. The retained database, request JSON and marker are the audit
record; never delete them to force another send.
