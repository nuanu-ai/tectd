use super::*;

#[test]
fn host_capability_snapshot_is_explicit_even_when_empty() {
    let snapshot = ModelRouteHostCapabilities {
        schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
        version: 3,
        capabilities: vec![],
    };
    let fact = snapshot.fact().unwrap();
    let evidence = validate_host_fact(&fact).unwrap().unwrap();
    assert!(evidence.ends_with(&snapshot.digest().unwrap()));
    assert_eq!(validate_host_fact(&ModelRouteFact::Unknown), Ok(None));
}

#[test]
fn caller_or_tampered_host_capabilities_cannot_enter_receipt() {
    let caller = ModelRouteFact::Known {
        value: vec!["model-api".to_string()],
        provenance: ModelRouteFactProvenance::Caller {
            source_ref: "caller/facts".into(),
            work_node_id: Uuid::new_v4(),
            work_node_revision: 1,
        },
    };
    assert_eq!(validate_host_fact(&caller), Err(Error::InputConflict));

    let snapshot = ModelRouteHostCapabilities {
        schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
        version: 3,
        capabilities: vec!["model-api".into()],
    };
    let mut forged = snapshot.fact().unwrap();
    if let ModelRouteFact::Known { value, .. } = &mut forged {
        value.push("unsupported".into());
    }
    assert_eq!(validate_host_fact(&forged), Err(Error::InputConflict));
}
