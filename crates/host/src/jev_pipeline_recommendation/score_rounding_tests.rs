use super::*;

#[test]
fn exact_and_reported_jev_113_examples_fit_latent_rounding_envelope() {
    let a = (0..10)
        .map(|level| {
            (
                level as f64,
                if level == 0 {
                    0.06
                } else if level == 4 {
                    0.22
                } else if level == 7 {
                    0.72
                } else {
                    0.0
                },
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        a.iter().map(|(value, mass)| value * mass).sum::<f64>(),
        5.92
    );
    assert_eq!(validate_jev_113_score(&a, 5.92), Ok(()));
    assert_eq!(validate_jev_113_score(&a, 5.98), Ok(()));

    let b = (0..10)
        .map(|level| (level as f64, if level == 1 { 0.91 } else { 0.01 }))
        .collect::<Vec<_>>();
    assert!((b.iter().map(|(value, mass)| value * mass).sum::<f64>() - 1.35).abs() < 1e-12);
    assert_eq!(validate_jev_113_score(&b, 1.30), Ok(()));
}

#[test]
fn outside_envelope_and_impossible_normalization_reject() {
    let levels = [(0.0, 0.5), (9.0, 0.5)];
    assert_eq!(validate_jev_113_score(&levels, 4.5), Ok(()));
    assert_eq!(
        validate_jev_113_score(&levels, 4.57),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        validate_jev_113_score(&[(0.0, 0.2), (9.0, 0.2)], 4.5),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        validate_jev_113_score(&[(0.0, 0.8), (9.0, 0.8)], 4.5),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        validate_jev_113_score(&levels, 4.501),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        validate_jev_113_score(&[(0.0, 0.5001), (9.0, 0.4999)], 4.5),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        validate_jev_113_score(&[(-1.0, 0.5), (9.0, 0.5)], 4.0),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn endpoints_are_clipped_and_nonuniform_shuffled_levels_are_order_independent() {
    assert_eq!(
        validate_jev_113_score(&[(0.0, 1.0), (9.0, 0.0)], 0.05),
        Ok(())
    );
    assert_eq!(
        validate_jev_113_score(&[(0.0, 1.0), (9.0, 0.0)], 0.06),
        Err(Error::InvalidArguments)
    );
    let ordered = [(0.0, 0.3), (2.0, 0.3), (9.0, 0.4)];
    let shuffled = [(9.0, 0.4), (0.0, 0.3), (2.0, 0.3)];
    assert_eq!(validate_jev_113_score(&ordered, 4.21), Ok(()));
    assert_eq!(validate_jev_113_score(&shuffled, 4.21), Ok(()));
    assert_eq!(
        validate_jev_113_score(&shuffled, 4.30),
        Err(Error::InvalidArguments)
    );
}
