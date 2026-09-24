# One-shot JEV Scope evidence harness

`jev_one_shot.rs` is an ignored integration test. It uses an isolated PostgreSQL
18 instance, a source-authored two-alternative Scope manifest, the normal
`WorkspaceService::new_with_scope_advisory_adapters` lifecycle, and PostgreSQL
dispatch/audit storage. The production daemon's Deny/Disabled defaults are
unchanged. The provider endpoint is exactly
`https://api.typesafe.ai/v1/systemone`, with model `jev-1.13.0`, 15-second
timeout, no HTTP retries, a 262144-byte request limit and a 65536-byte
response limit. No token or monetary estimate is asserted.

Before any live run, use a dedicated PostgreSQL 18 cluster with a persistent,
owner-chosen data directory outside the worktree. Keep that data directory after
both successful and failed runs, including a failed preflight or transport
failure. Restrict directory access to the owner; do not use an automatically
deleted temporary directory. Provide its admin URL, corresponding runtime URL,
and runtime role in process environment variables
`TECT_TEST_ADMIN_URL`, `TECT_TEST_RUNTIME_URL`, and `TECT_TEST_RUNTIME_ROLE`.
The harness applies migrations and creates an enrolled host, workspace,
session, Program, candidate set, and source-authored alternatives. These
database writes are restricted to that isolated test database. The URL values
may contain credentials and must not be copied into reports, commands captured
for review, or issue comments. Retain the cluster's data directory and record
its path privately before running either mode. Stopping PostgreSQL is safe;
deleting or reinitializing the cluster would discard the audit evidence.

Run the zero-send preflight:

```sh
JEV_ONE_SHOT_MODE=preflight cargo test -p tect-cli --test jev_one_shot one_shot_real_jev_evidence -- --ignored --exact --nocapture
```

The preflight validates PostgreSQL version/migrations, workspace optional
configuration, session and authority binding, authored manifest supplier,
serialized JEV body and size, and a durable `no_call` opportunity with zero
dispatch rows under DenyScopeBudget. It prints the call ID, fixture workspace
and candidate IDs, body byte count and SHA-256, and no-call opportunity ID.
Keep that output as the preflight artifact. It does not read `TYPESAFE_API_KEY`
and cannot make an external JEV request.

After reviewing the preflight artifact and arranging secure process-level
injection of the nonempty `TYPESAFE_API_KEY`, the single live invocation is:

```sh
JEV_ONE_SHOT_MODE=send cargo test -p tect-cli --test jev_one_shot one_shot_real_jev_evidence -- --ignored --exact --nocapture
```

The live invocation repeats all preflight checks on its own fresh fixture. It
creates the exact serialized JSON request at
`/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919/tectd-jev-scope-evidence-2026-09-24-1.request.json`
with exclusive create, owner-only `0600` permissions, and file and parent
directory sync. Review that file and its printed byte count and SHA-256. The
process then waits on stdin for the exact line `SEND JEV <printed-sha256>`.
EOF or any different line fails without creating the marker or making a JEV
request. Once that line is entered, the harness atomically creates and fsyncs
the fixed one-use marker
`/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919/tectd-jev-scope-evidence-2026-09-24-1.used`.
The marker contains only the fixed call ID and the prepared body SHA-256. It
is intentionally outside the isolated cluster and worktree. The service sends
the reviewed bytes once and the harness compares the dispatch audit payload
and digest to the retained JSON. If the process fails or the provider returns
an error, retain the marker and JSON: rerunning the test must refuse to send.
Never remove either artifact to retry without a separate decision.
The service records the attempted dispatch and response, including an
unknown send certainty if transport fails; provider-reported token usage can
remain unknown. The live output gives the opportunity ID, outcome, dispatch
count, advice presence, and marker location. Query `advisory_opportunity` and
`advisory_dispatch` by that opportunity ID in the retained isolated database
for the persisted evidence. A live run requires exactly one dispatch row.
For readback, use the recorded opportunity ID and a private connection session:

```sql
SELECT id, state, primary_reason, eligible_material_ref
FROM advisory_opportunity WHERE id = '<opportunity-id>';
SELECT id, attempt_number, state, send_certainty, outcome, payload_digest,
       octet_length(request_payload) AS request_bytes,
       input_tokens, output_tokens, raw_response_ref
FROM advisory_dispatch WHERE opportunity_id = '<opportunity-id>'
ORDER BY attempt_number;
```

Run these read-only queries with `psql` or an equivalent client that gets its
connection settings privately. Do not echo a credential-bearing URL, `\conninfo`,
or the raw request/response payload into shared logs. After a failure before an
opportunity ID was printed, inspect recent `advisory_opportunity` rows in the
retained database to identify the send invocation's workspace; the earlier
preflight uses a different workspace. Read the same tables and keep the data
directory and request artifact for later review.
