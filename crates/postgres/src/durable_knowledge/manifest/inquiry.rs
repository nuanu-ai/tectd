use super::*;

#[derive(Clone)]
pub(super) struct Projection {
    pub inquiry: PipelineInquiryContract,
    pub policy: PipelineKnowledgeProjectionPolicy,
}

pub(super) enum BriefSelection {
    Full,
    Omit {
        needs_context: bool,
    },
    Briefs {
        values: Vec<PlanningBrief>,
        needs_context: bool,
    },
}

pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
) -> Result<Option<Projection>> {
    let value: Option<Option<serde_json::Value>> = sqlx::query_scalar(
        "SELECT inquiry FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    value
        .flatten()
        .map(|value| {
            let inquiry: PipelineInquiryContract = decode(value)?;
            inquiry.validate()?;
            let policy = match inquiry.topic_level {
                PipelineInquiryTopicLevel::Program => {
                    PipelineKnowledgeProjectionPolicy::ProgramPlanningBriefs
                }
                PipelineInquiryTopicLevel::Scope => {
                    PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs
                }
                PipelineInquiryTopicLevel::Slice => {
                    PipelineKnowledgeProjectionPolicy::FullResources
                }
            };
            Ok(Projection { inquiry, policy })
        })
        .transpose()
}

impl Projection {
    pub(super) fn policy_name(&self) -> &'static str {
        match self.policy {
            PipelineKnowledgeProjectionPolicy::FullResources => "full_resources",
            PipelineKnowledgeProjectionPolicy::ProgramPlanningBriefs => "program_planning_briefs",
            PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs => "scope_planning_briefs",
        }
    }

    pub(super) fn stage(&self) -> Option<PlanningStage> {
        match self.policy {
            PipelineKnowledgeProjectionPolicy::FullResources => None,
            PipelineKnowledgeProjectionPolicy::ProgramPlanningBriefs => {
                Some(PlanningStage::Program)
            }
            PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs => Some(PlanningStage::Scope),
        }
    }

    pub(super) fn allows_binding(&self, kind: &str) -> bool {
        match self.policy {
            PipelineKnowledgeProjectionPolicy::FullResources => true,
            PipelineKnowledgeProjectionPolicy::ProgramPlanningBriefs => {
                matches!(kind, "workspace" | "program")
            }
            PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs => {
                matches!(kind, "workspace" | "program" | "scope")
            }
        }
    }

    pub(super) fn select(
        &self,
        document: &KnowledgeDocumentDraft,
        purpose: KnowledgeBindingPurpose,
    ) -> BriefSelection {
        self.select_briefs(&document.planning_briefs, purpose)
    }

    pub(super) fn select_briefs(
        &self,
        briefs: &[PlanningBrief],
        purpose: KnowledgeBindingPurpose,
    ) -> BriefSelection {
        let Some(stage) = self.stage() else {
            return BriefSelection::Full;
        };
        let mut values = Vec::new();
        let mut needs_context = false;
        for brief in briefs.iter().filter(|brief| brief.stage == stage) {
            match crate::planning_knowledge::applicable(brief, &self.inquiry.task_context) {
                Ok(true) => values.push(brief.clone()),
                Ok(false) => {}
                Err(()) if purpose != KnowledgeBindingPurpose::Reference => {
                    needs_context = true;
                }
                Err(()) => {}
            }
        }
        if values.is_empty() {
            BriefSelection::Omit { needs_context }
        } else {
            BriefSelection::Briefs {
                values,
                needs_context,
            }
        }
    }
}

pub(super) fn projected_text(values: &[PlanningBrief]) -> String {
    values
        .iter()
        .map(|brief| brief.instruction.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn projected_conditions(values: &[PlanningBrief]) -> Vec<String> {
    values
        .iter()
        .flat_map(|brief| brief.conditions.iter().cloned())
        .collect()
}

pub(super) fn projected_exceptions(values: &[PlanningBrief]) -> Vec<String> {
    values
        .iter()
        .flat_map(|brief| brief.exceptions.iter().cloned())
        .collect()
}

pub(super) fn projected_targets(values: &[PlanningBrief]) -> Vec<String> {
    let mut targets = values
        .iter()
        .flat_map(|brief| brief.selectors.target_iris.iter().cloned())
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection(
        topic_level: PipelineInquiryTopicLevel,
        context: PlanningTaskContext,
    ) -> Projection {
        let policy = match topic_level {
            PipelineInquiryTopicLevel::Program => {
                PipelineKnowledgeProjectionPolicy::ProgramPlanningBriefs
            }
            PipelineInquiryTopicLevel::Scope => {
                PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs
            }
            PipelineInquiryTopicLevel::Slice => PipelineKnowledgeProjectionPolicy::FullResources,
        };
        Projection {
            inquiry: PipelineInquiryContract {
                topic_level,
                task_context: context,
                completion: PipelineInquiryCompletion::Research {
                    allow_inconclusive: false,
                },
            },
            policy,
        }
    }

    fn document() -> KnowledgeDocumentDraft {
        KnowledgeDocumentDraft {
            title: "detailed title".into(),
            canonical_text: "detailed canonical text".into(),
            knowledge_kind: KnowledgeKind::Constraint,
            epistemic_state: KnowledgeEpistemicState::Normative,
            target_iris: vec!["urn:fixture:detailed".into()],
            conditions: vec!["detailed condition".into()],
            exceptions: vec!["detailed exception".into()],
            sources: vec![],
            bindings: vec![],
            profiles: vec![KnowledgeProfileId::General],
            access_scope: KnowledgeAccessScope::WorkspaceMembers,
            owner_ref: "owner".into(),
            authority_basis: "authority".into(),
            planning_briefs: vec![
                PlanningBrief {
                    local_id: "program".into(),
                    stage: PlanningStage::Program,
                    instruction: "brief instruction".into(),
                    conditions: vec!["brief condition".into()],
                    exceptions: vec!["brief exception".into()],
                    purpose: "purpose".into(),
                    selectors: PlanningBriefSelectors {
                        target_iris: vec!["urn:fixture:r1".into()],
                        environment_iris: vec!["urn:fixture:prod".into()],
                        ..Default::default()
                    },
                },
                PlanningBrief {
                    local_id: "scope".into(),
                    stage: PlanningStage::Scope,
                    instruction: "scope brief instruction".into(),
                    conditions: vec!["scope brief condition".into()],
                    exceptions: vec!["scope brief exception".into()],
                    purpose: "scope purpose".into(),
                    selectors: PlanningBriefSelectors::default(),
                },
            ],
            valid_from: None,
            valid_until: None,
            review_due_at: None,
            sections: KnowledgeProfileSections::default(),
        }
    }

    #[test]
    fn known_empty_mismatch_omits_and_unknown_requires_context() {
        let empty = projection(
            PipelineInquiryTopicLevel::Program,
            PlanningTaskContext {
                target_iris: Some(vec![]),
                ..Default::default()
            },
        );
        assert!(matches!(
            empty.select(&document(), KnowledgeBindingPurpose::Required),
            BriefSelection::Omit {
                needs_context: false
            }
        ));

        let unknown = projection(
            PipelineInquiryTopicLevel::Program,
            PlanningTaskContext::default(),
        );
        assert!(matches!(
            unknown.select(&document(), KnowledgeBindingPurpose::Required),
            BriefSelection::Omit {
                needs_context: true
            }
        ));
    }

    #[test]
    fn known_mismatch_wins_over_other_unknown_dimension() {
        let value = projection(
            PipelineInquiryTopicLevel::Program,
            PlanningTaskContext {
                target_iris: None,
                environment_iris: Some(vec!["urn:fixture:staging".into()]),
                action_classes: None,
            },
        );
        assert!(matches!(
            value.select(&document(), KnowledgeBindingPurpose::Required),
            BriefSelection::Omit {
                needs_context: false
            }
        ));
    }

    #[test]
    fn topic_level_selects_exact_brief_or_full_resource_policy() {
        let program = projection(
            PipelineInquiryTopicLevel::Program,
            PlanningTaskContext {
                target_iris: Some(vec!["urn:fixture:r1".into()]),
                environment_iris: Some(vec!["urn:fixture:prod".into()]),
                action_classes: Some(vec![]),
            },
        );
        let BriefSelection::Briefs {
            values,
            needs_context,
        } = program.select(&document(), KnowledgeBindingPurpose::Required)
        else {
            panic!("program brief was not selected")
        };
        assert!(!needs_context);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].local_id, "program");
        assert_eq!(projected_text(&values), "brief instruction");
        assert_eq!(projected_conditions(&values), ["brief condition"]);
        assert_eq!(projected_exceptions(&values), ["brief exception"]);
        assert!(!projected_text(&values).contains("detailed"));

        let scope = projection(
            PipelineInquiryTopicLevel::Scope,
            PlanningTaskContext::default(),
        );
        let BriefSelection::Briefs { values, .. } =
            scope.select(&document(), KnowledgeBindingPurpose::Required)
        else {
            panic!("scope brief was not selected")
        };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].local_id, "scope");

        let slice = projection(
            PipelineInquiryTopicLevel::Slice,
            PlanningTaskContext::default(),
        );
        assert!(matches!(
            slice.select(&document(), KnowledgeBindingPurpose::Required),
            BriefSelection::Full
        ));
        assert!(program.allows_binding("program"));
        assert!(!program.allows_binding("scope"));
        assert!(scope.allows_binding("scope"));
        assert!(!scope.allows_binding("slice"));
        assert!(slice.allows_binding("slice_phase"));
    }
}
