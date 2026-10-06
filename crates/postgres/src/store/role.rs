use super::*;

pub(super) fn decode_principal_role(role: &str) -> Result<PrincipalRole> {
    match role {
        "owner" => Ok(PrincipalRole::Owner),
        "verifier" => Ok(PrincipalRole::Verifier),
        _ => Err(Error::Unauthorized),
    }
}

#[cfg(test)]
mod role_tests {
    use super::*;

    #[test]
    fn trusted_role_decode_accepts_only_exact_known_roles() {
        assert_eq!(
            decode_principal_role("owner").unwrap(),
            PrincipalRole::Owner
        );
        assert_eq!(
            decode_principal_role("verifier").unwrap(),
            PrincipalRole::Verifier
        );
        for role in [
            "", "Owner", "VERIFIER", " owner", "owner ", "admin", "unknown",
        ] {
            assert!(matches!(
                decode_principal_role(role),
                Err(Error::Unauthorized)
            ));
        }
    }

    #[test]
    fn verifier_migration_has_no_owner_default_and_returns_trusted_role() {
        let migration = include_str!("../../migrations/0048_verifier_principal.sql");
        assert!(migration.contains("CHECK (role IN ('owner', 'verifier'))"));
        assert!(!migration.contains("DEFAULT"));
        assert_eq!(
            migration.matches("p.role, h.allowed_source_roots").count(),
            2
        );
        assert!(migration.contains("p.tenant_id = h.tenant_id AND p.id = h.principal_id"));
        assert!(migration.contains("FOR SHARE OF h, p"));
        assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTION"));
    }
}
