# Claude Code native session connection

This integration uses the same `tectd-mcp` executable as the stdio MCP server.
Its `claude-pre-tool-use` subcommand is a synchronous native command hook. It reads
one official `PreToolUse` JSON event from stdin, publishes a bounded record, and
returns with **empty stdout**. It emits no approval decision, replacement input,
session hint or MCP message. Success exits `0`; failure exits `2`, the official command-hook blocking status,
and exposes only a fixed error code on stderr. Normal MCP exit behavior is unchanged.

## Build and install

From a verified clean release commit, build the shared bridge:

```sh
cargo build --locked --release -p tect-cli --bin tectd-mcp
install -m 755 target/release/tectd-mcp /ABS/APPROVED/RELEASE/tectd-mcp
```

Use that exact installed path in both templates below. Installing this client
binary requires no daemon replacement or database migration.

## Install configuration

Use an existing enrolled host and its private host configuration. Obtain the
**public `host_id`** from that enrollment's metadata; this is the hook's
`--destination-host-id`, not a new identity. Do not copy credentials into hooks,
create another login, or change host authorization for this integration.

1. Choose a canonical absolute directory, outside the repository, owned by the
   account running Claude. Create it with mode `0700` (for example `install -d -m
   700 /ABS/PRIVATE/claude-attestations`). Every path component must be free of
   symlinks. On macOS resolve `/var` and temporary-directory aliases first.
2. Substitute the executable path, enrolled public host UUID and private directory
   in [settings.example.json](settings.example.json). Merge its `hooks` object
   into Claude's user settings (`~/.claude/settings.json`) or the approved project
   settings. Preserve existing hooks. The exact matcher is `mcp__tectd__.*`.
3. Substitute existing socket, enrolled host configuration, logical workspace key,
   executable and directory in [mcp.example.json](mcp.example.json). Install the
   `tectd` entry using Claude's standard MCP configuration at the chosen scope.
   The MCP alias, hook `--server-alias` and `TECT_CLAUDE_MCP_SERVER_ALIAS` must all
   be `tectd`. A different approved alias requires changing all three and matcher.
4. Start a fresh Claude invocation with those settings and verify the acceptance
   checklist below. These files are deterministic templates, not an installer;
   no service, Python runtime or plugin is needed.

Configuration format: [Claude command hooks](https://code.claude.com/docs/en/hooks)
and [Claude MCP configuration](https://code.claude.com/docs/en/mcp).
Environment variables select the provider, record location and alias. They never
provide native session identity. The MCP server gets its destination host from
its existing enrolled host configuration; the writer's explicit public UUID must
match that host. Never supply `CLAUDE_SESSION_ID`, `--session-id`, manual headers,
PID identities, transcript/history lookups or generated UUIDs as a substitute.

## Binding and trust

The original hook `session_id` must be a canonical nonnil UUID. The writer keeps
`tool_use_id` byte-for-byte and hashes its UTF-8 bytes with SHA-256 for the filename.
It records the original `tool_name` and object `tool_input`, alias, destination host,
version and timestamps. Extra official hook fields are accepted and ignored.

The MCP provider requires native `params._meta["claudecode/toolUseId"]`. It checks
that against the record, `mcp__tectd__<RPC tool name>`, and parsed JSON structural
equality of the original `tool_input` with the RPC's top-level `arguments` object.
Integer and floating-point values are distinct; omitted fields differ from null;
array order matters. Omitted top-level `arguments`, null, or an array is rejected.
If `threadId` is also present, it must match the hook session. Missing arguments,
missing record, changed arguments, stale records, mismatched IDs or an unsafe file
fail before daemon delivery. Hooks that modify tool input **after this hook are
unsupported**: their changed RPC arguments fail closed. The original hook remains
the source; no hint in tool arguments or environment can override it.

This is local provenance within the existing same-user Unix host trust boundary,
not cryptographic attestation. The final context directory is mode `0700` and owned
by the effective user; ancestors are traversed without following symlinks, without
requiring that every ancestor have that owner or mode. Records are mode `0600`,
owned by the effective user and have one link. Publication uses a fully written,
fsynced temporary file, exclusive atomic hard-link publication, removal of the
temporary link, and directory fsync before success. It never overwrites records.
A retry with the same semantic event preserves the original bytes and expiry;
conflicting, malformed, unsafe or stale reuse fails. TTL is **120 seconds** with
zero future timestamp allowance. JSON input and record are capped at **8 MiB**.
Expiry rejects identity resolution; it does **not** delete stored `tool_input`.
Private operational attestation records remain until operator cleanup. No background
cleaner or PostToolUse removal runs. After all clients using the holder and their
in-flight calls have finished, remove only expired records owned by that user, or a
closed invocation's isolated context directory. Do not remove an active shared host
directory, live records, or extend expiry to recover a failed call.

## Compatibility and acceptance limits

The historical implementation was probed with official Claude Code **2.1.280**. That PreToolUse probe confirmed its CLI session ID
matches hook `session_id`, hook `tool_use_id` matches unchanged RPC
`_meta["claudecode/toolUseId"]`, and `mcp__tectd__get_state` maps to RPC `get_state`
with structurally matching `{}` input. The hook record was atomically fsynced
before the RPC (observed interval 2.29 seconds). That protocol probe does not prove
installation or session lifecycle behavior on a new target.

The current source port is validated by provider and writer tests plus a disposable
stdio/Unix-socket/PostgreSQL fixture. That fixture proves that two MCP processes
using the same attested native UUID recover the same TectD session, while a distinct
UUID gets a separate session. It uses synthetic fixture events and does not prove a
fresh native Claude launch or native resume. Acceptance remains pending for Claude
Code **2.1.283** on the actual configured CLI and enrolled host. Verify: fresh `get_state` reaches the daemon under the original session;
changed input/missing hook are rejected; retries do not renew timestamps; a new
session and `/clear` do not reuse an old identity; fork has the native client's
new identity; resume uses its original native identity with fresh call records;
and concurrent invocations stay distinct. Record native hook and RPC evidence
without publishing raw tool payloads, transcripts or host credentials.
