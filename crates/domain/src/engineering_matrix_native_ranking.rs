//! Provisional native TypeSafe signal policy for optional Matrix ranking.
//! This heuristic is not calibrated and is not release or acceptance proof.

use crate::{Error, MatrixAdviceEligibility, MatrixRanking, Result};
use std::collections::BTreeSet;

pub const MATRIX_NATIVE_RANKING_POLICY_VERSION: &str =
    "tect.matrix-native-ranking-policy/provisional-v1";
/// Deliberately not selected by the native provider or production configuration.
pub const MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION: &str =
    "tect.matrix-native-ranking-policy/robust-trial-v1";
pub const MATRIX_NATIVE_RANKING_MIN_CONFIDENCE: f64 = 0.70;
pub const MATRIX_NATIVE_RANKING_MIN_SCORE_GAP: f64 = 0.10;
const CENT_HALF: f64 = 0.005;
const FLOAT_EPSILON: f64 = 1e-9;

/// Bounds on the expected score if each displayed cent probability was rounded
/// independently, subject to the true probabilities summing to exactly one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeMatrixExpectedScoreInterval {
    pub minimum: f64,
    pub maximum: f64,
}

/// Validated ten-level Score distribution, retained for explicit trial policy
/// evaluation. This is not a joint distribution over alternatives.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixScoreDistribution {
    probabilities: [f64; 10],
    interval: NativeMatrixExpectedScoreInterval,
    cent_quantized: bool,
}

impl NativeMatrixScoreDistribution {
    pub fn new(probabilities: [f64; 10]) -> Result<Self> {
        if probabilities
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err(Error::InvalidArguments);
        }
        let cent = |value: f64| (value * 100.0 - (value * 100.0).round()).abs() <= FLOAT_EPSILON;
        let cent_quantized = probabilities.iter().all(|value| cent(*value));
        let interval = if cent_quantized {
            let lower = probabilities.map(|value| (value - CENT_HALF).max(0.0));
            let upper = probabilities.map(|value| (value + CENT_HALF).min(1.0));
            NativeMatrixExpectedScoreInterval {
                minimum: feasible_expected_score_bound(&lower, &upper, false)
                    .ok_or(Error::InvalidArguments)?,
                maximum: feasible_expected_score_bound(&lower, &upper, true)
                    .ok_or(Error::InvalidArguments)?,
            }
        } else {
            if (probabilities.iter().sum::<f64>() - 1.0).abs() > 1e-6 {
                return Err(Error::InvalidArguments);
            }
            let mean = probabilities
                .iter()
                .enumerate()
                .map(|(level, value)| level as f64 * value)
                .sum();
            NativeMatrixExpectedScoreInterval {
                minimum: mean,
                maximum: mean,
            }
        };
        Ok(Self {
            probabilities,
            interval,
            cent_quantized,
        })
    }

    pub fn probabilities(&self) -> &[f64; 10] {
        &self.probabilities
    }

    pub fn displayed_mean(&self) -> f64 {
        self.probabilities
            .iter()
            .enumerate()
            .map(|(level, value)| level as f64 * value)
            .sum()
    }

    pub fn feasible_expected_score(&self) -> NativeMatrixExpectedScoreInterval {
        self.interval
    }

    /// Match the exact high-precision contract, or the feasible independent
    /// cent rounding of both the displayed distribution and declared score.
    pub fn consistent_with_declared_score(&self, declared: f64) -> bool {
        if !declared.is_finite() || !(0.0..=9.0).contains(&declared) {
            return false;
        }
        let cent = (declared * 100.0 - (declared * 100.0).round()).abs() <= FLOAT_EPSILON;
        if !self.cent_quantized || !cent {
            (self.displayed_mean() - declared).abs() <= 1e-6
        } else {
            self.interval.minimum <= declared + CENT_HALF + FLOAT_EPSILON
                && self.interval.maximum + FLOAT_EPSILON >= declared - CENT_HALF
        }
    }
}

fn feasible_expected_score_bound(
    lower: &[f64; 10],
    upper: &[f64; 10],
    maximize: bool,
) -> Option<f64> {
    let mut remaining = 1.0 - lower.iter().sum::<f64>();
    if remaining < -FLOAT_EPSILON || upper.iter().sum::<f64>() < 1.0 - FLOAT_EPSILON {
        return None;
    }
    remaining = remaining.max(0.0);
    let mut score = lower
        .iter()
        .enumerate()
        .map(|(level, value)| level as f64 * value)
        .sum::<f64>();
    for index in 0..10 {
        let level = if maximize { 9 - index } else { index };
        let added = remaining.min(upper[level] - lower[level]);
        score += level as f64 * added;
        remaining -= added;
    }
    (remaining <= FLOAT_EPSILON).then_some(score)
}

/// Already parsed and validated signal for one fixed ten-level suitability Score.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixCandidateScore {
    pub candidate_id: String,
    /// Probability-weighted mean in 0..=9; larger is more suitable.
    pub score: f64,
    pub answer_confidence: f64,
    /// Strict-v1 does not use this field; robust-trial-v1 requires it.
    pub distribution: Option<NativeMatrixScoreDistribution>,
}

/// Evidence returned with the trial decision for later durable/public review.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixTrialScoreEvidence {
    pub candidate_id: String,
    pub displayed_mean: f64,
    pub answer_confidence: f64,
    pub feasible_expected_score: NativeMatrixExpectedScoreInterval,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeMatrixRobustTrialEvaluation {
    pub ranking: MatrixRanking,
    pub score_evidence: Vec<NativeMatrixTrialScoreEvidence>,
    pub choice_confidence: f64,
    pub choice_selected_answer_probability: f64,
    pub low_loser_confidence: bool,
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

/// Produces a complete descending ranking only when every native signal agrees
/// and every adjacent mean-score gap is strictly greater than 0.10.
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
        && signals
            .candidate_scores
            .iter()
            .all(|score| (0.0..=9.0).contains(&score.score) && confident(score.answer_confidence))
    {
        let mut sorted = signals.candidate_scores.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| b.score.total_cmp(&a.score));
        let separated_scores = sorted
            .windows(2)
            .all(|pair| pair[0].score - pair[1].score > MATRIX_NATIVE_RANKING_MIN_SCORE_GAP);
        if separated_scores
            && let NativeMatrixChoice::Candidate(winner) = &signals.choice
            && winner == &sorted[0].candidate_id
        {
            ranking = MatrixRanking::Ranked {
                ranked_candidate_ids: sorted
                    .iter()
                    .map(|score| score.candidate_id.clone())
                    .collect(),
                recommended_candidate_id: winner.clone(),
            };
        }
    }
    ranking.validate(eligibility)?;
    Ok(ranking)
}

/// An explicit, two-option DEV/TEST trial only. Invalid signal shape is an
/// error; valid but indecisive signals produce a typed abstention. The native
/// provider continues to use strict-v1 until a separately authorized seam is
/// implemented.
pub fn evaluate_native_matrix_robust_trial(
    eligibility: &MatrixAdviceEligibility,
    signals: &NativeMatrixRankingSignals,
) -> Result<NativeMatrixRobustTrialEvaluation> {
    let MatrixAdviceEligibility::EligibleForAdvice { candidate_ids } = eligibility else {
        return Err(Error::InvalidArguments);
    };
    let eligible_ids = candidate_ids.iter().collect::<BTreeSet<_>>();
    if candidate_ids.len() != 2 || eligible_ids.len() != 2 || signals.candidate_scores.len() != 2 {
        return Err(Error::InvalidArguments);
    }
    let score_ids = signals
        .candidate_scores
        .iter()
        .map(|score| &score.candidate_id)
        .collect::<BTreeSet<_>>();
    if score_ids != eligible_ids
        || ![
            signals.choice_confidence,
            signals.choice_selected_answer_probability,
        ]
        .into_iter()
        .all(|value| (0.0..=1.0).contains(&value))
        || matches!(&signals.choice, NativeMatrixChoice::Candidate(id) if !eligible_ids.contains(id))
    {
        return Err(Error::InvalidArguments);
    }
    let mut score_evidence = signals
        .candidate_scores
        .iter()
        .map(|score| {
            let distribution = score.distribution.as_ref().ok_or(Error::InvalidArguments)?;
            if !(0.0..=9.0).contains(&score.score)
                || !(0.0..=1.0).contains(&score.answer_confidence)
                || (distribution.displayed_mean() - score.score).abs() > 1e-6
            {
                return Err(Error::InvalidArguments);
            }
            Ok(NativeMatrixTrialScoreEvidence {
                candidate_id: score.candidate_id.clone(),
                displayed_mean: score.score,
                answer_confidence: score.answer_confidence,
                feasible_expected_score: distribution.feasible_expected_score(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    score_evidence.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    let mut descending = score_evidence.iter().collect::<Vec<_>>();
    descending.sort_by(|a, b| b.displayed_mean.total_cmp(&a.displayed_mean));
    let winner = descending[0];
    let loser = descending[1];
    let low_loser_confidence = loser.answer_confidence < MATRIX_NATIVE_RANKING_MIN_CONFIDENCE;
    let ranked = signals.choice_confidence >= MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
        && signals.choice_selected_answer_probability >= MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
        && winner.answer_confidence >= MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
        && winner.feasible_expected_score.minimum
            > loser.feasible_expected_score.maximum + FLOAT_EPSILON
        && matches!(&signals.choice, NativeMatrixChoice::Candidate(id) if id == &winner.candidate_id);
    let ranking = if ranked {
        MatrixRanking::Ranked {
            ranked_candidate_ids: descending
                .iter()
                .map(|score| score.candidate_id.clone())
                .collect(),
            recommended_candidate_id: winner.candidate_id.clone(),
        }
    } else {
        MatrixRanking::Abstained {
            ranked_candidate_ids: Vec::new(),
            recommended_candidate_id: None,
        }
    };
    ranking.validate(eligibility)?;
    Ok(NativeMatrixRobustTrialEvaluation {
        ranking,
        score_evidence,
        choice_confidence: signals.choice_confidence,
        choice_selected_answer_probability: signals.choice_selected_answer_probability,
        low_loser_confidence,
    })
}

#[cfg(test)]
#[path = "engineering_matrix_native_ranking_tests.rs"]
mod tests;
