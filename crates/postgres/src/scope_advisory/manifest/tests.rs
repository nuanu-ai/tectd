#[cfg(test)]
mod authored_graph_binding_tests {
    use super::*;
    use tect_domain::{CandidateGrounding, ExploratoryProvenance};

    fn manifest(sources: &[Uuid]) -> ScopeConstructorManifest {
        let fragments = sources
            .iter()
            .map(|id| (*id, crate::scope_advisory::live_support::D))
            .collect::<Vec<_>>();
        let mut value = crate::scope_advisory::live_support::manifest(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            &fragments,
        );
        value.constructor = source_authored_identity();
        crate::scope_advisory::live_support::reseal_manifest(&mut value);
        value
    }

    #[test]
    fn binding_derives_exact_source_goal_and_stable_dependency_graph() {
        let source = Uuid::new_v4();
        let value = manifest(&[source]);
        let (links, digest, provenance) = authored_graph_binding(&value, &[]).unwrap();
        assert_eq!(
            links,
            vec![AntiBloatObligationLink {
                obligation_id: source.to_string(),
                goal_id: value.emitted[0].material.goals[0].id,
            }]
        );
        assert_eq!(digest.len(), 64);
        assert!(provenance.contains(&value.emitted[0].material_digest));
        assert_eq!(
            authored_graph_binding(&value, &[]).unwrap(),
            (links, digest.clone(), provenance)
        );

        let mut changed = value.clone();
        changed.emitted[0].material.candidates[0]
            .dependencies
            .push(Uuid::new_v4());
        assert_ne!(authored_graph_binding(&changed, &[]).unwrap().1, digest);
    }

    #[test]
    fn binding_refuses_missing_unmatched_and_duplicate_obligations() {
        let source = Uuid::from_u128(1);
        let metadata = Uuid::from_u128(2);
        let mut value = manifest(&[source, metadata]);
        assert_eq!(
            authored_graph_binding(&value, &[]).unwrap_err(),
            Error::InvalidSource
        );
        assert_eq!(
            authored_graph_binding(&value, &[metadata.to_string()])
                .unwrap()
                .0
                .len(),
            1
        );
        assert_eq!(
            authored_graph_binding(&value, &[source.to_string()]).unwrap_err(),
            Error::InvalidSource
        );
        assert_eq!(
            authored_graph_binding(&value, &[metadata.to_string(), metadata.to_string()])
                .unwrap_err(),
            Error::InvalidSource
        );

        value = manifest(&[source]);
        value.emitted[0].material.goals[0].source_ref_id = Uuid::new_v4();
        assert_eq!(
            authored_graph_binding(&value, &[]).unwrap_err(),
            Error::InvalidSource
        );

        value = manifest(&[source]);
        value.obligations.push(value.obligations[0].clone());
        assert_eq!(
            authored_graph_binding(&value, &[]).unwrap_err(),
            Error::InvalidSource
        );
    }

    #[test]
    fn native_source_kind_partition_is_boundary_specific_and_fail_closed() {
        use tect_domain::CandidateBoundary::{Finite, Ongoing};
        assert_eq!(
            source_ref_can_anchor_goal(Ongoing, "planning_input"),
            Ok(true)
        );
        assert_eq!(
            source_ref_can_anchor_goal(Ongoing, "program_success"),
            Ok(false)
        );
        assert_eq!(
            source_ref_can_anchor_goal(Finite, "program_success"),
            Ok(true)
        );
        assert_eq!(
            source_ref_can_anchor_goal(Finite, "planning_input"),
            Ok(false)
        );
        assert_eq!(
            source_ref_can_anchor_goal(Finite, "program_field"),
            Ok(false)
        );
        assert_eq!(
            source_ref_can_anchor_goal(Ongoing, "forged_kind"),
            Err(Error::InvalidSource)
        );
    }

    #[test]
    fn v2_exploratory_mechanism_preserves_source_obligations_and_v1_refuses_it() {
        let source = Uuid::new_v4();
        let mut value = manifest(&[source]);
        let original = authored_graph_binding(&value, &[]).unwrap().0;
        let mut exploratory = value.emitted[0].material.candidates[0].clone();
        exploratory.id = Uuid::new_v4();
        exploratory.grounding = CandidateGrounding::ExploratoryUnrequested {
            provenance: ExploratoryProvenance::SourceAuthoredV2,
        };
        exploratory.coverage_goal_ids.clear();
        value.emitted[0]
            .material
            .delta
            .added
            .push(tect_domain::CandidateAdded {
                candidate_id: exploratory.id,
                revision: exploratory.revision,
            });
        value.emitted[0].material.candidates.push(exploratory);
        assert!(
            value.emitted[0].material.validate().is_ok(),
            "{:?}",
            value.emitted[0].material.validate()
        );
        assert_eq!(authored_graph_binding(&value, &[]).unwrap().0, original);
        value.constructor = legacy_source_authored_identity();
        assert_eq!(
            authored_graph_binding(&value, &[]).unwrap_err(),
            Error::InvalidSource
        );
    }
}
