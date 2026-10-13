//! The user's words as an advice point reads them: by source, in recorded order, each
//! sequence up to its own bound.
use super::doubles::Journal;
use crate::advice::words::{program_level, scope_level, slice_level};
use serde_json::json;
use tect_domain::SizingWordsBasis;
use uuid::Uuid;

#[tokio::test]
async fn words_are_read_by_source_in_recorded_order_each_up_to_its_bound() {
    let (workspace, program, scopes, slices) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    let word = |source: &str, text: &str| json!({"source":source,"text":text});
    let mut journal = Journal::default();

    let words = scope_level(&mut journal, workspace, (program, 2), (scopes, 1))
        .await
        .unwrap();
    assert_eq!(
        json!(words),
        json!([
            word("program_input", "program 1"),
            word("program_input", "program 2"),
            word("scope_candidate_input", "scope set 1")
        ])
    );
    assert_eq!(
        journal.reads,
        [("program", program, 2), ("scope set", scopes, 1)]
    );

    // Behind a Slice candidate set stand the words its Scope was opened on, as the
    // basis had read them, and then its own.
    journal.reads.clear();
    let basis = SizingWordsBasis {
        program_id: program,
        program_latest_input: 1,
        planning_latest_input: 2,
    };
    let words = slice_level(&mut journal, workspace, (scopes, &basis), (slices, 2))
        .await
        .unwrap();
    assert_eq!(
        json!(words),
        json!([
            word("program_input", "program 1"),
            word("scope_candidate_input", "scope set 1"),
            word("scope_candidate_input", "scope set 2"),
            word("slice_candidate_input", "slice set 1"),
            word("slice_candidate_input", "slice set 2")
        ])
    );
    assert_eq!(
        journal.reads,
        [
            ("program", program, 1),
            ("scope set", scopes, 2),
            ("slice set", slices, 2)
        ]
    );

    // A bound before the first entry reads nothing of that sequence: the first input
    // a set records has no earlier words of the set.
    let words = slice_level(&mut journal, workspace, (scopes, &basis), (slices, 0))
        .await
        .unwrap();
    assert_eq!(words.len(), 3);
    assert_eq!(journal.reads.last(), Some(&("slice set", slices, 0)));
    assert!(journal.rows.is_empty() && !journal.locked);
}

#[tokio::test]
async fn the_words_behind_a_program_are_its_own_inputs_up_to_the_bound() {
    let (workspace, program) = (Uuid::new_v4(), Uuid::new_v4());
    let word = |text: &str| json!({"source":"program_input","text":text});
    let mut journal = Journal::default();

    // An input recorded third has the two before it as its earlier words.
    let words = program_level(&mut journal, workspace, (program, 2))
        .await
        .unwrap();
    assert_eq!(json!(words), json!([word("program 1"), word("program 2")]));
    assert_eq!(journal.reads, [("program", program, 2)]);

    // They are the words a Scope candidate set of the Program starts with.
    let scopes = Uuid::new_v4();
    let behind = scope_level(&mut journal, workspace, (program, 2), (scopes, 0))
        .await
        .unwrap();
    assert_eq!(behind, words);

    // The first input of a Program has none.
    let words = program_level(&mut journal, workspace, (program, 0))
        .await
        .unwrap();
    assert!(words.is_empty());
    assert_eq!(journal.reads.last(), Some(&("program", program, 0)));
    assert!(journal.rows.is_empty() && !journal.locked);
}
