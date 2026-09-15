---
name: engineering-review
description: Review a code-writing Slice specification, implementation plan, or resulting implementation against its pinned engineering standards before advancing the native phase.
---

# Engineering review

Read the exact engineering standards resource and report schema delivered with
the current phase. Use the phase's pinned versions, current native outputs and
project constraints. Do not substitute a remembered standard or another run's
review. Required standard IDs are ENG-01 through ENG-10.

## Review the decision before its execution

For specification review, inspect behavior ownership, component responsibilities,
dependency direction, ports, invariants and error semantics. Check the proposed
solution against existing source and project constraints. Unknown material
decisions are gaps to resolve, not assumptions for the implementation planner.

For plan review, follow each material requirement into its intended owner,
specific files, implementation sequence and verification. Check proposed
boundaries, existing behavior to reuse or change, file-size estimates and
unnecessary complexity. The plan must implement the accepted specification
without inventing new material decisions. A Lightweight contract and concise
file/change/test plan are sufficient; do not manufacture a Full Design package.

For implementation conformance, inspect the actual diff and relevant surrounding
code against the reviewed plan, then verify actual formatted file counts and
content digests. Preserve specification-compliance and code-quality checks.
Do not postpone the first architectural assessment until this stage.

## Produce evidence for every rule

Record one assessment for each delivered rule, with a concrete rationale and
references to the reviewed source, specification or plan. Mark non-applicability
only when the actual work makes the rule inapplicable, with the reason. A small
Slice is not a blanket exemption. State findings with their rule ID, location,
observable consequence and required correction. Do not count a repeated claim,
resource-read receipt or compliant=true as evidence of architectural correctness.

Record the exact consumed native output revisions/digests and rules digest in
the typed report. For file-size evidence, distinguish an estimate from an
observed count. Classify actual contents as behavioral, mixed or declarative;
a filename is not classification evidence. Files over 500 lines need a cohesion
justification; over 1000 requires declarative content; over 1500 cannot pass.

## Findings and rework

Review is read-only with respect to the reviewed specification, plan and product
code. Return findings through the native rework route. The owning producer
repairs the affected artifact, then the appropriate review runs again on the
new inputs. Existing native rework invalidates dependent review and execution
outputs; never reuse a verdict for an older plan or source basis.

Before or during implementation, if the required change alters behavior
ownership, architectural boundaries or a material contract, return to the
appropriate specification/plan phase before continuing that part of the work.
An ordinary implementation detail within the approved contract does not require
replanning. Inspect post-review changes for conformance during normal code
review and verification.

Pass requires complete rule coverage and no unresolved blocking violations.
Missing review evidence blocks advancement. Preserve honest findings on rework
or blocked routes; never fabricate pass-shaped evidence to report failure.

The phase contract determines whether a fresh independent reviewer is required.
Do not invent a new identity or call a second pass by the same author independent.
Follow the active root/executor policy; this skill does not mandate subagents,
additional human approvals or a separate audit harness. The backend can validate
the reported contract and bindings, not independently prove the truth of all
architectural judgments or external filesystem observations.
