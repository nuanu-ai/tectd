# TectD MCP

Independent plugin ID and MCP server key: `tectd`. Display name: **TectD MCP**.
MCP `serverInfo.name` and executable: `tectd-mcp`. This package does not replace
`tect@tect-local` or its `tect-dynamic-materialization` server.

The source template is packaged together with a built binary by
`scripts/package-codex-plugin.py --binary /absolute/tectd-mcp --output /absolute/new-parent`.
The output is `new-parent/tectd`; an existing output is refused. No installation,
marketplace mutation, daemon startup, migration or enrollment is performed.

The Codex process supplies three explicitly configured environment variables:
`TECT_SOCKET`, `TECT_HOST_CONFIG`, and `TECT_WORKSPACE_KEY`. They identify a private
Unix socket, an enrolled mode-0600 credential file, and a logical workspace key.
The package forwards only these names. It contains no secret or machine-specific
configuration. The operator provides them to the launcher or uses equivalent
explicit `mcp_servers.tectd.env` settings in the selected Codex configuration.
An absent setting fails closed. Keep daemon and database lifecycle separate.

Native identity comes from each tool call's host-generated `params._meta.threadId`.
No CODEX_SESSION_ID or CODEX_THREAD_ID environment setting is required or accepted
as an identity fallback. Both the Codex-generated native UUID and host credential
are required before a business operation. Model tool arguments contain no identity.

The public MCP surface is exactly `get_state`, `query`, `command`, `execute`, and
`help`. Read-only routes are `program.get`, `program.list`, `source.list`, and
`setup.get`. Logical transitions are `workspace.open`, `source.register`,
`session.select_worktrees`, `program.begin`, `program.save`,
`program.record_input`, `setup.inspect`, `setup.begin`, `setup.save`, and
`setup.record_input`. The only external-effect route is `setup.apply`. Each native
session has its own selected worktrees. Program drafts, original input and PRDs live
in the database. The focused `tectd-program` and `tectd-setup` methods are embedded in the
executable and returned by `help` describe. Opening a Program does not launch Scope or
implementation work. Tool results carry a short introduction and one JSON content
block with data and exact next actions, without duplicate structured content.
The enrolled host remains the
credential trust boundary; this is not a per-session secret scheme.

For direct Codex configuration use server key `tectd`, the packaged `sh ./run.sh`
entry with its absolute package directory as `cwd`, and the three settings above.
This package cwd is not the setup target. The model supplies the actual current
task launch directory from its environment to `command` route `setup.inspect`, without asking the
human to choose a folder. The enrolled host needs an independent `--setup-root`
grant for that path; source grants do not authorize AGENTS.md publication. Setup
persists its narrative/draft/question in PostgreSQL and creates only a verified
missing AGENTS.md. It preserves existing files and recovers the same ready revision
after an uncertain result. No Codex client changes or extra configuration variables
are needed for task-directory delivery; native ID authentication does not attest cwd.
Actual Codex app-server acceptance and persistent desktop installation are distinct
proofs; consult the parent Scope result for the exact accepted build and status.
