use super::*;

fn eligible() -> MatrixAdviceEligibility {
    MatrixAdviceEligibility::EligibleForAdvice {
        candidate_ids: vec!["a".into(), "b".into(), "c".into()],
    }
}

fn score(id: &str, mean: f64) -> NativeMatrixCandidateScore {
    let mut probabilities = [0.0; 10];
    let lower = mean.floor() as usize;
    probabilities[lower] = 1.0 - mean.fract();
    if mean.fract() > 0.0 {
        probabilities[lower + 1] = mean.fract();
    }
    NativeMatrixCandidateScore {
        candidate_id: id.into(),
        score: mean,
        answer_confidence: 0.8,
        distribution: Some(NativeMatrixScoreDistribution::new(probabilities).unwrap()),
    }
}

fn trial_signals() -> NativeMatrixRankingSignals {
    NativeMatrixRankingSignals {
        candidate_scores: vec![score("a", 8.0), score("b", 4.0)],
        choice: NativeMatrixChoice::Candidate("a".into()),
        choice_confidence: 0.9,
        choice_selected_answer_probability: 0.9,
    }
}

fn trial_eligible() -> MatrixAdviceEligibility {
    MatrixAdviceEligibility::EligibleForAdvice {
        candidate_ids: vec!["a".into(), "b".into()],
    }
}

#[test]
fn robust_trial_ranks_only_two_robustly_separated_options_and_retains_low_loser_confidence() {
    let mut input = trial_signals();
    input.candidate_scores[1].answer_confidence = 0.31;
    assert_eq!(
        compose_native_matrix_ranking(&trial_eligible(), &input),
        Ok(abstained())
    );
    let evaluated = evaluate_native_matrix_robust_trial(&trial_eligible(), &input).unwrap();
    assert!(matches!(evaluated.ranking, MatrixRanking::Ranked { .. }));
    assert!(evaluated.low_loser_confidence);
    assert_eq!(evaluated.score_evidence[1].answer_confidence, 0.31);
    assert_eq!(evaluated.score_evidence[0].candidate_id, "a");
    input.candidate_scores.reverse();
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &input).unwrap(),
        evaluated
    );
    assert_eq!(
        evaluate_native_matrix_robust_trial(&eligible(), &signals()),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn robust_trial_abstains_on_overlap_tie_reversal_and_low_winner_confidence() {
    let base = trial_signals();
    let mut overlap = base.clone();
    overlap.candidate_scores[1] = score("b", 7.95);
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &overlap)
            .unwrap()
            .ranking,
        abstained()
    );
    overlap.candidate_scores[1] = score("b", 8.0);
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &overlap)
            .unwrap()
            .ranking,
        abstained()
    );
    let mut reversed = base.clone();
    reversed.choice = NativeMatrixChoice::Candidate("b".into());
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &reversed)
            .unwrap()
            .ranking,
        abstained()
    );
    let mut low_winner = base;
    low_winner.candidate_scores[0].answer_confidence = 0.699;
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &low_winner)
            .unwrap()
            .ranking,
        abstained()
    );
}

#[test]
fn robust_trial_rejects_invalid_or_missing_distributions_and_cent_rounding() {
    let mut input = trial_signals();
    input.candidate_scores[0].distribution = None;
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &input),
        Err(Error::InvalidArguments)
    );
    input = trial_signals();
    input.candidate_scores[0].score = 7.9;
    assert_eq!(
        evaluate_native_matrix_robust_trial(&trial_eligible(), &input),
        Err(Error::InvalidArguments)
    );
    let mut rounded = [0.0; 10];
    rounded[7] = 0.05;
    rounded[8] = 0.85;
    rounded[9] = 0.10;
    let interval = NativeMatrixScoreDistribution::new(rounded)
        .unwrap()
        .feasible_expected_score();
    assert!(interval.minimum <= 8.0 && interval.maximum >= 8.0);
    rounded[8] = 0.75;
    assert_eq!(
        NativeMatrixScoreDistribution::new(rounded),
        Err(Error::InvalidArguments)
    );
    rounded[8] = f64::NAN;
    assert_eq!(
        NativeMatrixScoreDistribution::new(rounded),
        Err(Error::InvalidArguments)
    );
}

fn signals() -> NativeMatrixRankingSignals {
    NativeMatrixRankingSignals {
        candidate_scores: vec![score("a", 3.25), score("b", 8.75), score("c", 6.5)],
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
    input.candidate_scores[2].score = 8.75;
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
fn threshold_is_inclusive_for_score_and_choice_confidence_and_choice_mass() {
    let mut edge = signals();
    for score in &mut edge.candidate_scores {
        score.answer_confidence = 0.70;
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
    input.candidate_scores[2].score = 9.001;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );

    let mut input = signals();
    input.candidate_scores[2].score = f64::NAN;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );
}

#[test]
fn adjacent_mean_gap_must_be_strictly_greater_than_tenth() {
    let mut input = signals();
    input.candidate_scores[2].score = 8.65;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(abstained())
    );
    input.candidate_scores[2].score = 8.64;
    assert_eq!(
        compose_native_matrix_ranking(&eligible(), &input),
        Ok(ranked())
    );
    input.candidate_scores[2].score = 8.70;
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
