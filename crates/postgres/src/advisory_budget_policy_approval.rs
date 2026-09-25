//! Pure owner-approval verification; integration chooses the pinned owner key.
use ring::signature::{ED25519, UnparsedPublicKey};
use tect_domain::{AdvisoryBudgetPolicy, Error, Result};
use uuid::Uuid;

/// Verify a stored candidate against the expected workspace, owner and pinned
/// 32-byte Ed25519 public key. No key discovery or fallback trust occurs here.
pub fn verify_budget_policy_approval(
    workspace_id: Uuid,
    expected_owner: Uuid,
    policy: &AdvisoryBudgetPolicy,
    owner_public_key: &[u8],
) -> Result<()> {
    if expected_owner.is_nil()
        || policy.approved_by() != expected_owner
        || owner_public_key.len() != 32
    {
        return Err(Error::Forbidden);
    }
    let message = policy.approval_signing_message(workspace_id)?;
    let signature_hex = policy.approval_signature();
    let mut signature = [0u8; 64];
    for (index, byte) in signature.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&signature_hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Forbidden)?;
    }
    UnparsedPublicKey::new(&ED25519, owner_public_key)
        .verify(&message, &signature)
        .map_err(|_| Error::Forbidden)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tect_domain::AdvisoryBudgetCeilings;

    const WORKSPACE: &str = "7a4cd6a1-ea2e-4e34-a854-fa4a86ae2517";
    const OWNER: &str = "a7d6c875-4fb4-42bd-b972-b5a36c35618e";

    fn ceilings() -> AdvisoryBudgetCeilings {
        AdvisoryBudgetCeilings {
            provider_calls: 2,
            input_tokens: 3,
            output_tokens: 4,
            request_utf8_bytes: 5,
            elapsed_monotonic_ms: 6,
            retry_dispatches: 1,
        }
    }

    fn policy_with_signature(
        owner: Uuid,
        limits: AdvisoryBudgetCeilings,
        signature: String,
    ) -> AdvisoryBudgetPolicy {
        let id = Uuid::parse_str("7806dcac-e14e-47b9-a18e-d0cc4cbb6bb8").unwrap();
        AdvisoryBudgetPolicy::new(
            id,
            1,
            AdvisoryBudgetPolicy::digest_for(id, 1, 100, 200, limits),
            100,
            200,
            limits,
            owner,
            signature,
        )
        .unwrap()
    }

    #[test]
    fn verifies_only_exact_pinned_key_workspace_owner_and_policy() {
        let workspace = Uuid::parse_str(WORKSPACE).unwrap();
        let owner = Uuid::parse_str(OWNER).unwrap();
        let keypair = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
        let unsigned = policy_with_signature(owner, ceilings(), "0".repeat(128));
        let signature = keypair.sign(&unsigned.approval_signing_message(workspace).unwrap());
        let signed = policy_with_signature(owner, ceilings(), format_hex(signature.as_ref()));
        let public_key = keypair.public_key().as_ref();

        assert!(verify_budget_policy_approval(workspace, owner, &signed, public_key).is_ok());
        assert!(verify_budget_policy_approval(Uuid::new_v4(), owner, &signed, public_key).is_err());
        assert!(verify_budget_policy_approval(Uuid::nil(), owner, &signed, public_key).is_err());
        assert!(
            verify_budget_policy_approval(workspace, Uuid::new_v4(), &signed, public_key).is_err()
        );
        assert!(
            verify_budget_policy_approval(workspace, Uuid::nil(), &signed, public_key).is_err()
        );
        assert!(verify_budget_policy_approval(workspace, owner, &signed, &[0u8; 32]).is_err());
        assert!(verify_budget_policy_approval(workspace, owner, &signed, &[0u8; 31]).is_err());

        let other_key = Ed25519KeyPair::from_seed_unchecked(&[8u8; 32]).unwrap();
        assert!(
            verify_budget_policy_approval(
                workspace,
                owner,
                &signed,
                other_key.public_key().as_ref()
            )
            .is_err()
        );
        let altered = policy_with_signature(
            owner,
            AdvisoryBudgetCeilings {
                provider_calls: 3,
                ..ceilings()
            },
            signed.approval_signature().to_owned(),
        );
        assert_ne!(altered.digest(), signed.digest());
        assert!(verify_budget_policy_approval(workspace, owner, &altered, public_key).is_err());
        let wrong_owner = policy_with_signature(
            Uuid::new_v4(),
            ceilings(),
            signed.approval_signature().to_owned(),
        );
        assert!(
            verify_budget_policy_approval(
                workspace,
                wrong_owner.approved_by(),
                &wrong_owner,
                public_key
            )
            .is_err()
        );
        let bad_signature = policy_with_signature(owner, ceilings(), "0".repeat(128));
        assert!(
            verify_budget_policy_approval(workspace, owner, &bad_signature, public_key).is_err()
        );
    }

    #[test]
    fn malformed_digest_and_signature_never_make_a_policy_candidate() {
        let owner = Uuid::parse_str(OWNER).unwrap();
        let id = Uuid::new_v4();
        let digest = AdvisoryBudgetPolicy::digest_for(id, 1, 100, 200, ceilings());
        for bad_digest in ["0".repeat(64), digest.to_uppercase()] {
            assert!(
                AdvisoryBudgetPolicy::new(
                    id,
                    1,
                    bad_digest,
                    100,
                    200,
                    ceilings(),
                    owner,
                    "0".repeat(128)
                )
                .is_err()
            );
        }
        for bad_signature in ["0".repeat(127), "g".repeat(128)] {
            assert!(
                AdvisoryBudgetPolicy::new(
                    id,
                    1,
                    digest.clone(),
                    100,
                    200,
                    ceilings(),
                    owner,
                    bad_signature
                )
                .is_err()
            );
        }
    }

    fn format_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
