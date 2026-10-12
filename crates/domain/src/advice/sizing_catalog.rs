//! Wording of the `sizing` advice point, and nothing else: tune the texts here, the
//! logic in `sizing.rs` does not change. In instructions `{key}` stands for a candidate
//! key such as `c2` and `{item}` for an included item key such as `c2.i3`. Each list
//! pairs a criterion name with its description; every question also gets `ABSTAIN`.

/// One sentence that travels inside an advised field and explains it to the agent.
pub const NOTE: &str = "Jev's advice on size, splitting and extra items for this draft: positions are 1-based in draft order, each *_p is Jev's probability for its choice, and you decide what to do with it.";

pub(super) const SIZE_INSTRUCTIONS: &str = "Judge how large the work of candidate {key} in state.candidates is, relative to what the user asked for in state.user_words and to the product level in state.mode. Judge the work that its title, outcome and includes describe, not the quality of the writing.";
pub(super) const SIZE_CRITERIA: [(&str, &str); 4] = [
    ("small", "One narrow change, a short sitting of work."),
    (
        "bounded",
        "One coherent piece of work with a clear end that can be finished and proven in one go.",
    ),
    (
        "large",
        "Several distinct pieces of work in one candidate; finishing it in one go is doubtful.",
    ),
    (
        "oversized",
        "Too much for one candidate: it mixes independent outcomes or needs many separate passes.",
    ),
];
pub(super) const SIZE_ABSTAIN: &str = "The state does not say enough to judge the size.";

pub(super) const ADVICE_INSTRUCTIONS: &str = "Recommend what to do with candidate {key} in state.candidates. This is advice to the planning agent, which decides.";
pub(super) const ADVICE_CRITERIA: [(&str, &str); 4] = [
    ("keep", "Keep the candidate as it is."),
    (
        "split",
        "Split it into several candidates of the same level: it holds separable outcomes.",
    ),
    (
        "simplify",
        "Keep one candidate but cut it down: it does more than the user asked for or than the product level in state.mode needs.",
    ),
    (
        "clarify",
        "Ask the user: the request does not settle what this candidate must deliver.",
    ),
];
/// Offered at level `scope_to_slices` only.
pub(super) const ADVICE_SEPARATE_SCOPE: (&str, &str) = (
    "separate_scope",
    "Move it into a Scope of its own: it is a different body of work than the Scope in state.scope.",
);
pub(super) const ADVICE_ABSTAIN: &str = "The state does not say enough to recommend.";

pub(super) const ITEM_INSTRUCTIONS: &str = "Classify included item {item} of candidate {key} in state.candidates against state.user_words, state.goals and the product level in state.mode.";
pub(super) const ITEM_REQUESTED: (&str, &str) = (
    "requested",
    "The user asked for this, directly or as an evident part of what they asked.",
);
pub(super) const ITEM_ENABLER: (&str, &str) = (
    "enabler",
    "The user did not ask for it, but the requested work cannot be done without it at the product level in state.mode.",
);
/// The items Jev puts here are the ones the reply lists under `extra_items`.
pub(super) const ITEM_EXTRA: (&str, &str) = (
    "extra",
    "Neither requested nor needed for the requested work at the product level in state.mode.",
);
pub(super) const ITEM_ABSTAIN: &str = "The state does not say enough to classify.";
