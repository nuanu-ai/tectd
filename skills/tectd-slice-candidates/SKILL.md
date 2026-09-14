---
name: tectd-slice-candidates
description: Design and critically review the complete revisable Slice-candidate plan for one opened Scope through the backend-supplied contract.
---

# TectD Slice candidates

Design one complete initial set of Slice candidates for an opened Scope. This method plans bounded outcomes and unresolved decision points. It does not execute a pipeline, open a Slice, report a Slice Result, or authorize an external effect.

## Use the backend-owned contract

Use the exact routes, schemas, identifiers, revisions, context versions, and next actions supplied by TectD. The backend owns persistence, stable identity, revision history, structural graph validation, context freshness, and authorization checks. The agent owns semantic coverage, candidate boundaries, pipeline classification, dependencies, review, and later plan revision. Catalogue entries describe executable versioned pipelines, but candidate planning does not start a run.

Read the complete method, all four matched rules, the complete pipeline catalogue, and every required context page before drafting or reviewing. Preserve supplied durable identifiers. Use temporary local labels only within one draft payload and copy backend-assigned identifiers from its response.

Read the backend-captured Slice-candidate knowledge manifest and its declared needs. Apply the reviewed briefs relevant to vertical outcomes, interfaces/dependencies, proof requirements and pipeline choice, preserving exact conditions, exceptions and sources. This is more concrete than Program strategy but still precedes implementation. Do not load every dependency version, command or file-level detail from the underlying knowledge merely because it is available.

Provide only known target/environment/action context through the advertised typed fields; unknowns remain explicit. Required unresolved needs must be resolved or represented as a real planning gap. Do not invent bindings, subjects or a confidence score to bypass them. Empty DK permits a normal first plan.

Use the exact consumed-manifest guard in draft/review/open actions that require it. Current queries may report staleness but do not refresh a saved snapshot. Refresh through the supplied command when the basis changes, then reconsider the affected future candidates while preserving opened/running/completed work. Knowledge is an input to your judgment; it does not automatically classify the candidate, approve a dependency or change the user's Scope.

## Initial complete pass

Cover the opened Scope in one complete planning pass. Create concrete work candidates for bounded outcomes and explicit decision points for choices that available evidence cannot yet resolve. A decision point records its question, resolution criteria, and the evidence or predecessor result needed to choose future work; it is planned uncertainty, not necessarily a review blocker. It is not an openable Slice and does not require speculative alternative branches to be created in advance.

Each work candidate represents one bounded outcome and selects exactly one canonical pipeline type. Dependencies must express actual prerequisites between candidates. Do not concatenate pipelines into a rigid hybrid or use dependencies to imitate a stage machine. The complete initial set may later change as real Slice Results reveal facts.

## Pipeline classification

Choose a pipeline from evidence about the candidate outcome. Use `slice.lightweight-tdd-development` as the default path for bounded development, fixes, bugs, and small vertical features. A new-feature label, cross-component work, or size alone does not justify Full.

Use `slice.full-design-to-execution` only for an intrinsically complex or large indivisible vertical outcome. Record why Lightweight is insufficient and why further sensible vertical decomposition cannot produce independently deliverable outcomes. Full is not a generic fallback. Consider Debug when the cause of a behavioral deviation is unknown, and Custom Procedure Capture when the explicitly requested outcome is a reusable procedure; do not hide either behind Full.

Apply each catalogue entry's `choose_when`, `do_not_choose_when`, and `expected_result`. Selecting a pipeline for a candidate is planning evidence; it does not prove that a run has begun or completed.

## Review and continuation

Review the complete set against the opened Scope, captured input, existing Slice candidates and Slices, Slice Results, dependencies, and all four matched design rules. Confirm complete Scope coverage, bounded outcomes, honest decision points, valid prerequisites, evidence-based pipeline choices, and the required Full rationale. A ready plan is a reviewed plan and may intentionally retain unresolved decision points; it does not claim every future work candidate is executable. Work such as an initial Debug candidate may open when its own dependencies are satisfied while a downstream decision remains unresolved. Save the review through the supplied action. The same four rules apply during design, review, and refresh.

After a Slice Result, refresh and review the affected future graph even when the selected branch leaves its candidates unchanged. Revise, reorder, add, or supersede future candidates as evidence requires, recording the predecessor evidence and rationale. Preserve stable identities and history; do not silently rewrite opened, running, or completed work. An externally reported result is an observation and evidence record, not proof that TectD executed a pipeline.

Opening an already designed work candidate creates one Slice with that candidate's one chosen pipeline and bounded outcome. Do not inject this design method or the four candidate-design rules into Slice opening or Slice context. Use only the backend-supplied managed-run action after opening; do not treat Slice opening itself as pipeline execution.
