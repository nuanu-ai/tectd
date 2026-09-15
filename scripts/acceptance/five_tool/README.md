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

The deterministic zero-model phase also exercises the native Scope and Slice-planning lifecycle:
one accepted Scope candidate opens idempotently, a reviewed Debug-work-plus-Decision plan opens its
first Slice, an `externally_reported` result makes the old plan stale, and a refreshed revision
resolves the decision into a reviewed Lightweight successor. A blocked successor then accepts a
later result at its exact incremented revision, retains both results in history, and becomes
terminal when completed. This proves five-tool persistence, freshness, history and branching
mechanics. It does not claim that TectD executed either selected pipeline or
independently verified caller-supplied evidence.

The deterministic discovery checks keep the five public tools and verify the exact `16` query,
`37` command, and `1` execute routes, including `slice.pipeline.checkpoint.resolve`.
Knowledge discovery covers the exact eighteen DK-1 through
DK-4 routes: six DK-1 routes, seven DK-2 lifecycle routes, DK-3 search, the DK-4 Program knowledge
refresh, and three DK-4 maintenance routes. Strict maintenance help
schemas expose the bounded owner query, the three accepted external signal kinds, and exactly one
revalidate, revise, or supersede operation; invalid inputs are rejected before native state. The
Slice catalogue revision4 contains nine executable kinds while only the eight
ordinary kinds create `SlicePipelineRun`; Promotion is owned by the twelve-phase Knowledge Change,
defaults to whole delivery, allows both delivery modes, exposes the seven profile methods, and
reports DK-2 search as `not_configured`. The metadata-only fixture remains inactive, so these are
static public-contract checks rather than positive RDF or complete lifecycle acceptance.

Current inquiry discovery checks include Research12 and Deep Brainstorming10 with
their exact version/digest and immutable begin contract. The optional model smoke
below is not a full research quality assessment; any separately authored model
scenario must report its actual completed phases and source/result evidence.

Add `--model-turn` only for the separately authorized fresh native model smoke. Use
`--postgres-bin /absolute/postgresql-18/bin` when PostgreSQL is elsewhere. `--keep-fixture`
retains the owned temporary database and files for diagnosis; it never retains account auth.
Pass `--protected-artifact /absolute/nonsecret/receipt.json` for any additional installed
receipt outside the standard Codex and TectD locations.

Delegation remains rejected by default. `--allow-one-child-sol` is a test-only opt-in that also
requires `--model-turn --scope-candidates`. It permits the parent to create exactly one
`gpt-5.6-sol` medium child. Live notifications prove the parent collaboration boundary, while
metadata-only `thread/read` validates the parent and binds that child to its parent; both must be
`gpt-5.6-sol` at medium effort. The paged
`thread/loaded/list` inventory rejects any unexpected third actor; the child may be absent from
that inventory and the proof records this fact. The runtime may omit the spawn item; when it
is present its target, model, and effort must match. Child non-MCP actions are not observable in
this runtime and are reported as that limitation rather than claimed as passed.

The deterministic process and the default model process explicitly keep both delegation features
disabled. This opt-in model process enables `multi_agent` while keeping `multi_agent_v2` disabled;
the proof records the requested overrides and effective feature state separately for each process.
The feature state describes configuration only. Native lineage, metadata, and the MCP transport
guard enforce the exactly-one-child TectD boundary.

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

The owned launcher relays the unchanged packaged `run.sh` and records every MCP request and
response byte in a private append-only JSONL sidecar. Records use only
`direction=connection|request|response` and `phase=transparent|guarded`; responses say whether
they were forwarded and identify `mcp_wire` versus `fixture_capture` origin. Each call carries the host-generated
`_meta.threadId`; the relay blocks candidate writes until the first successful child `get_state`
matches the parent-child metadata and the loaded-thread inventory contains no third actor. It never accepts environment
identity, never forwards `execute`, and labels its own fail-closed errors as fixture-capture errors.
The model proof records every TectD MCP call from those paired wire frames with its complete
arguments, result, native actor, canonical payload, `isError`, and stable error code. An expected rejected attempt is
accepted only when the test names its exact invalid arguments and proves a later successful
corrected call. Transport errors, malformed results, and unclassified failures fail the run.
The model starts as a Sol Executor under the root Astra and may not create descendants unless the
explicit one-child test flag is supplied. That exception applies only to the parent; the child may
not delegate further.

Every native event is appended before validation to a private JSONL sidecar next to the proof. A
separate private MCP sidecar keeps exact request/response bytes, global order across connections,
per-connection identity, pairing, and launcher/relay/package hashes. The main proof records the
sidecar paths, counts, and SHA-256 digests. Its public fields retain allowlisted lineage metadata and
the full paired TectD calls, without copying passive model conversation events.

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
