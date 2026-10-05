use super::*;

pub(super) fn validate_validator_receipts(
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    if let Some(index) = duplicate_index(
        output
            .validator_receipts
            .iter()
            .map(|receipt| (&receipt.resource_id, &receipt.stage)),
    ) {
        return Err(artifact_refusal(
            RefusalCode::InvalidOutput,
            "WP6-VALIDATOR-RECEIPT-DUPLICATE",
            format!("output.validator_receipts[{index}].resource_id"),
            "unique resource/stage receipt",
            "duplicate",
        ));
    }
    if let Some(index) = output.validator_receipts.iter().position(|receipt| {
        !phase.validator_contracts.iter().any(|contract| {
            receipt.resource_id == contract.resource_id && receipt.stage == contract.stage
        })
    }) {
        return Err(artifact_refusal(
            RefusalCode::InvalidOutput,
            "WP6-VALIDATOR-RECEIPT-UNKNOWN",
            format!("output.validator_receipts[{index}].resource_id"),
            "receipt for declared resource/stage contract",
            "unknown identity",
        ));
    }
    for (contract_index, contract) in phase.validator_contracts.iter().enumerate() {
        let receipt = output
            .validator_receipts
            .iter()
            .enumerate()
            .find(|(_, receipt)| {
                receipt.resource_id == contract.resource_id && receipt.stage == contract.stage
            });
        let verdict = output.verdict.as_ref();
        let required = contract.required_verdicts.is_empty()
            || verdict.is_some_and(|verdict| contract.required_verdicts.contains(verdict));
        if required && receipt.is_none() {
            return Err(artifact_refusal(
                RefusalCode::InvalidOutput,
                "WP6-VALIDATOR-RECEIPT-MISSING",
                "output.validator_receipts",
                format!("receipt for validator_contracts[{contract_index}]"),
                "missing",
            ));
        }
        let Some((index, receipt)) = receipt else {
            continue;
        };
        let path = format!("output.validator_receipts[{index}]");
        let fail = |rule, field: &str, expected: &str, actual: &str| {
            artifact_refusal(
                RefusalCode::InvalidOutput,
                rule,
                format!("{path}.{field}"),
                expected,
                actual,
            )
        };
        let expected = output
            .artifacts
            .iter()
            .filter(|artifact| {
                contract
                    .artifact_patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, &artifact.name))
            })
            .map(|artifact| (&artifact.name, &artifact.digest))
            .collect::<BTreeSet<_>>();
        let actual = receipt
            .artifacts
            .iter()
            .map(|artifact| (&artifact.name, &artifact.digest))
            .collect::<BTreeSet<_>>();
        let success = verdict.is_some_and(|verdict| contract.success_verdicts.contains(verdict));
        let not_run = receipt.command == "not_run";
        if receipt.version != contract.version {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-VERSION",
                "version",
                "contract version",
                "mismatched",
            ));
        }
        if receipt.digest != contract.digest {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DIGEST",
                "digest",
                "contract digest",
                "mismatched",
            ));
        }
        if receipt.command.trim().is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-COMMAND",
                "command",
                "nonblank validator command",
                "blank",
            ));
        }
        if !not_run {
            if let Some(artifact_index) = duplicate_index(
                receipt
                    .artifacts
                    .iter()
                    .map(|artifact| (&artifact.name, &artifact.digest)),
            ) {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-ARTIFACT-DUPLICATE",
                    &format!("artifacts[{artifact_index}]"),
                    "unique artifact name/digest tuple",
                    "duplicate",
                ));
            }
            if actual != expected {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-ARTIFACT-SET",
                    "artifacts",
                    "exact set of matching output artifact names and digests",
                    "mismatched",
                ));
            }
        }
        if not_run {
            if required {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-NOT-RUN-REQUIRED",
                    "command",
                    "executed validator when required",
                    "not_run",
                ));
            }
            if receipt.valid {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-NOT-RUN-VALID",
                    "valid",
                    "false for not_run",
                    "true",
                ));
            }
            if !receipt.artifacts.is_empty() {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-NOT-RUN-ARTIFACTS",
                    "artifacts",
                    "empty for not_run",
                    "nonempty",
                ));
            }
        }
        if success {
            if receipt.exit_code != 0 {
                return Err(artifact_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-VALIDATOR-RECEIPT-SUCCESS-EXIT",
                    format!("{path}.exit_code"),
                    "0 for success verdict",
                    receipt.exit_code.to_string(),
                ));
            }
            if !receipt.valid {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-SUCCESS-VALID",
                    "valid",
                    "true for success verdict",
                    "false",
                ));
            }
        }
    }
    Ok(())
}
