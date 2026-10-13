//! Wording of the `model` advice point, and nothing else: tune the texts here, the
//! logic in `model.rs` does not change. Each list pairs a criterion name with its
//! description; every question also gets `ABSTAIN`. The texts name kinds of work only:
//! which model runs a kind is the policy of the agent that reads the advice, and no
//! text here names one.

/// One sentence that travels inside an advised field: it explains the field to the
/// agent and says what to do with the advice. No review is held to this point.
pub const NOTE: &str = "Jev's advice on the executors that the work of the input just recorded needs: practical is the kind of executor its practical work needs, specialist is whether it needs bounded intellectual work of its own and how hard that work is, none means there is no such work, and each *_p is Jev's probability for its choice; start the executor of the practical work of this input on the route your orchestration policy gives the named kind, routine or implementation, and hand separate intellectual work to the specialist at its standard effort for standard and at its raised effort for demanding; depart only where the advice is plainly wrong, and tell the user so.";

pub(super) const PRACTICAL_INSTRUCTIONS: &str = "Judge which executor the practical work that the text in state.new_input asks for needs. Practical work is doing; intellectual work of its own, such as research, a design or a review, is judged by another question and does not count here. Judge by the new input, by the user's earlier words in state.user_words and by the product level in state.mode; do not judge the length or the quality of the text. When two criteria fit about equally, choose the more capable one: implementation over routine, and either over none.";
pub(super) const PRACTICAL_CRITERIA: [(&str, &str); 3] = [
    ("none", "The input asks for no practical work."),
    ("routine", "Routine or tightly bounded practical work."),
    ("implementation", "Ordinary or complex implementation."),
];
pub(super) const PRACTICAL_ABSTAIN: &str =
    "The state does not say enough to judge the practical work.";

pub(super) const SPECIALIST_INSTRUCTIONS: &str = "Judge whether the text in state.new_input needs bounded intellectual work of its own, apart from the practical work: research, analysis, a comparison, a design, a plan or a substantive review; and when it does, how hard that work is. Judge by the new input, by the user's earlier words in state.user_words and by the product level in state.mode; do not judge the length or the quality of the text. When two criteria fit about equally, choose the more capable one: demanding over standard, and either over none.";
pub(super) const SPECIALIST_CRITERIA: [(&str, &str); 3] = [
    (
        "none",
        "The input needs no bounded intellectual work of its own.",
    ),
    (
        "standard",
        "It needs such work, of ordinary difficulty: a specialist's standard effort is enough.",
    ),
    (
        "demanding",
        "It needs such work, and the work is hard: it takes a specialist's raised effort.",
    ),
];
pub(super) const SPECIALIST_ABSTAIN: &str =
    "The state does not say enough to judge the intellectual work.";
