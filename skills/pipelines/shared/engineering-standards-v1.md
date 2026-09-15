# Engineering standards

Version: 1.0.0. Owner-approved on 2026-09-15.

These requirements govern specifications, implementation plans and authorized
code changes. They apply to product source, tests, executable scripts and IaC.
The phase contract still controls what may be changed. A rule injection or a
review never grants implementation, deployment or operational authority.
Project-specific requirements delivered from Durable Knowledge supplement this
baseline. Existing explicit owner decisions remain authoritative; do not ask for
permissions already granted. Do not silently waive a requirement.

## ENG-01 — One accountable owner for behavior

Every business rule, invariant and state transition has an explicit accountable
owner within its semantic domain. Before adding behavior, locate the existing
owner and determine whether the change belongs there. Do not introduce
independent implementations of the same policy in different components.
Boundary validation and database constraints may protect the same invariant,
but must preserve its contract and have an explicit protective purpose. They
must not become divergent business-policy implementations. A read model or
cache is not a second authority for writes. Different domain meanings do not
justify forcing unrelated policies into a global shared component.

## ENG-02 — Cohesive responsibilities and justified boundaries

Every new component, module or layer needs a concrete responsibility or a real
boundary, such as IO, security, transaction ownership or isolation. Keep related
behavior understandable and local. Do not scatter one responsibility across
components, add chains of empty forwarding wrappers, or put unowned business
policy into generic helpers, utils or common modules. Splitting a file solely
to satisfy a line limit does not establish a valid responsibility boundary.

## ENG-03 — Inward dependencies

Business/domain policy must not depend on transport, UI frameworks, ORM types,
database drivers or external SDKs. Application code coordinates use cases;
adapters perform external interactions; the outer composition boundary selects
concrete implementations. Translate infrastructure data and errors at the
boundary instead of allowing them to become domain contracts. Preserve these
dependency rules without imposing an arbitrary folder count or service layout.

## ENG-04 — Purposeful ports and adapters

A port expresses the consuming application's actual needs. Its adapter
translates the external operation, data and errors into that contract. Adapters
must not introduce hidden business policy or bypass the use-case owner.
Replacing an implementation while preserving the port contract must not require
rewriting domain policy. Create interfaces at meaningful boundaries; an
interface for every class or local call is not required.

## ENG-05 — Behavioral SOLID

- SRP: keep one cohesive responsibility; separate independent reasons to change.
- OCP: support required variations through appropriate extension points. This
  does not prohibit correcting existing code or require speculative plugins.
- LSP: replacement preserves preconditions, results, errors, invariants and
  side-effect guarantees, not merely a method signature.
- ISP: consumers depend only on the capabilities they require; avoid interfaces
  enlarged for unrelated consumers.
- DIP: stable policy does not depend on technical details; abstractions belong
  at the responsibility boundary that needs them.

Classes, interfaces and directory names are not evidence of compliance.

## ENG-06 — Necessary complexity only

Choose the smallest solution that fully meets the current requirements and
preserves architectural boundaries. Justify a new abstraction, dependency,
service, extension mechanism or store by a concrete requirement or established
constraint. Hypothetical future reuse is insufficient. Fix behavior at its
proper existing owner instead of creating a competing execution path.

## ENG-07 — Explicit missing-input and failure semantics

The contract specifies required inputs, accepted values, errors, defaults and
fallback behavior. Do not guess missing required data, convert failure into an
apparently successful empty result, or invent a fallback that changes the
meaning of an operation. Defaults already established by the contract do not
need renewed approval. Retries, partial success and recovery must preserve the
operation's declared guarantees.

## ENG-08 — Evidence at the production boundary

Map material requirements and invariants to concrete verification. Tests must
exercise the relevant production boundary rather than prove a second copy of
the algorithm inside test code. Do not weaken a contract or its checks to make
tests pass. Product tests, fixtures and necessary ordinary test infrastructure
are allowed within scope. Creating an unsolicited standalone audit/test harness,
then diverting the task into developing or debugging it, is strictly prohibited.
Use focused checks and existing infrastructure; expand only for demonstrated
gaps that matter to the requested behavior.

## ENG-09 — No concealed architectural debt

New or worsened violations block acceptance of the affected specification,
plan or implementation. "Temporary", "refactor later" and "already like this"
are not justifications. Unrelated pre-existing debt does not authorize a
whole-project rewrite. Do not enlarge that debt or introduce a new dependency
on it without an explicit applicable owner decision. Record the exact boundary
and decision; do not label a known violation as compliant.

## ENG-10 — Content-sensitive file size

Count all physical lines of each source file after its ordinary formatting,
including comments and blank lines. An empty file has zero lines; a terminal
newline does not create an additional empty line. Count source, tests,
executable scripts and IaC consistently.

- Up to 500 lines is the target for every file and the normal range for
  behavioral/business code.
- 501–1000 lines requires one cohesive responsibility and an explicit
  justification. Behavioral code must explain why a split would harm cohesion.
- 1001–1500 lines is allowed only for genuinely declarative definitions such
  as fields, types, schemas and enumerations without substantial execution
  logic. The responsibility and reason to retain that unit remain explicit.
- More than 1500 lines is prohibited. There is no exception above this ceiling.

Classify by content, not a filename such as types or schema. Mixed files are
behavioral for the upper limit. Do not evade the rule through minification,
encoding code in strings, a generated label, or arbitrary file fragments.
Planning uses honest size estimates; implementation verifies actual formatted
files and their content digests. Review the affected files and dependency area;
do not turn unrelated repository debt into unsolicited work.

## Delivery and evidence boundary

The backend pins this exact resource and requires its read receipt on the
applicable phases. Whole delivery still checkpoints each phase: receiving a
future code phase is not permission to execute it before the current review
has passed. The backend validates report shape, rule coverage, exact native
input bindings and legal transitions. Architectural truth and externally
reported source observations remain the reviewing agent's responsibility;
a boolean assertion alone is not substantive review evidence.

Origin: the six package-owned Tect V1 engineering standards and Tony's approved
2026-09-15 replacement. Conceptual references: Robert C. Martin, The Clean
Architecture (2012); Alistair Cockburn, Hexagonal Architecture / Ports and
Adapters. This resource contains the normative product policy; these references
do not import additional workflow requirements.
