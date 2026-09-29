# S04 Active JEV MVP anti-bloat one-shot

`anti_bloat_one_shot.rs` is an ignored public/native MCP integration test. It
clones an exact, clean `codex/tectd-jev-dev` HEAD into a private source snapshot.
Its same-session saved Work has three nonrankable required candidates: Active
JEV MVP, EM02-SCOPE@0.1, and EM02-PROTECT@0.1. A fourth, explicitly
agent-proposed duplicate advisory status panel has no source-goal coverage and
is the only rankable exploratory candidate. This fixture is source-backed test
Work, not a replay of installed or production Active JEV Work; its Scope setup
is synthetic and is not a real Matrix or Scope JEV acceptance. The Choice wire
provides ranking and probabilities, with no narrative explanation.

Use a newly initialized PostgreSQL 18.6 cluster and an empty database named
`tect_s04_live_*`, retained outside the worktree. Supply two loopback TCP URLs
for `postgres` and `tect_ci` and pin the server system identifier, database
OID, port, and database name using `TECT_TEST_EXPECTED_PG_SYSTEM_ID`,
`TECT_TEST_EXPECTED_DB_OID`, `TECT_TEST_EXPECTED_PG_PORT`, and
`TECT_TEST_EXPECTED_DB_NAME`. Set `TECT_TEST_DISPOSABLE_PG=1`,
`TECT_TEST_RUNTIME_ROLE=tect_ci`, `TECT_TEST_ADMIN_URL`, and
`TECT_TEST_RUNTIME_URL`. Set `CODEX_HOME` and `TECT_TEST_ISOLATED_ROOT` to a
fresh private directory; the branch-built daemon receives only that isolated
home. Set `JEV_S04_ARTIFACT_DIR` to a fresh absolute owner-only `0700`
directory outside the checkout. The harness refuses a nonempty database before
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
one-shot test has no retry invocation. The current fixed call ID is
`tectd-jev-s04-active-mvp-2026-09-30-2`; the older campaign `-1` marker must
remain untouched.

For an authorized later send, start from a different fresh empty database and
set `JEV_S04_ONE_SHOT_MODE=send` with a private process-level
`TYPESAFE_API_KEY`. The send path freezes a new exact request and manifest,
recomputes the provider body, and waits for the exact newline-terminated
`SEND JEV <printed-sha256>` line. It exclusively creates and syncs the fixed
`*.used` marker before starting the configured daemon. A marker or request
artifact already present prevents a second send. Do not delete these files to
retry. After the single native `scope.anti_bloat.run`, the test checks the
durable request and response hashes and replay equality. A ranked eligible
finding still cannot apply automatically: the Owner must separately enter the
exact printed `SELECT JEV S04 <request-sha256> <review-id> <finding-id>` line.
Only then does a single-candidate removal through public
`scope.anti_bloat.apply` occur, followed by a distinct Verifier's whole-plan
preservation attestation. The saved MVP, SCOPE and PROTECT candidate bodies
must remain byte-for-byte equivalent in the next revision. No SELECT, abstain,
tie, invalid content, and transport failure leave the draft unchanged. A
foreign finding is denied.

No-network signed-path tests use `loopback_ranked_select`,
`loopback_ranked_no_select`, and `loopback_abstain` modes on separate fresh
databases. These require **no** `TYPESAFE_API_KEY`; the response is controlled
by a local loopback responder and is not a fresh JEV acceptance.

Keep the private database and request/response audit evidence. Do not print
the key or a credential-bearing database URL in a report.
