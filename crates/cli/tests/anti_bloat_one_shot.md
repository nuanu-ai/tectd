# S04 campaign anti-bloat one-shot

`anti_bloat_one_shot.rs` is an ignored public/native MCP integration test. It
creates a source-authored selected draft for the campaign editor Preview: render
the current unsaved subject and body for the selected sample recipient with the
existing renderer in a 600px panel, mark unresolved variables, and neither save
nor send. That P0 candidate is nonrankable. E1 is an unrequested 375/600px
comparison toggle with synchronized scrolling; E2 is an unrequested nonblocking
copy-advice checklist. Both are rankable optional extras. The test does not
preassign which optional extra the provider should rank first. The Choice wire
provides ranking and probabilities, with no narrative explanation.

Use a newly initialized PostgreSQL 18.6 cluster and an empty database named
`tect_s04_live_*`, retained outside the worktree. Supply two loopback TCP URLs
for `postgres` and `tect_ci` and pin the server system identifier, database
OID, port, and database name using `TECT_TEST_EXPECTED_PG_SYSTEM_ID`,
`TECT_TEST_EXPECTED_DB_OID`, `TECT_TEST_EXPECTED_PG_PORT`, and
`TECT_TEST_EXPECTED_DB_NAME`. Set `TECT_TEST_DISPOSABLE_PG=1`,
`TECT_TEST_RUNTIME_ROLE=tect_ci`, `TECT_TEST_ADMIN_URL`, and
`TECT_TEST_RUNTIME_URL`. The harness refuses a nonempty database before
migration. Retain both the cluster and its database after any attempt.

First run `JEV_S04_ONE_SHOT_MODE=preflight cargo test -p tect-cli --test
anti_bloat_one_shot one_shot_campaign_anti_bloat -- --ignored --exact
--nocapture` with the fixture variables above. This path never reads
`TYPESAFE_API_KEY`, constructs no external transport, and proves a native
`provider_unconfigured` no-call with zero budget reservations. It freezes a
request and manifest in owner-only files under the fixed artifacts directory;
both print the request size and SHA-256. The request must remain below 45 KB.
The installed signed test budget is one call, 24,000 input tokens and 2,000
output tokens, with one retry slot only because the schema requires it. This
one-shot test has no retry invocation.

For an authorized later send, start from a different fresh empty database and
set `JEV_S04_ONE_SHOT_MODE=send` with a private process-level
`TYPESAFE_API_KEY`. The send path freezes a new exact request and manifest,
recomputes the provider body, and waits for the exact newline-terminated
`SEND JEV <printed-sha256>` line. It exclusively creates and syncs the fixed
`*.used` marker before starting the configured daemon. A marker or request
artifact already present prevents a second send. Do not delete these files to
retry. After the single native `scope.anti_bloat.run`, the test checks the
durable request and response hashes and replay equality. Only a uniquely
ranked eligible top finding can lead to an explicit single-candidate removal
through `scope.anti_bloat.apply`, followed by a distinct Verifier's whole-plan
preservation attestation. Abstain, tie, invalid content, and transport failure
leave the draft unchanged.

Keep the private database and request/response audit evidence. Do not print
the key or a credential-bearing database URL in a report.
