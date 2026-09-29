//! Reviewable, typed DEV/TEST uncertainty evidence. No raw provider payloads.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    Error, MATRIX_NATIVE_RANKING_MIN_CONFIDENCE, MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION,
    MatrixRanking, NativeMatrixChoice, NativeMatrixRankingSignals,
    NativeMatrixRobustTrialEvaluation, NativeMatrixScoreDistribution, Result,
};

pub const MATRIX_TRIAL_POLICY_ID: &str = "tect.matrix-native-ranking-policy";
const POLICY_DEFINITION: &[u8] = b"tect.matrix-native-ranking-policy/robust-trial-v1\0two-options;choice-top-agreement;choice-confidence>=0.70;choice-mass>=0.70;winner-score-confidence>=0.70;cent-feasible-winner-min>loser-max;loser-confidence-disclosed";

pub fn matrix_trial_policy_digest() -> String {
    format!("{:x}", Sha256::digest(POLICY_DEFINITION))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixTrialCandidateEvidence {
    pub candidate_id: String,
    pub declared_score: f64,
    pub score_confidence: f64,
    pub probabilities: [f64; 10],
    pub displayed_mean: f64,
    pub feasible_minimum: f64,
    pub feasible_maximum: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixTrialRankingEvidence {
    pub policy_id: String,
    pub policy_version: String,
    pub policy_digest: String,
    pub choice_selected_candidate_id: String,
    pub choice_confidence: f64,
    pub choice_selected_answer_probability: f64,
    /// Sorted by candidate ID; ranking order is the separate advice outcome.
    pub scores: Vec<MatrixTrialCandidateEvidence>,
    pub low_loser_confidence: bool,
}

impl MatrixTrialRankingEvidence {
    pub fn from_evaluation(
        signals: &NativeMatrixRankingSignals,
        evaluation: &NativeMatrixRobustTrialEvaluation,
    ) -> Result<Self> {
        let MatrixRanking::Ranked {
            ranked_candidate_ids,
            ..
        } = &evaluation.ranking
        else {
            return Err(Error::InvalidArguments);
        };
        let NativeMatrixChoice::Candidate(selected) = &signals.choice else {
            return Err(Error::InvalidArguments);
        };
        let mut scores = signals
            .candidate_scores
            .iter()
            .map(|score| {
                let distribution = score.distribution.as_ref().ok_or(Error::InvalidArguments)?;
                let bounds = distribution.feasible_expected_score();
                Ok(MatrixTrialCandidateEvidence {
                    candidate_id: score.candidate_id.clone(),
                    declared_score: score.score,
                    score_confidence: score.answer_confidence,
                    probabilities: *distribution.probabilities(),
                    displayed_mean: distribution.displayed_mean(),
                    feasible_minimum: bounds.minimum,
                    feasible_maximum: bounds.maximum,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        scores.sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        let evidence = Self {
            policy_id: MATRIX_TRIAL_POLICY_ID.into(),
            policy_version: MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION.into(),
            policy_digest: matrix_trial_policy_digest(),
            choice_selected_candidate_id: selected.clone(),
            choice_confidence: signals.choice_confidence,
            choice_selected_answer_probability: signals.choice_selected_answer_probability,
            scores,
            low_loser_confidence: evaluation.low_loser_confidence,
        };
        evidence.validate_ranked(ranked_candidate_ids)?;
        Ok(evidence)
    }

    pub fn validate_ranked(&self, ranked_candidate_ids: &[String]) -> Result<()> {
        if self.policy_id != MATRIX_TRIAL_POLICY_ID
            || self.policy_version != MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION
            || self.policy_digest != matrix_trial_policy_digest()
            || ranked_candidate_ids.len() != 2
            || self.scores.len() != 2
            || self.scores[0].candidate_id >= self.scores[1].candidate_id
            || self.choice_selected_candidate_id != ranked_candidate_ids[0]
            || self.choice_confidence < MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
            || self.choice_selected_answer_probability < MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
            || ![
                self.choice_confidence,
                self.choice_selected_answer_probability,
            ]
            .into_iter()
            .all(|value| value.is_finite() && value <= 1.0)
        {
            return Err(Error::InvalidArguments);
        }
        for score in &self.scores {
            let distribution = NativeMatrixScoreDistribution::new(score.probabilities)?;
            let bounds = distribution.feasible_expected_score();
            if score.candidate_id.is_empty()
                || !(0.0..=1.0).contains(&score.score_confidence)
                || !score.displayed_mean.is_finite()
                || !score.feasible_minimum.is_finite()
                || !score.feasible_maximum.is_finite()
                || !distribution.consistent_with_declared_score(score.declared_score)
                || (score.displayed_mean - distribution.displayed_mean()).abs() > 1e-9
                || (score.feasible_minimum - bounds.minimum).abs() > 1e-9
                || (score.feasible_maximum - bounds.maximum).abs() > 1e-9
            {
                return Err(Error::InvalidArguments);
            }
        }
        let winner = self
            .scores
            .iter()
            .find(|score| score.candidate_id == ranked_candidate_ids[0])
            .ok_or(Error::InvalidArguments)?;
        let loser = self
            .scores
            .iter()
            .find(|score| score.candidate_id == ranked_candidate_ids[1])
            .ok_or(Error::InvalidArguments)?;
        if winner.score_confidence < MATRIX_NATIVE_RANKING_MIN_CONFIDENCE
            || winner.feasible_minimum <= loser.feasible_maximum + 1e-9
            || winner.displayed_mean <= loser.displayed_mean
            || self.low_loser_confidence
                != (loser.score_confidence < MATRIX_NATIVE_RANKING_MIN_CONFIDENCE)
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MatrixAdviceEligibility, NativeMatrixCandidateScore, evaluate_native_matrix_robust_trial,
    };

    fn fixture() -> (MatrixTrialRankingEvidence, Vec<String>) {
        let mut winner = [0.0; 10];
        winner[8] = 1.0;
        let mut loser = [0.0; 10];
        loser[4] = 1.0;
        let signals = NativeMatrixRankingSignals {
            candidate_scores: vec![
                NativeMatrixCandidateScore {
                    candidate_id: "a".into(),
                    score: 8.0,
                    answer_confidence: 0.9,
                    distribution: Some(NativeMatrixScoreDistribution::new(winner).unwrap()),
                },
                NativeMatrixCandidateScore {
                    candidate_id: "b".into(),
                    score: 4.0,
                    answer_confidence: 0.5,
                    distribution: Some(NativeMatrixScoreDistribution::new(loser).unwrap()),
                },
            ],
            choice: NativeMatrixChoice::Candidate("a".into()),
            choice_confidence: 0.8,
            choice_selected_answer_probability: 0.8,
        };
        let evaluation = evaluate_native_matrix_robust_trial(
            &MatrixAdviceEligibility::EligibleForAdvice {
                candidate_ids: vec!["a".into(), "b".into()],
            },
            &signals,
        )
        .unwrap();
        let MatrixRanking::Ranked {
            ranked_candidate_ids,
            ..
        } = &evaluation.ranking
        else {
            panic!("fixture must rank")
        };
        (
            MatrixTrialRankingEvidence::from_evaluation(&signals, &evaluation).unwrap(),
            ranked_candidate_ids.clone(),
        )
    }

    #[test]
    fn trial_uncertainty_is_typed_roundtrippable_and_tamper_checked() {
        let (evidence, ranks) = fixture();
        let json = serde_json::to_value(&evidence).unwrap();
        let decoded: MatrixTrialRankingEvidence = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(decoded, evidence);
        assert_eq!(decoded.validate_ranked(&ranks), Ok(()));
        assert!(decoded.low_loser_confidence);
        for pointer in [
            "/policy_digest",
            "/scores/0/feasible_minimum",
            "/scores/0/probabilities/8",
            "/choice_selected_candidate_id",
        ] {
            let mut changed = json.clone();
            *changed.pointer_mut(pointer).unwrap() = serde_json::json!("tampered");
            if let Ok(parsed) = serde_json::from_value::<MatrixTrialRankingEvidence>(changed) {
                assert!(parsed.validate_ranked(&ranks).is_err(), "{pointer}");
            }
        }
        let mut changed = evidence.clone();
        changed.low_loser_confidence = false;
        assert!(changed.validate_ranked(&ranks).is_err());
        assert!(evidence.validate_ranked(&["b".into(), "a".into()]).is_err());
    }
}
