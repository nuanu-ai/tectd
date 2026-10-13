//! A ready review against the advice open on its draft: when it is refused, what
//! the refusal names, and what a passing review records.
use super::*;

fn open(answer: u128, candidate: u128, revision: i64, point: BindingPoint) -> OpenAdvice {
    let answer_body = match point {
        BindingPoint::Sizing => json!({"candidate":1,"size":"large","size_p":0.6,
            "advice":"split","advice_p":0.8,"extra_items":[2,5],"extra_p":[0.7,0.4]}),
        BindingPoint::Pipeline => json!({"candidate":1,"pipeline":"slice.research",
            "pipeline_p":0.8,"draft_pipeline_p":0.1,"differs":true}),
    };
    OpenAdvice {
        id: Uuid::from_u128(answer),
        candidate_id: Uuid::from_u128(candidate),
        candidate_revision: revision,
        point,
        answer: answer_body,
    }
}

fn draft(candidates: &[(u128, i64)]) -> Vec<(Uuid, i64)> {
    candidates
        .iter()
        .map(|(id, revision)| (Uuid::from_u128(*id), *revision))
        .collect()
}

fn refusal(error: Error) -> Refusal {
    assert_eq!(error.code(), "ADVICE_NOT_FOLLOWED");
    error.refusal().unwrap()
}

#[test]
fn advice_is_open_while_its_candidate_stands_in_the_draft_under_the_same_revision() {
    let level = SizingLevel::ProgramToScopes;
    let advice = [open(10, 1, 1, BindingPoint::Sizing)];
    let ready = |candidates: &[(u128, i64)]| {
        level.review_departures(true, &draft(candidates), &advice, &[])
    };
    // The candidate stands as it was advised on: the review is refused.
    assert!(ready(&[(1, 1), (2, 1)]).is_err());
    // It was changed and saved: the answer about the earlier revision is not open.
    assert_eq!(ready(&[(1, 2), (2, 1)]), Ok(vec![]));
    // It left the draft: nothing is open.
    assert_eq!(ready(&[(2, 1)]), Ok(vec![]));
    assert_eq!(ready(&[]), Ok(vec![]));
    // No advice at all: any review passes and records nothing.
    assert_eq!(
        level.review_departures(true, &draft(&[(1, 1)]), &[], &[]),
        Ok(vec![])
    );
}

#[test]
fn only_a_ready_review_is_refused_and_a_departure_closes_one_point_of_one_candidate() {
    let level = SizingLevel::ScopeToSlices;
    let advice = [
        open(10, 1, 1, BindingPoint::Sizing),
        open(11, 1, 1, BindingPoint::Pipeline),
        open(12, 2, 3, BindingPoint::Sizing),
    ];
    let standing = draft(&[(1, 1), (2, 3), (3, 1)]);
    // A review that does not say ready passes with everything open.
    assert_eq!(
        level.review_departures(false, &standing, &advice, &[]),
        Ok(vec![])
    );

    let stated = [
        departure(1, BindingPoint::Sizing, "It is one outcome"),
        departure(1, BindingPoint::Pipeline, "The cause is known"),
        departure(2, BindingPoint::Sizing, "It cannot be cut"),
        // About a candidate without open advice: no error, nothing recorded.
        departure(3, BindingPoint::Sizing, "Nothing was advised"),
        departure(9, BindingPoint::Pipeline, "Not in the draft"),
    ];
    let recorded = level
        .review_departures(true, &standing, &advice, &stated)
        .unwrap();
    assert_eq!(
        recorded,
        [
            (Uuid::from_u128(10), "It is one outcome"),
            (Uuid::from_u128(11), "The cause is known"),
            (Uuid::from_u128(12), "It cannot be cut")
        ]
    );
    // A review that does not say ready records what it states all the same.
    assert_eq!(
        level.review_departures(false, &standing, &advice, &stated[..1]),
        Ok(vec![(Uuid::from_u128(10), "It is one outcome")])
    );

    // A departure for one of two open items leaves the other, and the refusal names
    // that one alone; a departure under another point closes nothing.
    let error = level
        .review_departures(true, &standing, &advice, &stated[..2])
        .unwrap_err();
    let actual = refusal(error).actual.unwrap();
    assert_eq!(
        actual,
        format!(
            "open advice on 1 item: candidate {} sizing: split, extra items 2, 5",
            Uuid::from_u128(2)
        )
    );
    let other_point = [departure(2, BindingPoint::Pipeline, "Another point")];
    assert!(
        level
            .review_departures(true, &draft(&[(2, 3)]), &advice, &other_point)
            .is_err()
    );
}

#[test]
fn the_refusal_names_the_open_items_and_the_two_ways_out() {
    let advice = [
        open(12, 2, 3, BindingPoint::Pipeline),
        open(11, 1, 1, BindingPoint::Pipeline),
        open(10, 1, 1, BindingPoint::Sizing),
    ];
    let standing = draft(&[(1, 1), (2, 3)]);
    for (level, rule, route) in [
        (
            SizingLevel::ProgramToScopes,
            "SCOPE-REVIEW-ADVICE-OPEN",
            "scope.candidates.save",
        ),
        (
            SizingLevel::ScopeToSlices,
            "SLICE-REVIEW-ADVICE-OPEN",
            "slice.candidates.save",
        ),
    ] {
        let error = level
            .review_departures(true, &standing, &advice, &[])
            .unwrap_err();
        let refusal = refusal(error);
        assert_eq!(refusal.code, RefusalCode::AdviceNotFollowed);
        assert_eq!(refusal.rule.as_deref(), Some(rule));
        assert_eq!(
            refusal.path.as_deref(),
            Some("/params/review/advice_departures")
        );
        assert_eq!(refusal.next_action.as_deref(), Some(route));
        assert_eq!(
            refusal.required.as_deref(),
            Some("advice_followed_or_departure_reasons")
        );
        assert_eq!(
            refusal.message.as_deref(),
            Some(RefusalCode::AdviceNotFollowed.message())
        );
        assert!(refusal.is_complete_pipeline_refusal());
        let expected = refusal.expected.clone().unwrap();
        assert!(expected.contains("changed as advised and the draft saved again"));
        assert!(expected.contains("advice_departures with a reason"));
        // In draft order, and for one candidate `sizing` before `pipeline`: candidate
        // id, point and what Jev chose.
        assert_eq!(
            refusal.actual.clone().unwrap(),
            format!(
                "open advice on 3 items: candidate {first} sizing: split, extra items 2, 5; \
                 candidate {first} pipeline: slice.research; \
                 candidate {second} pipeline: slice.research",
                first = Uuid::from_u128(1),
                second = Uuid::from_u128(2)
            )
        );
        assert_eq!(SizingLevel::of_open_advice(&refusal), Some(level));
    }
    // Another refusal belongs to no level of this one.
    let other = Refusal::new(RefusalCode::StateConflict).with_rule("SCOPE-REVIEW-ADVICE-OPEN");
    assert_eq!(SizingLevel::of_open_advice(&other), None);
    let unruled = Refusal::new(RefusalCode::AdviceNotFollowed);
    assert_eq!(SizingLevel::of_open_advice(&unruled), None);
}

#[test]
fn what_jev_chose_is_the_advice_with_its_extra_items_or_the_pipeline() {
    let chose = |point: BindingPoint, answer: Value| point.chose(&answer);
    let sizing = |advice: &str, extra: Value| json!({"advice":advice,"extra_items":extra});
    assert_eq!(
        chose(BindingPoint::Sizing, sizing("simplify", json!([]))),
        Some("simplify".into())
    );
    assert_eq!(
        chose(BindingPoint::Sizing, sizing("keep", json!([3]))),
        Some("keep, extra items 3".into())
    );
    assert_eq!(
        chose(BindingPoint::Pipeline, json!({"pipeline":"slice.research"})),
        Some("slice.research".into())
    );
    // A kept answer that does not read back is never guessed at.
    for (point, broken) in [
        (BindingPoint::Sizing, json!({"advice":"split"})),
        (BindingPoint::Sizing, json!({"extra_items":[]})),
        (
            BindingPoint::Pipeline,
            json!({"advice":"split","extra_items":[]}),
        ),
    ] {
        assert_eq!(chose(point, broken.clone()), None, "{broken}");
        let unread = OpenAdvice {
            answer: broken,
            ..open(10, 1, 1, point)
        };
        assert_eq!(
            SizingLevel::ScopeToSlices.review_departures(true, &draft(&[(1, 1)]), &[unread], &[]),
            Err(Error::InternalInvariant)
        );
    }
}

#[test]
fn a_long_list_of_open_items_is_named_as_far_as_it_fits_and_counted_beyond() {
    let advice = (1..=100)
        .map(|candidate| open(1000 + candidate, candidate, 1, BindingPoint::Sizing))
        .collect::<Vec<_>>();
    let standing = (1..=100)
        .map(|candidate| (candidate, 1))
        .collect::<Vec<_>>();
    let error = SizingLevel::ProgramToScopes
        .review_departures(true, &draft(&standing), &advice, &[])
        .unwrap_err();
    let actual = refusal(error).actual.unwrap();
    assert!(
        actual.starts_with("open advice on 100 items: candidate "),
        "{actual}"
    );
    let item = format!(
        "candidate {} sizing: split, extra items 2, 5",
        Uuid::from_u128(1)
    );
    let named = actual.matches("candidate ").count();
    assert_eq!(named, NAMED_OPEN_BYTES / item.len());
    assert!(
        actual.ends_with(&format!("; and {} more", 100 - named)),
        "{actual}"
    );
    assert!(actual.len() < NAMED_OPEN_BYTES + 100, "{}", actual.len());

    // One item is named whatever its length.
    let mut wide = open(10, 1, 1, BindingPoint::Sizing);
    wide.answer["extra_items"] = json!((1..=2000).collect::<Vec<_>>());
    let error = SizingLevel::ProgramToScopes
        .review_departures(
            true,
            &draft(&[(1, 1), (2, 1)]),
            &[wide, advice[1].clone()],
            &[],
        )
        .unwrap_err();
    let actual = refusal(error).actual.unwrap();
    assert!(
        actual.contains(", 1999, 2000; and 1 more"),
        "{}",
        &actual[actual.len() - 60..]
    );
}
