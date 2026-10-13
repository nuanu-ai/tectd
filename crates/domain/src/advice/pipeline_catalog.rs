//! Wording of the `pipeline` advice point, and nothing else: tune the texts here, the
//! logic in `pipeline.rs` does not change. In the instructions `{key}` stands for a
//! candidate key such as `c2`. The criteria are not written here: each is one pipeline
//! of the catalogue the draft was saved against, under its own name, described with
//! the three texts of its entry under the labels below; the question also gets `ABSTAIN`.

/// One sentence that travels inside an advised field: it explains the field to the
/// agent and says what to do with the advice, which binds.
pub const NOTE: &str = "Jev's second opinion on the pipeline of each work node this save created or changed: positions are 1-based in draft order, pipeline is Jev's choice from the same catalogue and pipeline_p its probability, draft_pipeline_p is Jev's probability for the pipeline your draft chose and differs says Jev chose another one; where it differs follow the advice by changing the draft and saving it again, or where the advice is plainly wrong give the reason in advice_departures of the review; a review that says ready is refused until then.";

pub(super) const INSTRUCTIONS: &str = "Choose the pipeline that fits the work of candidate {key} in state.candidates, judged against what the user asked for in state.user_words and against the candidate's product level: its own mode_override when it carries one, else state.mode. Judge the work that its title, outcome and includes describe. Each criterion is one pipeline of the catalogue: when to choose it, when not to choose it and the result it gives. Read the catalogue's words as it means them: durable knowledge and known material are entries of TectD's own knowledge base, never files of the product repository, and an operation is a consequential action on a running system or environment, such as a deployment, a migration or a change of live data. Work whose result is files of the repository, its documents and plans included, is development work. This is a second opinion for the planning agent.";

/// The labels a criterion's description puts before the `choose_when`, the
/// `do_not_choose_when` and the `expected_result` of its catalogue entry, in this order.
pub(super) const CHOOSE_WHEN: &str = "Choose when:";
pub(super) const DO_NOT_CHOOSE_WHEN: &str = "Do not choose when:";
pub(super) const EXPECTED_RESULT: &str = "Expected result:";

pub(super) const ABSTAIN: &str = "The state does not say enough to choose a pipeline.";
