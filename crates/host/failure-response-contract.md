# Failure response envelope

Failures measure the serialized MCP content using the actual failure introduction,
JSON diagnostic, actions, response rules, and `isError`. The byte ceiling is the
smaller of 8192 bytes and the transport capacity supplied by the MCP call path.
Convenience wrappers use 8192 bytes.

Normal diagnostics retain their original values. Recovery actions use recognized
route help selectors or known top-level, non-nil UUID identities. Access failures
return no protected diagnostics, state, or recovery actions. Full route contracts
are obtained through the bounded help read.

When a diagnostic exceeds capacity after recovery actions are reduced, dynamic
string fields may be omitted. `error.diagnostic_delivery.complete: false` records
honest partial diagnostic delivery, with error-relative JSON pointers and original
UTF-8 byte lengths in `omitted`. An omission list that cannot fit may itself be
reduced to the explicit incomplete marker. Codes and semantic rules are retained.
There is no diagnostic replay cache or pagination.

A capacity below the minimum MCP error shell returns a distinct envelope failure
and the existing minimal JSON-RPC wire error. The MCP shell cannot be promised
below its minimum size. Byte fixtures do not establish latency or token savings.
