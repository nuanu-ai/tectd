//! Wording of the `intake` advice point, and nothing else: tune the texts here, the
//! logic in `intake.rs` does not change. In `NEAREST_CANDIDATE`, `{key}` stands for a
//! candidate key such as `c2`. Each criterion pairs a name with its description; every
//! question also gets `ABSTAIN`.

/// One sentence that travels inside an advised field and explains it to the agent.
pub const NOTE: &str = "Jev's advice on the input just recorded: work is how much new work it asks for, placement is where that work lands, nearest is the 1-based position in the saved draft of the closest candidate, fit is how it sits with the earlier words and the product level; each *_p is Jev's probability for its choice, and you decide what to do with it.";

pub(super) const WORK_INSTRUCTIONS: &str = "Judge how much new work the text in state.new_input asks for, beyond what state.user_words and state.program already ask for. Measure it as if that work were one candidate, at the product level in state.mode; do not judge the length or the quality of the text.";
pub(super) const WORK_NONE: (&str, &str) = (
    "none",
    "It asks for no new work: it confirms, explains, answers or repeats what is already asked for.",
);
/// The four sizes after `none` mean what they mean in `sizing`: one text, two points.
pub(super) use super::sizing_catalog::SIZE_CRITERIA as WORK_SIZES;
pub(super) const WORK_ABSTAIN: &str = "The state does not say enough to judge the work.";

pub(super) const PLACEMENT_INSTRUCTIONS: &str = "Say where the work that state.new_input asks for belongs. state.program states the Program and its boundaries; state.candidates, when it is present, holds the candidates of the latest saved draft.";
/// Offered only when the saved draft has a candidate that can be named.
pub(super) const PLACEMENT_IN_DRAFT: [(&str, &str); 2] = [
    (
        "covered",
        "A candidate in state.candidates already delivers it; nothing has to be added.",
    ),
    (
        "extends",
        "It adds to what one candidate in state.candidates delivers and belongs inside that candidate.",
    ),
];
pub(super) const PLACEMENT_NEW_CANDIDATE: (&str, &str) = (
    "new_candidate",
    "It needs a candidate of its own at this level.",
);
/// Offered at level `scope_to_slices` only.
pub(super) const PLACEMENT_SEPARATE_SCOPE: (&str, &str) = (
    "separate_scope",
    "It is a different body of work than the Scope in state.scope and needs a Scope of its own.",
);
pub(super) const PLACEMENT_OUTSIDE_PROGRAM: (&str, &str) = (
    "outside_program",
    "It lies beyond the boundaries of the Program in state.program.",
);
pub(super) const PLACEMENT_ABSTAIN: &str = "The state does not say enough to place the work.";

pub(super) const NEAREST_INSTRUCTIONS: &str = "Name the candidate in state.candidates whose work is closest to what state.new_input asks for. A node of kind decision is there for context and is not offered.";
/// One criterion per candidate that can be named; its name is the candidate key.
pub(super) const NEAREST_CANDIDATE: &str = "Candidate {key} in state.candidates is the closest.";
pub(super) const NEAREST_NONE: (&str, &str) = (
    "none",
    "No candidate in state.candidates is close to what the input asks for.",
);
pub(super) const NEAREST_ABSTAIN: &str = "The state does not say enough to name one.";

pub(super) const FIT_INSTRUCTIONS: &str = "Judge how the text in state.new_input sits with the user's earlier words in state.user_words, with the Program in state.program and with the product level in state.mode.";
pub(super) const FIT_CRITERIA: [(&str, &str); 4] = [
    (
        "in_line",
        "It agrees with the earlier words and the Program, and asks for no more than the product level in state.mode needs.",
    ),
    (
        "beyond_mode",
        "It asks for more than the product level in state.mode needs.",
    ),
    (
        "contradicts",
        "It argues with the earlier words in state.user_words or with the constraints in state.program.",
    ),
    (
        "unclear",
        "What it asks for cannot be told from the text; a question to the user is needed.",
    ),
];
pub(super) const FIT_ABSTAIN: &str = "The state does not say enough to judge the fit.";
