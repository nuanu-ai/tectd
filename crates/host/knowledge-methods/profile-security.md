# Security knowledge: boundaries, controls and proven status

Apply General and this method to security assertions, requirements, threats, controls and findings. Distinguish a proposed control, documented configuration, verified behavior, finding status and remediation proof. A closed ticket, clean schema or policy document does not prove remediation or control effectiveness.

Evidence: identify assets, trust boundaries, threat/requirement, control mapping, source authority and the exact verification scope/time. Preserve sensitivity, applicable access, exceptions, residual risk and owner. Restricted or sensitive material uses OwnersOnly delivery or remains unavailable; do not copy secrets, credentials or exploit-ready restricted values into ordinary knowledge sources. A reference to controlled evidence can describe its identity and limits without reproducing its payload.

Create: map each scoped threat/requirement to controls, evidence, verification status, exceptions and residual risk. Distinguish supply-chain, configuration and runtime evidence when material. Record unsupported or unverified claims honestly. A finding's status and proof of mitigation/remediation are separate fields of meaning. Do not run a scan, probe, exploit or operational change merely to complete this publication method.

Revise: identify changed boundary, threat, control, authority, evidence, sensitivity, exception or status. Re-evaluate affected mappings and invalidate proof that no longer covers the changed system. Never lower access scope or turn residual risk into acceptance by omission. A changed finding status needs its stated evidence and authority, not only a label.

Revalidate: obtain new suitable evidence for the unchanged security assertion in its exact asset/boundary/configuration context. Preserve verification limitations and residual risk. An old scan with a new date, unchanged document or closed issue cannot establish current effectiveness. Changed control meaning or applicability requires revise. Missing authorization for verification remains an explicit dependency.

Supersede: compare successor asset/boundary scope, controls, coverage, authority, exceptions, sensitivity and verification. Enumerate replaced bindings. Prevent a weaker or narrower successor from silently replacing a stronger requirement. Explicitly preserve uncovered threats and transition risks; publication is not acceptance of those risks.

Retract: withdraw the unsupported or unsafe security claim/control guidance and identify affected consumers and required gaps. Preserve the reason and scoped historical evidence under its access policy. Retraction does not disable a deployed control, remediate a vulnerability or close a finding.

Erase: apply the owned-copy inventory to restricted source fragments, findings, control evidence, outputs and derived summaries, including exact replay and history. Do not reproduce the erased payload in a tombstone, digest-derived identifier or proof report. Preserve required independent evidence only under explicit scope/retention and report any unresolved retention conflict. Erasure and credential rotation/remediation are separate operations with separate authority.

Impact and terminal checks: reconcile constraints delivered to engineering/operations with the security requirement they implement, without automatically exposing restricted upstream evidence. Record broken derivations and access gaps rather than silently dropping required controls. Report canonical acceptance, proven security behavior, residual risk and erasure separately. The lifecycle does not confer operational permission or compliance certification.

Source: design §7.7, §7.12, §8, §11, §14; Tect V1 security-knowledge family at ea7259c9a30ff7ea9a9eef1eae116cfc525eedd8, including control mapping, proof gaps, sensitivity and source-class boundaries.
