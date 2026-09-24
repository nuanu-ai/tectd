# One-shot JEV Scope evidence harness

`jev_one_shot.rs` is an ignored integration test. It uses a disposable PostgreSQL
18 fixture, a source-authored two-alternative Scope manifest, the normal
`WorkspaceService::new_with_scope_advisory_adapters` lifecycle, and PostgreSQL
dispatch/audit storage. The production daemon's Deny/Disabled defaults are
unchanged. The provider endpoint is exactly
`https://api.typesafe.ai/v1/systemone`, with model `jev-1.13.0`, 15-second
timeout, no HTTP retries, a 262144-byte request limit and a 65536-byte
response limit. No token or monetary estimate is asserted.

Before any live run, provide a disposable PostgreSQL 18 admin URL, the
corresponding runtime URL, and its runtime role in process environment variables
`TECT_TEST_ADMIN_URL`, `TECT_TEST_RUNTIME_URL`, and `TECT_TEST_RUNTIME_ROLE`.
The harness applies migrations and creates an enrolled host, workspace,
session, Program, candidate set, and source-authored alternatives. These
database writes are restricted to that disposable test database. The URL
values may contain credentials and must not be copied into reports.

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

The live invocation repeats all preflight checks on its own fresh fixture.
Immediately before calling the service, it atomically creates and fsyncs the
fixed one-use marker
`/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919/tectd-jev-scope-evidence-2026-09-24-1.used`.
The marker contains only the fixed call ID and the prepared body SHA-256. It
is intentionally outside the disposable fixture and worktree. If the process
fails or the provider returns an error, retain this marker: rerunning the
test must refuse to send. Never remove it to retry without a separate decision.
The service records the attempted dispatch and response, including an
unknown send certainty if transport fails; provider-reported token usage can
remain unknown. The live output gives the opportunity ID, outcome, dispatch
count, advice presence, and marker location. Query `advisory_opportunity` and
`advisory_dispatch` by that opportunity ID in the disposable test database
for the persisted evidence. A live run asserts at most one dispatch row.
