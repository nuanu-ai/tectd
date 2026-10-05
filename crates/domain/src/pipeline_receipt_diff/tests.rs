use super::*;

#[test]
fn complete_unicode_receipt_diff_is_unambiguous_sorted_and_retains_multiplicity() {
    let long = "id@界🙂".repeat(100);
    let expected = [
        (long.as_str(), "v@1", "d界"),
        ("a", "1", "d1"),
        ("a", "1", "d1"),
    ];
    let submitted = [
        ("z@id", "v@界", "d🙂"),
        ("z@id", "v@界", "d🙂"),
        ("z@id", "v@界", "d🙂"),
        ("a", "1", "d1"),
    ];
    let diff = FullReceiptDiff::between(&expected, &submitted);
    assert_eq!(
        (
            diff.expected_unique_count,
            diff.submitted_count,
            diff.submitted_unique_count
        ),
        (2, 4, 2)
    );
    assert_eq!(diff.missing[0].instruction_id, long);
    assert_eq!(diff.unexpected[0].instruction_id, "z@id");
    assert_eq!(diff.duplicates[0].repeat_count, 3);
    let (expected_json, actual_json) = diff.diagnostic_sections().unwrap();
    assert!(expected_json.len() > 240);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&expected_json).unwrap()["missing"][0]["instruction_id"],
        long
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual_json).unwrap()["duplicates"][0]["repeat_count"],
        3
    );
    let mut reversed = submitted;
    reversed.reverse();
    assert_eq!(diff, FullReceiptDiff::between(&expected, &reversed));
    assert_eq!(
        serde_json::to_string(&diff).unwrap(),
        serde_json::to_string(&FullReceiptDiff::between(&expected, &reversed)).unwrap()
    );
}

#[test]
fn complete_diff_matches_original_set_and_duplicate_acceptance_for_all_subsets() {
    let expected = [("a", "1", "d"), ("b", "1", "d")];
    let universe = [("a", "1", "d"), ("b", "1", "d"), ("extra", "1", "d")];
    let expected_set = expected.into_iter().collect::<BTreeSet<_>>();
    for len in 0..=4usize {
        for mut index in 0..3usize.pow(len as u32) {
            let mut submitted = Vec::new();
            for _ in 0..len {
                submitted.push(universe[index % 3]);
                index /= 3;
            }
            let actual_set = submitted.iter().copied().collect::<BTreeSet<_>>();
            let old_reject = actual_set != expected_set || actual_set.len() != submitted.len();
            let diff = FullReceiptDiff::between(&expected, &submitted);
            assert_eq!(
                old_reject,
                !diff.missing.is_empty()
                    || !diff.unexpected.is_empty()
                    || !diff.duplicates.is_empty()
            );
        }
    }
}
