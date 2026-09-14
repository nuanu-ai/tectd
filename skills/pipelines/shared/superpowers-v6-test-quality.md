# Apply TDD and writing-good-tests to this native phase

Read both pinned `test-driven-development` and `test-driven-development/writing-good-tests` resources. The latter replaces the old anti-patterns resource; an old read receipt does not cover the new ID or body.

Before writing a test, name the observable production break it catches. Derive expected results independently from the implementation and its helpers. Exercise the real boundary, including meaningful negative behavior; a stub that always accepts inputs is not evidence of the real contract. Use a controlled double only at the required external or slow boundary. A test that merely greps an instruction or repeats a constant does not establish behavioral quality.

In authorized Lightweight/Full implementation, preserve RED → minimal GREEN → REFACTOR evidence, including the expected failure cause and actual final result. Preserve pre-existing and unrelated work. The upstream delete/start-over wording grants no authority to discard user files, another worker's changes or already accepted implementation. Report the actual test sequence rather than manufacturing a RED claim.

In Debug's future-proof phase, apply these principles to the proposed failing case, assertions and acceptance contract only. Do not write or run test source, apply a cause-level fix or build a test harness in a diagnosis-only phase. Its result is a precise future verification plan, not executed RED/GREEN.

Human-facing prose and trivial forwarding do not earn ceremonial tests. Existing validators still verify transport, schema and exact read identity; do not label those checks as proof of an agent's reasoning or instruction-following behavior.
