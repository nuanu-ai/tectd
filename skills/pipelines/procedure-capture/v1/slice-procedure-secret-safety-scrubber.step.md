---
id: "slice-procedure-secret-safety-scrubber"
kind: "internal_step"
owner_family: "slice_variant"
pipeline_id: "slice.custom-procedure-capture"
step_id: "slice-procedure-secret-safety-scrubber"
entry_gate: false
public_trigger: false
native_skill_discovery: false
body_path: "capabilities/instructions/pipelines/slice-variants/procedure-capture/slice-procedure-secret-safety-scrubber.step.md"
source_manifest: "capabilities/pipelines/slice-variants/procedure-capture.pipeline.json"
legacy_skill_ref: "slice-procedure-secret-safety-scrubber"
architecture_refs:
  - "docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6"
  - "docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19"
---

# Procedure Secret Safety Scrubber

## Overview

This skill turns a procedure-capture candidate into a safe, parameterized proposal input. Core rule: a reusable procedure, runbook draft, command recipe, proof template, or skill candidate must never carry raw secrets, private credentials, unsafe local details, raw transcripts, or sensitive environment evidence forward.

## When to Use

Use this inside the `slice.custom-procedure-capture` variant when captured commands, command history, logs, artifacts, screenshots, normalized steps, proof examples, setup notes, source events, or proposal text may contain credentials, account identifiers, private URLs, customer or personal data, wallet material, raw session excerpts, local paths, deployment names, or other sensitive environment details.

Do not use it for normal operation execution, existing runbook execution, broad repository secret scanning, live incident response, package inspection, durable KB/runbook/skill mutation, or approval to preserve raw secret values. Route operation execution to the operational pipeline, broad security review to the security/domain pipeline, existing runbook use to the runbook/ops path, and skill creation to the later skill-candidate/authoring route after this scrub has finished.

## Source Contract

Grounding: `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json` step `slice-procedure-secret-safety-scrubber` is a validator pause point. It invokes this skill plus `superpowers:verification-before-completion`, produces `secret-safety.md`, gates `secret_safety_recorded`, fails to `stop_or_handoff`, and can advance only to `ready_for_next_step`.

Architecture anchors: `docs/architecture/master-plugin-target-architecture-part-6b-spine-operational-pipelines.html#procedure-and-research-slice-variants` defines the procedure-capture skillset and says this step removes secrets, raw tokens, private credentials, and unsafe environment-specific material. `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s6` defines procedure capture as a Slice variant with proposal/rejection boundaries. `docs/architecture/master-plugin-target-architecture-final-whole-plugin-map.html#s19` requires unexpected custom workflows to become governed proposals, not silent durable writes. `docs/architecture/master-plugin-target-architecture-memory-session-privacy.html` requires targeted raw-history access, redaction, custody, and no unredacted transcript commits. `docs/architecture/master-plugin-target-architecture-validation-harness-evals.html` requires safety regressions for no hidden memory, no authority inflation, and no automatic procedure-to-skill promotion.

Reference adaptation: apply the proof-before-claim rule from `skills/references/superpowers/verification-before-completion/SKILL.md`. Do not claim `safe_to_continue` or `secret_safety_recorded` until the source inputs, scrub checklist, leak sweep, and residual-risk line have been inspected.

## Operating Procedure

1. Confirm scope and prerequisites. The selected variant must be procedure capture, not execution or durable promotion. Required inputs are `source-event.md`, `captured-steps.md`, candidate or normalized procedure text, `authority-risk.md`, `proof-contract.md`, and any command history/log/artifact excerpts used as evidence. If the candidate, authority boundary, or proof boundary is absent, emit `stop_or_handoff`.
2. Freeze the write boundary. This step may write only `secret-safety.md`; scrubbed candidate fragments are sections inside that file, not extra unmanaged artifacts. It must not update a runbook, KB page, skill, registry, command recipe, environment file, source repo, workspace control plane, live system, or durable domain.
3. Inventory sensitive surfaces by location without copying raw values. Sweep captured shell commands, command flags, env assignments, `.env` snippets, config blocks, API requests, headers, cookies, database URLs, RPC URLs, SSH material, cloud keys, wallet/seed material, logs, screenshots, artifact names, local paths, account IDs, customer identifiers, private hostnames, IPs, and raw transcript/session excerpts.
4. Classify every finding as one of: `secret_value`, `credential_reference`, `private_endpoint`, `restricted_identity`, `customer_or_personal_data`, `wallet_or_key_material`, `unsafe_environment_detail`, `local_path_or_machine_detail`, `raw_session_material`, `proof_payload_too_sensitive`, `safe_placeholder`, or `safe_public_context`. Record class, source artifact, line/step reference, and procedural role; never record the raw value.
5. Apply redaction rules. Replace values with stable placeholders such as `<TOKEN:redacted>`, `<CREDENTIAL:redacted>`, `<AUTH_HEADER:redacted>`, `<PRIVATE_URL:redacted>`, `<ACCOUNT_ID:redacted>`, `<CUSTOMER_DATA:redacted>`, `<WALLET_OR_KEY:redacted>`, `<LOCAL_PATH:workspace-specific>`, `<ENVIRONMENT:target-specific>`, and `<TRANSCRIPT_EXCERPT:restricted>`. Preserve command shape only when the future operator can safely supply the parameter from their own authorized environment.
6. Rewrite proof and command examples. Keep the order of checks, expected evidence classes, stop conditions, and failure handling, but remove proof payloads. For example, say "verify an authenticated 200 from `<PRIVATE_URL:redacted>` using an authorized token" rather than preserving the actual URL, token, header, or response body.
7. Run required leak sweeps over `secret-safety.md` and the scrubbed candidate: token/API-key/JWT-looking strings, private key blocks, seed phrase/wallet language, auth headers, cookies, password flags, database/RPC URLs, internal hostnames or IPs, cloud account IDs, local absolute paths, emails/customer names when not public, raw transcript quotes, and screenshots or artifacts that visually contain credentials.
8. Apply unsafe promotion blockers. Block when the procedure cannot be understood without secret values, the user asks to preserve credentials, evidence requires restricted custody but no custody pointer exists, raw session material would need broad mining, a private endpoint or account ID remains identifying, or the candidate still contains prohibited evidence after redaction.
9. Write `secret-safety.md` with the output shape below. End with one manifest-aligned routing verdict. If safe, record `safe_to_continue`, set gate `secret_safety_recorded`, and advance with terminal state `ready_for_next_step` toward `slice-procedure-reuse-fit-evaluator`. If blocked by sensitive evidence, route through `stop_or_handoff` with reason `blocked_secret_risk`. If inputs, authority, or custody are unclear, route through `stop_or_handoff` with the exact missing authority, source, or custody need.

## Outputs

Write `secret-safety.md` only. Required sections:

- `Inputs inspected`: artifact names and source classes, not secret values.
- `Sensitive-surface inventory`: category, location, procedural role, and disposition.
- `Redaction map`: placeholder class to meaning, with no reversible mapping.
- `Scrubbed candidate fragments`: parameterized command/proof/checklist text.
- `Allowed evidence retained`: command shapes, proof classes, freshness notes, source links, and restricted-custody references.
- `Prohibited evidence removed`: secret classes removed or quarantined by reference.
- `Leak-sweep checklist`: command history, logs, artifacts, proof payloads, environment details, and raw session material checked.
- `Residual risks and handoff`: unresolved custody, authority, freshness, or review needs.
- `Verdict`: `safe_to_continue` with `ready_for_next_step` and whether `secret_safety_recorded` may be set, or `stop_or_handoff` with reason `blocked_secret_risk` or another exact blocker.

The scrubbed candidate is proposal input only. This skill does not promote a procedure, mutate durable knowledge, create or update a skill, approve execution, or make runbook/KB/source truth current.

## Verification

Before any safe claim, perform proof-before-claim verification: read the original input list, read `secret-safety.md`, compare the scrubbed candidate against the inventory, and confirm the leak-sweep checklist has no unresolved prohibited evidence. Verify that the output references `capabilities/pipelines/slice-variants/procedure-capture.pipeline.json`, records `secret_safety_recorded` only when supported, preserves proof requirements without proof payloads, and names residual risks plainly.

For maintaining this skill source, run:

```bash
node tools/validate-internal-skill-body-quality.mjs --skill slice-procedure-secret-safety-scrubber
node tools/validate-internal-skill-trigger-fixtures.mjs --skill slice-procedure-secret-safety-scrubber
```

These validators prove the skill source and trigger fixtures. They do not prove that any real captured procedure is safe; each real candidate still needs the scrub and leak sweep.

## Failure Modes

Use `blocked_secret_risk` when raw secrets are required to understand the procedure, the candidate still leaks prohibited evidence, sensitive proof cannot be summarized safely, a user asks to preserve secret values, or no restricted-custody pointer exists for evidence that must remain private.

Use `stop_or_handoff` when required procedure-capture inputs are missing, authority/proof boundaries are unclear, the work is actually operation execution, the ask is broad secret scanning or security incident handling, or the next safe owner is a security, ops, runbook, or user-approval route.

Forbidden actions: do not run target commands, fetch secrets, decrypt or print credentials, widen raw-history access, copy unredacted logs/transcripts, invent safe placeholders without preserving procedural role, promote to durable KB/runbook/skill, update registries, or downgrade the scrub to keep the pipeline moving.
