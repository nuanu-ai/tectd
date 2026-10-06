use super::*;

#[test]
fn live_route_input_gate_accepts_exact_pins_and_denies_catalogue_or_provider_drift() {
    let fixture = fixture();
    let prepared = &fixture.prepared;
    let gate = crate::model_route_authority::require_current_route_inputs;
    assert!(
        gate(
            prepared,
            prepared.catalogue.as_ref(),
            &prepared.work.host_capabilities
        )
        .is_ok()
    );
    assert!(matches!(
        gate(prepared, None, &prepared.work.host_capabilities),
        Err(Error::StaleContext)
    ));
    let mut changed = prepared.catalogue.clone().unwrap();
    changed.routes[0].provider = "changed-provider".into();
    assert!(matches!(
        gate(prepared, Some(&changed), &prepared.work.host_capabilities),
        Err(Error::StaleContext)
    ));
    let mut changed = prepared.catalogue.clone().unwrap();
    changed.version += 1;
    assert!(matches!(
        gate(prepared, Some(&changed), &prepared.work.host_capabilities),
        Err(Error::StaleContext)
    ));
}

#[test]
fn live_route_input_gate_preserves_unknown_empty_and_full_capability_provenance() {
    let fixture = fixture();
    let prepared = &fixture.prepared;
    let gate = crate::model_route_authority::require_current_route_inputs;
    for changed in [
        ModelRouteFact::Unknown,
        ModelRouteHostCapabilities {
            schema: tect_domain::MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec![],
        }
        .fact()
        .unwrap(),
        ModelRouteHostCapabilities {
            schema: tect_domain::MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 2,
            capabilities: vec!["owned-stdio".into()],
        }
        .fact()
        .unwrap(),
    ] {
        assert!(matches!(
            gate(prepared, prepared.catalogue.as_ref(), &changed),
            Err(Error::StaleContext)
        ));
    }
}

#[test]
fn live_route_input_gate_recomputes_eligibility_and_denies_expired_operating_evidence() {
    let fixture = fixture();
    let gate = crate::model_route_authority::require_current_route_inputs;
    let mut prepared = fixture.prepared.clone();
    prepared.eligible.as_mut().unwrap().route_ids.clear();
    assert!(matches!(
        gate(
            &prepared,
            prepared.catalogue.as_ref(),
            &prepared.work.host_capabilities
        ),
        Err(Error::StaleContext)
    ));
    let mut prepared = fixture.prepared;
    let ModelRouteFact::Known {
        provenance:
            ModelRouteFactProvenance::OperatingEvidence {
                expires_at_epoch_ms,
                ..
            },
        ..
    } = &mut prepared.work.available_latency_ms
    else {
        panic!("fixture has operating evidence")
    };
    *expires_at_epoch_ms = 2;
    assert!(matches!(
        gate(
            &prepared,
            prepared.catalogue.as_ref(),
            &prepared.work.host_capabilities
        ),
        Err(Error::StaleContext)
    ));
}
