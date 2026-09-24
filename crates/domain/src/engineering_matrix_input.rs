//! Slice 02 factual inputs only. Source authority and policy are bound by a later caller.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_FACT_TEXT_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineeringMode {
    Demo,
    Mvp,
    Production,
}

/// A source reference, deliberately without a task, Program or workspace binding.
/// Its authority and freshness must be established outside this value model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactProvenance(pub String);

impl FactProvenance {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.0)
    }
}

/// Absence is not the same as an evidenced empty answer. Nor can missing,
/// contradictory or malformed material be silently normalized to an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MatrixFact<T> {
    Absent,
    KnownEmpty {
        provenance: FactProvenance,
    },
    Known {
        value: T,
        provenance: FactProvenance,
    },
    Unknown {
        provenance: FactProvenance,
    },
    Gap {
        provenance: FactProvenance,
    },
    Conflict {
        provenance: FactProvenance,
    },
    Invalid {
        provenance: FactProvenance,
    },
}

impl<T> MatrixFact<T> {
    pub fn validate(&self, validate_value: impl FnOnce(&T) -> Result<()>) -> Result<()> {
        match self {
            Self::Absent => Ok(()),
            Self::Known { value, provenance } => {
                provenance.validate()?;
                validate_value(value)
            }
            Self::KnownEmpty { provenance }
            | Self::Unknown { provenance }
            | Self::Gap { provenance }
            | Self::Conflict { provenance }
            | Self::Invalid { provenance } => provenance.validate(),
        }
    }
}

/// No numeric or named scale tiers are assumed by the input model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingEnvelope {
    pub scale: MatrixFact<String>,
    pub operational_facts: Vec<OperatingFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingFact {
    pub name: String,
    pub fact: MatrixFact<String>,
}

impl OperatingEnvelope {
    pub fn validate(&self) -> Result<()> {
        self.scale.validate(|value| validate_text(value))?;
        let mut names = BTreeSet::new();
        for fact in &self.operational_facts {
            validate_text(&fact.name)?;
            if !names.insert(&fact.name) {
                return Err(Error::InvalidArguments);
            }
            fact.fact.validate(|value| validate_text(value))?;
        }
        Ok(())
    }
}

/// Hotfix is explicit; other intent stays source-authored rather than a
/// prematurely frozen taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "description", rename_all = "snake_case")]
pub enum EngineeringIntent {
    ProductionHotfix,
    Other(String),
}

impl EngineeringIntent {
    fn validate(&self) -> Result<()> {
        match self {
            Self::ProductionHotfix => Ok(()),
            Self::Other(description) => validate_text(description),
        }
    }
}

/// A possibly incomplete source projection. Validation checks shape, not
/// applicability, source authority, mandatory duties or execution eligibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringMatrixInput {
    pub mode: MatrixFact<EngineeringMode>,
    pub envelope: OperatingEnvelope,
    pub criticality: MatrixFact<String>,
    pub intent: MatrixFact<EngineeringIntent>,
    pub urgency: MatrixFact<String>,
}

impl EngineeringMatrixInput {
    pub fn validate(&self) -> Result<()> {
        self.mode.validate(|_| Ok(()))?;
        self.envelope.validate()?;
        self.criticality.validate(|value| validate_text(value))?;
        self.intent.validate(EngineeringIntent::validate)?;
        self.urgency.validate(|value| validate_text(value))?;
        Ok(())
    }
}

fn validate_text(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_FACT_TEXT_BYTES {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> FactProvenance {
        FactProvenance("source-revision-1".into())
    }

    fn known<T>(value: T) -> MatrixFact<T> {
        MatrixFact::Known {
            value,
            provenance: source(),
        }
    }

    fn input() -> EngineeringMatrixInput {
        EngineeringMatrixInput {
            mode: known(EngineeringMode::Production),
            envelope: OperatingEnvelope {
                scale: known("observed demand and provider capacity".into()),
                operational_facts: vec![OperatingFact {
                    name: "users".into(),
                    fact: known("active users".into()),
                }],
            },
            criticality: known("payments affected".into()),
            intent: known(EngineeringIntent::ProductionHotfix),
            urgency: known("urgent repair".into()),
        }
    }

    #[test]
    fn mode_and_hotfix_inputs_round_trip() {
        for mode in [
            EngineeringMode::Demo,
            EngineeringMode::Mvp,
            EngineeringMode::Production,
        ] {
            let mut value = input();
            value.mode = known(mode);
            value.validate().unwrap();
            let encoded = serde_json::to_string(&value).unwrap();
            let decoded: EngineeringMatrixInput = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn all_fact_states_remain_distinct_in_json() {
        let facts: Vec<MatrixFact<String>> = vec![
            MatrixFact::Absent,
            MatrixFact::KnownEmpty {
                provenance: source(),
            },
            known("observed".into()),
            MatrixFact::Unknown {
                provenance: source(),
            },
            MatrixFact::Gap {
                provenance: source(),
            },
            MatrixFact::Conflict {
                provenance: source(),
            },
            MatrixFact::Invalid {
                provenance: source(),
            },
        ];
        for fact in &facts {
            fact.validate(|value| validate_text(value)).unwrap();
        }
        let encoded = serde_json::to_string(&facts).unwrap();
        let decoded: Vec<MatrixFact<String>> = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, facts);
        assert_eq!(encoded.matches("\"state\":").count(), facts.len());
    }

    #[test]
    fn rejects_empty_provenance_value_and_duplicate_operating_fact() {
        let mut value = input();
        value.mode = MatrixFact::KnownEmpty {
            provenance: FactProvenance(" ".into()),
        };
        assert_eq!(value.validate(), Err(Error::InvalidArguments));
        let mut value = input();
        value.criticality = known(" ".into());
        assert_eq!(value.validate(), Err(Error::InvalidArguments));
        let mut value = input();
        value
            .envelope
            .operational_facts
            .push(value.envelope.operational_facts[0].clone());
        assert_eq!(value.validate(), Err(Error::InvalidArguments));
    }

    #[test]
    fn missing_and_uncertain_inputs_are_preserved() {
        let mut value = input();
        value.mode = MatrixFact::Absent;
        value.envelope.scale = MatrixFact::Conflict {
            provenance: source(),
        };
        value.urgency = MatrixFact::Unknown {
            provenance: source(),
        };
        value.validate().unwrap();
        assert!(matches!(value.mode, MatrixFact::Absent));
        assert!(matches!(value.envelope.scale, MatrixFact::Conflict { .. }));
        assert!(matches!(value.urgency, MatrixFact::Unknown { .. }));
    }
}
