use super::{
    AdvisoryDispatchSeal, AdvisorySendCertainty, Error, Result, Uuid,
    validate_non_secret_identifier,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisoryReconciliationEvidence {
    Inconclusive {
        dispatch_id: Uuid,
    },
    ConfirmedSent(AdvisoryDispatchSeal),
    ConfirmedNotSent {
        dispatch_id: Uuid,
        evidence_ref: String,
    },
}

impl AdvisoryReconciliationEvidence {
    pub fn dispatch_id(&self) -> Uuid {
        match self {
            Self::Inconclusive { dispatch_id } | Self::ConfirmedNotSent { dispatch_id, .. } => {
                *dispatch_id
            }
            Self::ConfirmedSent(seal) => seal.dispatch_id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.dispatch_id().is_nil() {
            return Err(Error::InvalidArguments);
        }
        match self {
            Self::Inconclusive { .. } => Ok(()),
            Self::ConfirmedSent(seal) => {
                seal.validate()?;
                if seal.send_certainty != AdvisorySendCertainty::Sent {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            Self::ConfirmedNotSent { evidence_ref, .. } => {
                validate_non_secret_identifier(evidence_ref)
            }
        }
    }
}
