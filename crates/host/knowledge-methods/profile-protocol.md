# Protocol knowledge: versioned contracts and observed behavior

Apply General and this method to protocol, API and integration knowledge. Keep specification, implementation/provider declaration and observed behavior distinguishable. A provider observation does not establish a universal protocol rule. Preserve network, provider, version and observation bounds in delivery.

Evidence: pin the authoritative specification URI/version and the exact source excerpts or saved outputs used. Map capabilities, request/response or schema contracts, invariants, compatibility restrictions, negative states, quirks and failure behavior to their sources. Distinguish unavailable documentation, unsupported capability, observed failure and documented prohibition. Identify discrepancies between specification and observation instead of silently choosing one.

Create: state which protocol/version/provider/network is described, the supported contract and constraints, expected negative behavior and the limits of observations. Represent compatibility and integration boundaries explicitly. Preserve decisions/risks and open questions that materially affect use. Document a quirk only within the evidence's scope; do not generalize a single response.

Revise: compare version, contract, schema, capability, invariant, compatibility and negative-state changes. Identify breaking implications and consumer assumptions invalidated by the change. Separate a corrected interpretation from an actual upstream change. Old observations remain tied to their original context and do not automatically validate the new version.

Revalidate: use new suitable evidence for the same meaning and scoped protocol context. Recheck the authoritative source and any observations required for provider-specific assertions. Merely fetching the same title or rereading a saved response is insufficient. A changed contract/version with semantic consequences requires revise; lack of permission for a live request remains an evidence gap rather than an invented success.

Supersede: compare successor scope, version, capabilities, compatibility and negative behavior. Enumerate exactly which consumer bindings can move. Do not claim a newer protocol/provider version replaces an older one for every integration. Preserve uncovered legacy needs and explicitly identify migration work.

Retract: withdraw the unsupported or contradicted scoped assertion and expose a required gap to consumers that relied on it. Retain the evidence and original version in history. A retraction of a capability assertion does not remove a deployed integration or prove that it is safe.

Erase: include copied specification/response fragments, examples, provider identifiers where owned, observations and derived protocol summaries in the General copy inventory. Shared specifications remain governed by their own ownership. Redact approved owned copies without changing unrelated protocol knowledge. Do not reproduce restricted request/response values in erasure evidence.

Impact and terminal checks: preserve source authority, version/provider/network, freshness and declared/observed distinctions in exact delivery. Record affected integration assumptions and unresolved conflicts. No runtime, network or compatibility acceptance may be claimed without its actual evidence. Route new research or operational validation as a separate follow-up when needed.

Source: design §7.7, §7.12, §7.16; Tect V1 protocol-knowledge family at ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8.
