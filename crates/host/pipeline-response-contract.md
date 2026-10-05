# Pipeline snapshot references and pinned reads

All begin, mutation and Current replies use the same compact projection for every
pipeline kind and stored version. They retain run identity, revision, status,
current phase, definition identity and digest, source references, brief counts,
and truthful output availability. They identify their scope as
`delivery_scope: "snapshot_reference"`.

A `delivery_receipt` is an immutable backend-issued availability reference for the
stored definition snapshot at a run revision. Its fixed fields are `delivery_id`,
`run_id`, `context_epoch`, `manifest_digest`, and `delivered_at`.
`manifest_digest` is the stored run's `definition_digest`. The receipt proves
neither full body delivery nor reading nor consumption, and completion does not
use it as a consumption gate. `delivery_fresh` means only that the record was
newly inserted. It never triggers body replay. Skill, resource, validator and
knowledge receipt contracts remain independent.

Current may allocate a snapshot-reference receipt. Its compact output guard runs
inside the application transaction before commit, using the same projection and
encoder as the final reply. An encoding failure therefore prevents committing
that transaction. Database rollback/concurrency behavior requires SQL integration
acceptance; pure byte fixtures do not establish that runtime proof.

## Read destinations

All routes below use `slice.pipeline.context` and the authenticated run scope.
Every page repeats authorization and the original source pins. These views are
read-only and allocate no receipts:

| View | Required pins | Complete payload |
| --- | --- | --- |
| `snapshot` | `run_id`, `definition_digest` | Stored definition, including all method bodies, phase definitions, completion/escalation contracts and forbidden claims |
| `phase_contract` | `run_id`, `definition_digest`, exact `phase_id` | Complete stored static phase, including instructions, skills, resources, schemas, constraints and verdict routes |
| `details` | `run_id`, `run_revision`; optional `section` | Full moved context fields in the sections below |
| `output` | `run_id`, `output_id`, original body `digest` | Complete exact output, including fields, artifacts and receipts |
| `delivery_receipt` | `run_id` | Existing current-epoch snapshot-reference receipt; missing receipt returns a refusal with an exact Current call |

The `details` sections are:

- `inputs`: phase-local inputs and source amendments, knowledge manifests and
  status, resource manifests and status, plus current legacy consumption refs.
  Version 0.7 reads omit caller-forbidden consumption parameter fields.
- `outputs`: bindings, available output records, original `outputs_complete`, and
  `erasure.payloads_omitted`. Omitted payloads never appear as a complete empty set.
- `history`: attempts, checkpoints, inquiry, result, source checkpoint,
  qualification reason and delivered-phase history.
- `all` (default): the union of these three sections.

Large legacy consumption parameters move from the compact action into a pinned
`details` inputs read. The bounded action explicitly requires reading and copying
those exact references before submission. Their acceptance contract is unchanged.
A historical retired reply advertises reads; a fresh retired Current keeps the
existing exact migration action. Static stored snapshots remain readable after
retirement when authorization succeeds.

## JSON fragments

`output`, `snapshot`, `phase_contract`, and `details` accept `offset_bytes`,
`limit_bytes`, and `representation_digest`. The default offset is 0 and the
default limit is 4096; explicit limits must be in 1..=4096. A nonzero offset
requires the representation digest. `slice.pipeline.instruction` uses the same
fragment parameters and still requires exact instruction/version/digest pins and
`refresh: true`.

The representation is deterministic serde_json UTF-8 serialization of the entire
authorized read payload. Its SHA256 `representation_digest` is separate from the
stored definition/output/instruction digest. This is not an RFC8785 claim.
A representation change refuses the continuation rather than mixing versions.
An offset must be a UTF-8 boundary in 0..=total_bytes; an offset equal to total
returns an empty final fragment.

Small reads without explicit pagination retain their original complete shape.
Every fragment declares `kind: "fragment"`, JSON/UTF-8 format, original source
pins, representation digest, total/offset/returned byte counts, exact text, and
`next_offset_bytes`. Intermediate replies advertise only one byte continuation.
A final reply may expose its existing collection-next action; it never exposes
that cursor before the current JSON representation is complete.

The complete MCP content envelope, including intro, escaped JSON, actions and
footer, is bounded by min(transport capacity, 8192 bytes). Prefix selection uses
actual encoded envelope length and preserves UTF-8. Terminal action overhead is
fitted separately; impossible metadata or a remaining character returns a size
error. No route contract or full schema is attached to compact actions or fragment
continuations; exact short help selectors provide the schema read route.

These byte bounds make no token-count or latency claim.
