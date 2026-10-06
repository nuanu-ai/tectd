//! An agent's explicit Matrix decision, separate from optional ranked advice.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatrixDispositionBasis {
    AfterAdvice,
    NoCall,
    Manual,
}

impl MatrixDispositionBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AfterAdvice => "after_advice",
            Self::NoCall => "no_call",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatrixDispositionDecision {
    Selected { selected_choice_id: String },
    Blocked { blocked_reason: String },
}

impl MatrixDispositionDecision {
    pub fn validate(&self) -> Result<()> {
        let value = match self {
            Self::Selected { selected_choice_id } => selected_choice_id,
            Self::Blocked { blocked_reason } => blocked_reason,
        };
        if value.is_empty() || value.len() > 4096 || value.trim() != value || value.contains('\0') {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_and_blocked_reason_are_explicit() {
        assert!(
            MatrixDispositionDecision::Selected {
                selected_choice_id: "choice-a".into()
            }
            .validate()
            .is_ok()
        );
        assert!(
            MatrixDispositionDecision::Blocked {
                blocked_reason: "Evidence expired".into()
            }
            .validate()
            .is_ok()
        );
        assert_eq!(
            MatrixDispositionDecision::Blocked {
                blocked_reason: " ".into()
            }
            .validate(),
            Err(Error::InvalidArguments)
        );
        assert_eq!(
            MatrixDispositionDecision::Selected {
                selected_choice_id: "".into()
            }
            .validate(),
            Err(Error::InvalidArguments)
        );
    }
}
