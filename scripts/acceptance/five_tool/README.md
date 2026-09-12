# Five-tool native Codex acceptance

This harness proves the consolidated TectD API through the native Codex app-server while all
database, daemon, package, socket, enrollment, source, setup, and AGENTS.md effects remain under
one owned temporary directory. It does not use the installed daemon, PostgreSQL, package,
configuration, host credential, grants, or a real workspace AGENTS.md.

## Prerequisites

- source worktree containing the exact feature candidate; HEAD, status, and every changed file
  hash are bound into the proof;
- build-ready `target/debug/tectd`, `tectd-mcp`, and `tect-admin` from that exact tree;
- PostgreSQL 18 command directory, default `/opt/homebrew/opt/postgresql@18/bin`;
- bundled Codex executable, default `/Applications/Codex.app/Contents/Resources/codex`.

The deterministic phase uses a new unauthenticated temporary `CODEX_HOME`. It exercises native
MCP discovery and calls but does not start a model turn. `--model-turn` additionally starts one
ephemeral `gpt-5.6-sol` medium turn through the ordinary existing Codex auth store. The script
does not read, copy, link, print, or hash account credentials. Omit the flag when that ordinary
authenticated runtime is unavailable; the proof then records the model phase as `not_run`, not
passed.

## Run

From the source root, after the ordinary build and tests are complete:

```sh
python3 scripts/acceptance/five_tool/run.py \
  --proof /absolute/nonsecret/evidence/five-tool-native-proof.json
```

This default run is labeled `exploratory_feature_smoke`, because a parallel feature worktree may
be dirty. The release-quality rerun must use binaries rebuilt from the clean final commit and add
`--final`; the harness then refuses a dirty source tree and labels the proof `final_clean_commit`.

Add `--model-turn` only for the separately authorized fresh native model smoke. Use
`--postgres-bin /absolute/postgresql-18/bin` when PostgreSQL is elsewhere. `--keep-fixture`
retains the owned temporary database and files for diagnosis; it never retains account auth.
Pass `--protected-artifact /absolute/nonsecret/receipt.json` for any additional installed
receipt outside the standard Codex and TectD locations.

Delegation remains rejected by default. `--allow-one-child-sol` is a test-only opt-in that also
requires `--model-turn --scope-candidates`. It permits the parent to create exactly one
`gpt-5.6-sol` medium child and captures both threads through native thread history APIs before
cleanup. The proof distinguishes the spawn request from the child's configured model metadata;
App Server documents that configured model metadata is not per-turn execution telemetry. A
missing or ambiguous spawn request, another child, a grandchild, or any action outside the exact
parent-collaboration and child-TectD boundaries fails the run.

After the candidate implementation and conditional guidance registry are frozen, run the one
bounded candidate scenario with `--model-turn --scope-candidates`. It opens a Program and selects
the owned Git fixture, begins an ongoing candidate set with a proposed tables/API/UI breakdown,
proves exact begin replay, restarts the owned daemon, and asks Sol to follow only backend-owned
continuation calls before correcting and reviewing the proposal. The final proof retains complete
call payloads for human semantic review; automated checks cover workflow structure, source refs,
IDs, revisions, input history, review decisions, receipt replay, and the absence of execute or
Scope-open calls.

The model proof records every TectD MCP call with its complete arguments, native status,
canonical typed payload, `isError`, and stable error code. An expected rejected attempt is
accepted only when the test names its exact invalid arguments and proves a later successful
corrected call. Transport errors, malformed results, and unclassified failures fail the run.
The model starts as a Sol Executor under the root Astra and may not create descendants unless the
explicit one-child test flag is supplied. That exception applies only to the parent; the child may
not delegate further.

Scope-candidate acceptance extends this same harness through `scope_candidates.py`; it does
not create a separate test project. The module currently supplies schema-independent call
capture and exact recovery validation, preparation of an open Program with one selected owned
source through the existing public DTOs, and assertions for snapshot binding, backend-resolved
entity identities, exact ongoing input binding, review states, and the
candidate-only tool boundary. Candidate route calls are added only from the implemented live
host schemas. The owned fixture can restart its copied daemon and verify a new process/socket
while retaining the same temporary PostgreSQL data.

Before the model turn, deterministic calls on that same native thread open only the temporary
workspace and seed one Program. The model can therefore consume a ready `query program.get`
action without performing a bootstrap mutation. Both phases reject a resolved `tectd` server
whose full native status does not point at the owned package, socket, and host config.

The proof contains UTC start/finish times, source HEAD/tree plus changed-file hashes, Codex and product binary
hashes, discovered tool schema hashes, native thread/turn IDs, check results, sanitized result
hashes, fixture AGENTS.md hash, and cleanup status. It contains no database URL, command line,
host credential, account material, or conversational prompt/final response outside the captured
TectD calls. It also records before/after hashes
for the persistent Codex config and the nonsecret installed TectD receipts; it never opens the
installed host credential or account authentication file. Installed upgrade receipts and their
captured native and preservation proofs are protected by default as well.
