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

The deterministic process and the default model process explicitly keep both delegation features
disabled. This opt-in model process enables `multi_agent` while keeping `multi_agent_v2` disabled;
the proof records the requested overrides and effective feature state separately for each process.
The feature state describes configuration only. Native lineage and tool history enforce and prove
the exactly-one-child boundary.

After the candidate implementation and conditional guidance registry are frozen, run the one
bounded model turn with `--model-turn --scope-candidates`. It opens a Program and selects the owned
Git fixture, begins an ongoing candidate set with a proposed horizontal breakdown, proves exact
begin replay, restarts the owned daemon, and asks Sol to follow only backend-owned continuation
calls. Sol corrects and critically reviews the first result to Ready, then records the exact
separate amendment supplied by the fixture, refreshes, reads the complete two-input context, and
produces a second reviewed Ready result. It then reads compact history and reconstructs the full
original historical context before returning to the current head. The final proof retains complete
call payloads for human semantic review; automated checks cover workflow structure, source refs,
IDs, revisions, both input windows, delta classification, history, both draft receipt replays, and
the absence of execute or Scope-open calls. Candidate count and grouping remain model decisions.

The model proof records every TectD MCP call with its complete arguments, native status,
canonical typed payload, `isError`, and stable error code. An expected rejected attempt is
accepted only when the test names its exact invalid arguments and proves a later successful
corrected call. Transport errors, malformed results, and unclassified failures fail the run.
The model starts as a Sol Executor under the root Astra and may not create descendants unless the
explicit one-child test flag is supplied. That exception applies only to the parent; the child may
not delegate further.

Every native event is appended before validation to a unique JSONL sidecar next to the proof. The
main proof records its absolute path, event count, and SHA-256 digest, while compact checkpoints
capture child discovery, completed or failed MCP calls, and terminal state. Final thread snapshots
retain the complete MCP arguments, results, errors, and latest item status without rewriting the
entire raw event history for every token delta.

Scope-candidate acceptance extends this same harness through `scope_candidates.py`; it does
not create a separate test project. The module supplies schema-independent call capture and exact
recovery validation, preparation of an open Program with one selected owned source through the
existing public DTOs, and assertions for snapshot binding, backend-resolved entity identities,
ordered input and revision transitions, review states, complete delta partitions, historical
reconstruction, and the candidate-only tool boundary. Candidate route calls are added only from
the implemented live host schemas. The owned fixture can restart its copied daemon and verify a
new process/socket while retaining the same temporary PostgreSQL data.

Before the model turn, deterministic calls on that same native thread open only the temporary
workspace and seed one Program. The model can therefore consume a ready `query program.get`
action without performing a bootstrap mutation. Both phases reject a resolved `tectd` server
whose full native status does not point at the owned package, socket, and host config.

The proof contains UTC start/finish times, source HEAD/tree plus changed-file hashes, Codex and product binary
hashes, discovered tool schema hashes, native thread/turn IDs, check results, sanitized result
hashes, fixture AGENTS.md hash, and cleanup status. It contains no database URL, command line,
host credential, or account material. The raw model-event sidecar includes native conversation
events and must be handled as acceptance evidence. It also records before/after hashes
for the persistent Codex config and the nonsecret installed TectD receipts; it never opens the
installed host credential or account authentication file. Installed upgrade receipts and their
captured native and preservation proofs are protected by default as well.
