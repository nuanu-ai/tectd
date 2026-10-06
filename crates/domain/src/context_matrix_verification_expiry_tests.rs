use super::*;

fn classify(
    input: &EngineeringMatrixInput,
    context: &EffectiveMatrixRequirements,
    record: &ContextMatrixVerificationRecord,
) -> Result<ContextMatrixVerificationCurrentness> {
    classify_context_matrix_verification("task", "1", SNAPSHOT_ID, input, context, record, 20)
}

fn redigest(record: &mut ContextMatrixVerificationRecord) {
    record.digest = record.canonical_digest().unwrap();
}

#[test]
fn valid_current_expired_and_equality_now_are_distinct_without_expired_token() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let baseline = record(&input, &context);
    let current = classify(&input, &context, &baseline).unwrap();
    let ContextMatrixVerificationCurrentness::Current(token) = current else {
        panic!("otherwise-valid current evidence must produce a token");
    };
    assert_eq!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &context,
            &baseline,
            20,
        ),
        Ok(token)
    );
    for expires_at in [15, 20] {
        let mut expired = baseline.clone();
        expired.bindings[0].expires_at = expires_at;
        redigest(&mut expired);
        assert_eq!(
            classify(&input, &context, &expired),
            Ok(ContextMatrixVerificationCurrentness::Expired)
        );
        assert_eq!(
            evaluate_context_matrix_verification(
                "task",
                "1",
                SNAPSHOT_ID,
                &input,
                &context,
                &expired,
                20,
            ),
            Err(Error::InvalidArguments)
        );
    }
}

#[test]
fn expired_evidence_does_not_hide_schema_digest_or_binding_corruption() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let mut expired = record(&input, &context);
    expired.bindings[0].expires_at = 15;
    redigest(&mut expired);
    for case in 0..3 {
        let mut invalid = expired.clone();
        match case {
            0 => invalid.schema = "other-schema".into(),
            1 => invalid.digest = "wrong-digest".into(),
            _ => invalid.bindings[0].value_digest = "wrong-binding".into(),
        }
        if case != 1 {
            redigest(&mut invalid);
        }
        assert_eq!(
            classify(&input, &context, &invalid),
            Err(Error::InvalidArguments)
        );
    }
}

#[test]
fn invalid_expiry_order_is_not_classified_as_expired() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let baseline = record(&input, &context);
    for expires_at in [9, 10] {
        let mut invalid = baseline.clone();
        invalid.bindings[0].expires_at = expires_at;
        redigest(&mut invalid);
        assert_eq!(
            classify(&input, &context, &invalid),
            Err(Error::InvalidArguments)
        );
    }
}

#[test]
fn first_expired_binding_does_not_hide_later_malformed_or_future_observed_binding() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let mut expired = record(&input, &context);
    assert!(expired.bindings.len() > 1);
    expired.bindings[0].expires_at = 15;
    for future_observed in [false, true] {
        let mut invalid = expired.clone();
        if future_observed {
            invalid.bindings[1].observed_at = 21;
        } else {
            invalid.bindings[1].evidence_ref = "\0".into();
        }
        redigest(&mut invalid);
        assert_eq!(
            classify(&input, &context, &invalid),
            Err(Error::InvalidArguments)
        );
    }
}
