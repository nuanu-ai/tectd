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
ephemeral `gpt-5.6-sol` low turn through the ordinary existing Codex auth store. The script
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

Before the model turn, deterministic calls on that same native thread open only the temporary
workspace and seed one Program. The model can therefore consume a ready `query program.get`
action without performing a bootstrap mutation. Both phases reject a resolved `tectd` server
whose full native status does not point at the owned package, socket, and host config.

The proof contains UTC start/finish times, source HEAD/tree plus changed-file hashes, Codex and product binary
hashes, discovered tool schema hashes, native thread/turn IDs, check results, sanitized result
hashes, fixture AGENTS.md hash, and cleanup status. It contains no database URL, command line,
host credential, account material, or full user/model text. It also records before/after hashes
for the persistent Codex config and the nonsecret installed TectD receipts; it never opens the
installed host credential or account authentication file.
