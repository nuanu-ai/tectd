use super::*;

pub(crate) const CANDIDATE_MAX_KEYS: usize = 64;
pub(crate) const CANDIDATE_MAX_ROWS: usize = 8192;
pub(crate) const CANDIDATE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Private optimization eligibility. Converting back preserves public errors.
#[derive(Debug)]
pub(crate) enum CandidateProofError {
    Refusal(Error),
    Terminal(Error),
}
impl CandidateProofError {
    pub(crate) fn refusal(error: Error) -> Self {
        match error {
            Error::InternalInvariant | Error::KnowledgePayloadErased | Error::InvalidArguments => {
                Self::Refusal(error)
            }
            _ => Self::Terminal(error),
        }
    }
    /// Called only at pure proof codec/identity/digest validation sites. A
    /// serde codec error historically maps to StorageUnavailable, but is not IO.
    pub(crate) fn proof_refusal(error: Error) -> Self {
        match error {
            Error::StorageUnavailable => Self::Refusal(error),
            _ => Self::refusal(error),
        }
    }
    pub(crate) fn public(self) -> Error {
        match self {
            Self::Refusal(e) | Self::Terminal(e) => e,
        }
    }
    pub(crate) fn storage(error: sqlx::Error) -> Self {
        Self::Terminal(storage_error(error))
    }
}
impl From<Error> for CandidateProofError {
    // Unclassified application errors NEVER authorize a fallback.
    fn from(error: Error) -> Self {
        Self::Terminal(error)
    }
}

#[derive(Default)]
pub(crate) struct CandidateBudget {
    keys: usize,
    expected_rows: usize,
    bytes: usize,
    pub(crate) returned_rows: usize,
}
impl CandidateBudget {
    pub(crate) fn reserve_keys(
        &mut self,
        keys: usize,
    ) -> std::result::Result<(), CandidateProofError> {
        self.keys = self
            .keys
            .checked_add(keys)
            .ok_or_else(|| CandidateProofError::refusal(Error::InternalInvariant))?;
        if self.keys > CANDIDATE_MAX_KEYS {
            count("proof.candidate_cap_keys", 1);
            return Err(CandidateProofError::refusal(Error::InternalInvariant));
        }
        Ok(())
    }
    pub(crate) fn reserve_bytes(
        &mut self,
        bytes: usize,
    ) -> std::result::Result<(), CandidateProofError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| CandidateProofError::refusal(Error::InternalInvariant))?;
        if self.bytes > CANDIDATE_MAX_BYTES {
            count("proof.candidate_cap_bytes", 1);
            return Err(CandidateProofError::refusal(Error::InternalInvariant));
        }
        Ok(())
    }
    pub(crate) fn reserve_expected_rows(
        &mut self,
        rows: usize,
    ) -> std::result::Result<(), CandidateProofError> {
        self.expected_rows = self
            .expected_rows
            .checked_add(rows)
            .ok_or_else(|| CandidateProofError::refusal(Error::InternalInvariant))?;
        if self.expected_rows > CANDIDATE_MAX_ROWS {
            count("proof.candidate_cap_rows", 1);
            return Err(CandidateProofError::refusal(Error::InternalInvariant));
        }
        Ok(())
    }
    pub(crate) fn record(&self) {
        count("proof.candidate_keys", self.keys);
        count("proof.candidate_expected_rows", self.expected_rows);
        count("proof.candidate_returned_rows", self.returned_rows);
        count("proof.candidate_bytes", self.bytes);
    }
    pub(crate) fn remaining_rows(&self) -> usize {
        CANDIDATE_MAX_ROWS - self.returned_rows
    }
    pub(crate) fn remaining_bytes(&self) -> usize {
        CANDIDATE_MAX_BYTES - self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_caps_are_aggregate_across_passes() {
        let mut b = CandidateBudget::default();
        b.reserve_keys(32).unwrap();
        b.reserve_keys(32).unwrap();
        assert!(matches!(
            b.reserve_keys(1),
            Err(CandidateProofError::Refusal(_))
        ));
        let mut b = CandidateBudget::default();
        b.reserve_expected_rows(4096).unwrap();
        b.reserve_expected_rows(4096).unwrap();
        assert!(b.reserve_expected_rows(1).is_err());
        let mut b = CandidateBudget::default();
        b.reserve_bytes(CANDIDATE_MAX_BYTES - 1).unwrap();
        b.reserve_bytes(1).unwrap();
        assert!(b.reserve_bytes(1).is_err());
    }
    #[test]
    fn unclassified_errors_are_terminal_with_original_public_mapping() {
        for error in [
            Error::InternalInvariant,
            Error::StorageUnavailable,
            Error::OperationTimeout,
        ] {
            assert!(matches!(
                CandidateProofError::from(error),
                CandidateProofError::Terminal(_)
            ));
        }
        assert!(matches!(
            CandidateProofError::refusal(Error::StorageUnavailable),
            CandidateProofError::Terminal(_)
        ));
        assert!(matches!(
            CandidateProofError::proof_refusal(Error::OperationTimeout),
            CandidateProofError::Terminal(_)
        ));
        assert_eq!(
            CandidateProofError::proof_refusal(Error::StorageUnavailable).public(),
            Error::StorageUnavailable
        );
    }
}
