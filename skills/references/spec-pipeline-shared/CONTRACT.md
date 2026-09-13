# Custom Spec Pipeline Contract v1

The four-stage specification workflow writes five JSON sidecars in the active
specification workspace. Markdown design artifacts remain the implementation
source; these sidecars prove that normative source requirements were not lost.

All IDs are case-sensitive and stable for the lifetime of the specification.
All arrays are required, even when empty.

## `requirements-ledger.json`

```json
{
  "schemaVersion": "1.0",
  "source": { "path": "source-spec.md", "digest": "sha256:..." },
  "sourceRequirementIds": ["REQ-001"],
  "requirements": [
    {
      "id": "REQ-001",
      "sourceRef": "source-spec.md#billing",
      "text": "Billing must reject cross-tenant writes.",
      "modality": "MUST",
      "scope": "in",
      "owner": "billing-service",
      "observableOutcomes": ["A same-tenant write commits."],
      "negativeCases": ["A cross-tenant write creates no durable side effect."],
      "status": "covered"
    }
  ]
}
```

Allowed modalities are `MUST`, `SHOULD`, and `MAY`. Allowed scopes are `in`
and `out`. Allowed statuses are `covered`, `deferred`, `ambiguous`,
`untestable`, and `gap`.

A deferred in-scope requirement additionally requires:

```json
{
  "deferral": {
    "approvedBy": "human identity or recorded decision",
    "rationale": "why scope changed",
    "sourceRef": "decision record proving authority"
  }
}
```

## `decision-traceability.json`

```json
{
  "schemaVersion": "1.0",
  "requirements": [
    {
      "requirementId": "REQ-001",
      "decisionRefs": ["decisions/tenant-boundary.md"],
      "acceptanceObligationIds": ["OBL-REQ-001-POSITIVE", "OBL-REQ-001-NEGATIVE"]
    }
  ]
}
```

Every non-deferred in-scope requirement has exactly one trace row with at
least one decision reference and references to both positive and negative
acceptance obligations.

## `acceptance-obligations.json`

```json
{
  "schemaVersion": "1.0",
  "obligations": [
    {
      "id": "OBL-REQ-001-NEGATIVE",
      "requirementIds": ["REQ-001"],
      "kind": "negative",
      "owner": "billing-service",
      "observable": "No durable order or billing event exists.",
      "verification": "integration test plus event-store query"
    }
  ]
}
```

Allowed kinds are `positive` and `negative`.

## `reconciliation-closure.json`

```json
{
  "schemaVersion": "1.0",
  "mode": "closure-audit",
  "unresolvedRequirementIds": [],
  "completionClaim": "ready"
}
```

Allowed modes are `findings-resolution` and `closure-audit`. Allowed claims
are `ready` and `blocked`. A ready claim with unresolved IDs is invalid.

## `synthesis-traceability.json`

```json
{
  "schemaVersion": "1.0",
  "requirements": [
    {
      "requirementId": "REQ-001",
      "modality": "MUST",
      "sectionRefs": ["implementation-ready-spec.md#tenant-boundary"]
    }
  ],
  "completionClaim": "ready"
}
```

Every non-deferred in-scope requirement is mapped to at least one exact section
and keeps the modality recorded in the requirements ledger.

## Validation

Before synthesis, reconciliation can validate the first four artifacts:

```bash
node "${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/validate-spec-pipeline.js" . --stage reconciliation
```

After synthesis, run the final five-artifact validation:

```bash
node "${CODEX_HOME:-$HOME/.codex}/custom-spec-shared/validate-spec-pipeline.js" .
```

The validator is read-only, reports every structural error it can find, and
exits non-zero when the completion contract is unsupported.
