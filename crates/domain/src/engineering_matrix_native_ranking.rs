//! Provisional native TypeSafe signal policy for optional Matrix ranking.
//! This heuristic is not calibrated and is not release or acceptance proof.

use crate::{Error, MatrixAdviceEligibility, MatrixRanking, Result};
use std::collections::BTreeSet;

pub const MATRIX_NATIVE_RANKING_POLICY_VERSION: &str =
    "tect.matrix-native-ranking-policy/provisional-v1";
pub const MATRIX_NATIVE_RANKING_MIN_CONFIDENCE: f64 = 0.70;

/// Already parsed and validated signal for one fixed ten-level suitability Score.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixCandidateScore {
    pub candidate_id: String,
    /// Suitability level 0..=9; larger is more suitable.
    pub score: u8,
    pub answer_confidence: f64,
    pub selected_answer_probability: f64,
}

/// Already parsed selection from the single Choice question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeMatrixChoice {
    Candidate(String),
    Abstain,
}

/// One Score per candidate and one Choice over all candidates plus ABSTAIN.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixRankingSignals {
    pub candidate_scores: Vec<NativeMatrixCandidateScore>,
    pub choice: NativeMatrixChoice,
    pub choice_confidence: f64,
    pub choice_selected_answer_probability: f64,
}

/// Produces a complete descending ranking only when every native signal agrees.
/// Any signal-level failure yields typed abstention; invalid eligibility is an error.
pub fn compose_native_matrix_ranking(
    eligibility: &MatrixAdviceEligibility,
    signals: &NativeMatrixRankingSignals,
) -> Result<MatrixRanking> {
    let MatrixAdviceEligibility::EligibleForAdvice { candidate_ids } = eligibility else {
        return Err(Error::InvalidArguments);
    };
    let eligible_ids = candidate_ids.iter().collect::<BTreeSet<_>>();
    if !(2..=5).contains(&candidate_ids.len()) || eligible_ids.len() != candidate_ids.len() {
        return Err(Error::InvalidArguments);
    }

    let abstained = || MatrixRanking::Abstained {
        ranked_candidate_ids: Vec::new(),
        recommended_candidate_id: None,
    };
    let confident = |value: f64| (MATRIX_NATIVE_RANKING_MIN_CONFIDENCE..=1.0).contains(&value);
    let score_ids = signals
        .candidate_scores
        .iter()
        .map(|score| &score.candidate_id)
        .collect::<BTreeSet<_>>();
    let mut ranking = abstained();
    if signals.candidate_scores.len() == candidate_ids.len()
        && score_ids == eligible_ids
        && confident(signals.choice_confidence)
        && confident(signals.choice_selected_answer_probability)
        && signals.candidate_scores.iter().all(|score| {
            score.score <= 9
                && confident(score.answer_confidence)
                && confident(score.selected_answer_probability)
        })
    {
        let mut sorted = signals.candidate_scores.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.score.cmp(&a.score));
        let distinct_scores = sorted.windows(2).all(|pair| pair[0].score != pair[1].score);
        if distinct_scores {
            if let NativeMatrixChoice::Candidate(winner) = &signals.choice {
                if winner == &sorted[0].candidate_id {
                    ranking = MatrixRanking::Ranked {
                        ranked_candidate_ids: sorted
                            .iter()
                            .map(|score| score.candidate_id.clone())
                            .collect(),
                        recommended_candidate_id: winner.clone(),
                    };
                }
            }
        }
    }
    ranking.validate(eligibility)?;
    Ok(ranking)
}

#[cfg(test)]
#[path = "engineering_matrix_native_ranking_tests.rs"]
mod tests;
