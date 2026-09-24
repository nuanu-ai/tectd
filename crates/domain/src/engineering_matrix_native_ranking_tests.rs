use super::*;

fn eligible() -> MatrixAdviceEligibility {
    MatrixAdviceEligibility::EligibleForAdvice {
        candidate_ids: vec!["a".into(), "b".into(), "c".into()],
    }
}

fn score(id: &str, level: u8) -> NativeMatrixCandidateScore {
    NativeMatrixCandidateScore {
        candidate_id: id.into(),
        score: level,
        answer_confidence: 0.8,
        selected_answer_probability: 0.8,
    }
}

fn signals() -> NativeMatrixRankingSignals {
    NativeMatrixRankingSignals {
        candidate_scores: vec![score("a", 3), score("b", 9), score("c", 6)],
        choice: NativeMatrixChoice::Candidate("b".into()),
        choice_confidence: 0.8,
        choice_selected_answer_probability: 0.8,
    }
}

fn ranked() -> MatrixRanking {
    MatrixRanking::Ranked {
        ranked_candidate_ids: vec!["b".into(), "c".into(), "a".into()],
        recommended_candidate_id: "b".into(),
    }
}

fn abstained() -> MatrixRanking {
    MatrixRanking::Abstained {
        ranked_candidate_ids: vec![],
        recommended_candidate_id: None,
    }
}

#[test]
fn complete_distinct_scores_and_matching_choice_rank() {
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &signals()),
        Ok(ranked())
    );
}

#[test]
fn tied_score_contradictory_choice_or_abstain_never_rank() {
    let mut input = signals();
    input.candidate_scores[2].score = 9;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.choice = NativeMatrixChoice::Candidate("c".into());
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.choice = NativeMatrixChoice::Abstain;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );
}

#[test]
fn threshold_is_inclusive_and_applies_to_each_confidence_and_probability() {
    let mut edge = signals();
    for score in &mut edge.candidate_scores {
        score.answer_confidence = 0.70;
        score.selected_answer_probability = 0.70;
    }
    edge.choice_confidence = 0.70;
    edge.choice_selected_answer_probability = 0.70;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &edge),
        Ok(ranked())
    );

    for index in 0..edge.candidate_scores.len() {
        let mut low = edge.clone();
        low.candidate_scores[index].answer_confidence = 0.699;
        assert_eq!(
            compose_native_matrix_ranking(&eligible(), &low),
            Ok(abstained())
        );
        let mut low = edge.clone();
        low.candidate_scores[index].selected_answer_probability = 0.699;
        assert_eq!(
            compose_native_matrix_ranking(&eligible(), &low),
            Ok(abstained())
        );
    }
    let mut low = edge.clone();
    low.choice_confidence = 0.699;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &low),
        Ok(abstained())
    );
    let mut low = edge;
    low.choice_selected_answer_probability = 0.699;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &low),
        Ok(abstained())
    );
}

#[test]
fn duplicate_missing_extra_or_out_of_range_scores_abstain() {
    let mut input = signals();
    input.candidate_scores[2].candidate_id = "b".into();
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.candidate_scores.pop();
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.candidate_scores[2].candidate_id = "extra".into();
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.candidate_scores[2].score = 10;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );
}

#[test]
fn input_and_eligible_order_do_not_affect_ranking() {
    let mut input = signals();
    input.candidate_scores.reverse();
    let reversed = MatrixAdviceEligibility::EligibleForAdvice {
        candidate_ids: vec!["c".into(), "b".into(), "a".into()],
    };
    assert_eq!(
        compose_native_matrix_ranking(&reversed, &input),
        Ok(ranked())
    );
}

#[test]
fn noneligible_or_malformed_eligibility_is_rejected() {
    assert_eq!(
        compose_native_matrix_ranking(&MatrixAdviceEligibility::NotApplicable, &signals()),
        Err(Error::InvalidArguments)
    );
    let duplicate = MatrixAdviceEligibility::EligibleForAdvice {
        candidate_ids: vec!["a".into(), "a".into()],
    };
    assert_eq!(
        compose_native_matrix_ranking(&duplicate, &signals()),
        Err(Error::InvalidArguments)
    );
}
