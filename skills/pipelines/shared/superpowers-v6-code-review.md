# Apply the updated review pair to the native review contract

Read `requesting-code-review` together with its pinned `code-reviewer` template. Use the exact authorized base/head and full relevant requirements; do not assume `HEAD~1` is the change boundary. A reviewer packet includes the product contract, scoped diff/evidence and specific verification limits, rather than the coordinator's full conversation.

Review is read-only on the target working tree, index and HEAD. Reviewers do not spawn other reviewers or repair the reviewed work. Inspect the actual code and evidence. Classify findings by concrete consequence, cite exact locations and explain uncertainty where the available material cannot establish behavior. Rerun a check only when a changed input, failure or unresolved evidence question justifies it.

Full's current phase-12 contract still requires its existing spec-compliance and code-quality obligations and recorded review evidence. This pair update does not migrate SDD to a single dual-verdict reviewer, change the two-stage sequence or introduce a `.superpowers/sdd` ledger. The pinned historical SDD suite remains explicit until a separately designed protocol migration.

Apply the current model/delegation policy. If the policy requires primary-agent intellectual review, preserve that ownership; do not claim independent seats that did not exist. If actual independence is required by the current contract and unavailable, record the gap through the native state rather than fabricate approval. A positive reviewer verdict concerns the reviewed source and evidence; it is not permission to merge, deploy or discard a workspace.
