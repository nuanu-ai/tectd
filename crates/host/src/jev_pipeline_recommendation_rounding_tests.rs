use super::*;

fn set_score(value: &mut Value, index: usize, score: f64, masses: &[(usize, f64)]) {
    let answer = &mut value["answers"][format!("score_v1_{index}")];
    answer["score"] = json!(score);
    for level in 0..10 {
        answer["probabilities"][level.to_string()] = json!(
            masses
                .iter()
                .find(|(candidate, _)| *candidate == level)
                .map_or(0.0, |(_, mass)| *mass)
        );
    }
}

#[test]
fn both_reported_score_discrepancies_parse_together() {
    let prepared = prepared(2);
    let mut value = response(&prepared);
    set_score(&mut value, 0, 5.98, &[(0, 0.06), (4, 0.22), (7, 0.72)]);
    let second = (0..10)
        .map(|level| (level, if level == 1 { 0.91 } else { 0.01 }))
        .collect::<Vec<_>>();
    set_score(&mut value, 1, 1.30, &second);
    let parsed = parse(&value, &prepared).unwrap();
    assert_eq!(parsed.scores[0].1, 5.98);
    assert_eq!(parsed.scores[1].1, 1.30);
    assert_eq!(
        parsed.ranking,
        PipelineRecommendationRanking::Ranked {
            ranked_ids: prepared.eligible_ids.clone()
        }
    );
}

#[test]
fn reported_score_drives_ranking_when_displayed_means_reverse_it() {
    let prepared = prepared(2);
    let mut value = response(&prepared);
    set_score(&mut value, 0, 5.98, &[(0, 0.06), (4, 0.22), (7, 0.72)]); // displayed mean 5.92
    set_score(&mut value, 1, 5.94, &[(5, 0.05), (6, 0.95)]); // displayed mean 5.95
    assert_eq!(
        parse(&value, &prepared).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::InsufficientScoreSeparation)
    );
    let parsed = parse(&value, &prepared).unwrap();
    assert_eq!(parsed.scores[0].1, 5.98);
    assert_eq!(parsed.scores[1].1, 5.94);
}

#[test]
fn score_rounding_is_scoped_to_jev_113_and_structure_stays_strict() {
    let prepared = prepared(2);
    let mut value = response(&prepared);
    set_score(&mut value, 0, 5.98, &[(0, 0.06), (4, 0.22), (7, 0.72)]);
    assert!(parse(&value, &prepared).is_ok());
    value["model"] = json!("jev-1.12.0");
    let mut older = prepared.clone();
    older.model = "jev-1.12.0".into();
    assert_eq!(parse(&value, &older), Err(Error::InvalidArguments));

    let mut value = response(&prepared);
    set_score(&mut value, 0, 5.98, &[(0, 0.20), (8, 0.70)]);
    assert_eq!(parse(&value, &prepared), Err(Error::InvalidArguments));
    let mut value = response(&prepared);
    set_score(&mut value, 0, 5.98, &[(0, 0.06), (4, 0.22), (7, 0.72)]);
    value["answers"]["score_v1_0"]["probabilities"]["9"] = json!(1.01);
    assert_eq!(parse(&value, &prepared), Err(Error::InvalidArguments));
}

#[test]
fn score_rounding_is_not_applied_to_jev_113_patch_versions() {
    let mut prepared = prepared(2);
    prepared.model = "jev-1.13.1".into();
    let mut value = response(&prepared);
    value["model"] = json!("jev-1.13.1");
    set_score(&mut value, 0, 5.98, &[(0, 0.06), (4, 0.22), (7, 0.72)]);

    // The displayed expectation is 5.92, so the legacy exact-score rule rejects 5.98.
    assert_eq!(parse(&value, &prepared), Err(Error::InvalidArguments));

    set_score(&mut value, 0, 5.92, &[(0, 0.06), (4, 0.22), (7, 0.72)]);
    assert!(parse(&value, &prepared).is_ok());
}
