//! The product level a Program is built to: one scale of four values.
use super::mode_catalog as catalog;
use serde::{Deserialize, Serialize};

/// Mandatory on every Program that is opened or edited while open. There is no
/// default level: the user states it or the agent records its own reading of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramMode {
    Demo,
    Mvp,
    Prod,
    SeriousProd,
}

impl ProgramMode {
    pub const ALL: [Self; 4] = [Self::Demo, Self::Mvp, Self::Prod, Self::SeriousProd];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Demo => "demo",
            Self::Mvp => "mvp",
            Self::Prod => "prod",
            Self::SeriousProd => "serious_prod",
        }
    }

    /// The level with this name; `None` for any other text.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.as_str() == name)
    }

    /// What the level means, in the catalog's wording.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Demo => catalog::DEMO,
            Self::Mvp => catalog::MVP,
            Self::Prod => catalog::PROD,
            Self::SeriousProd => catalog::SERIOUS_PROD,
        }
    }
}
