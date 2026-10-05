use super::*;

impl RecordPipelineInput {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil() {
            return Err(schema_refusal(
                "WP6-INPUT-REQUEST-ID",
                "arguments.params.request_id",
                "non-nil request UUID",
                "nil UUID",
            ));
        }
        if self.run_id.is_nil() {
            return Err(schema_refusal(
                "WP6-INPUT-RUN-ID",
                "arguments.params.run_id",
                "non-nil run UUID",
                "nil UUID",
            ));
        }
        if self.run_revision < 1 {
            return Err(schema_refusal(
                "WP6-INPUT-REVISION",
                "arguments.params.run_revision",
                "positive run revision",
                "nonpositive",
            ));
        }
        if self.phase_id.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-INPUT-PHASE-ID",
                "arguments.params.phase_id",
                "nonblank phase ID",
                "blank",
            ));
        }
        if self.input.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-INPUT-BODY",
                "arguments.params.input",
                "nonblank input",
                "blank",
            ));
        }
        if self.input.len() > MAX_PIPELINE_INPUT_BYTES {
            return Err(schema_refusal(
                "WP6-INPUT-SIZE",
                "arguments.params.input",
                "at most 65536 UTF-8 bytes",
                format!("bytes={}", self.input.len()),
            ));
        }
        if let Some(amendment) = &self.source_amendment {
            validate_source_amendment(amendment)?;
        }
        Ok(())
    }
}

fn valid_relative_source_path(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_SOURCE_PATH_BYTES
        && !value.starts_with('/')
        && !value.contains(['\\', '\0'])
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

fn valid_media_type(value: &str) -> bool {
    value.trim() == value
        && value.len() <= 255
        && value.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                        )
                })
        })
}

pub(super) fn validate_source_amendment(amendment: &PipelineSourceAmendment) -> Result<()> {
    let successor = &amendment.successor;
    let artifact = &successor.artifact;
    if amendment.target_phase_id.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-TARGET-PHASE",
            "arguments.params.source_amendment.target_phase_id",
            "nonblank target phase",
            "blank",
        ));
    }
    if amendment.predecessor.output_id.is_nil() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-OUTPUT-ID",
            "arguments.params.source_amendment.predecessor.output_id",
            "non-nil output UUID",
            "nil UUID",
        ));
    }
    if amendment.predecessor.output_revision < 1 {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-REVISION",
            "arguments.params.source_amendment.predecessor.output_revision",
            "positive output revision",
            "nonpositive",
        ));
    }
    if amendment.predecessor.output_digest.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-OUTPUT-DIGEST-BLANK",
            "arguments.params.source_amendment.predecessor.output_digest",
            "nonblank predecessor output_digest",
            "blank",
        ));
    }
    if amendment.predecessor.output_digest.len() > 128 {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-OUTPUT-DIGEST-LIMIT",
            "arguments.params.source_amendment.predecessor.output_digest",
            "at most 128 UTF-8 bytes",
            format!("bytes={}", amendment.predecessor.output_digest.len()),
        ));
    }
    if amendment.predecessor.artifact_name.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-ARTIFACT-NAME-BLANK",
            "arguments.params.source_amendment.predecessor.artifact_name",
            "nonblank predecessor artifact_name",
            "blank",
        ));
    }
    if amendment.predecessor.artifact_name.len() > MAX_SOURCE_PATH_BYTES {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-ARTIFACT-NAME-LIMIT",
            "arguments.params.source_amendment.predecessor.artifact_name",
            "at most 4096 UTF-8 bytes",
            format!("bytes={}", amendment.predecessor.artifact_name.len()),
        ));
    }
    if amendment.predecessor.artifact_digest.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-ARTIFACT-DIGEST-BLANK",
            "arguments.params.source_amendment.predecessor.artifact_digest",
            "nonblank predecessor artifact_digest",
            "blank",
        ));
    }
    if amendment.predecessor.artifact_digest.len() > 128 {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-ARTIFACT-DIGEST-LIMIT",
            "arguments.params.source_amendment.predecessor.artifact_digest",
            "at most 128 UTF-8 bytes",
            format!("bytes={}", amendment.predecessor.artifact_digest.len()),
        ));
    }
    if !valid_relative_source_path(&amendment.predecessor.source_path) {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-PATH",
            "arguments.params.source_amendment.predecessor.source_path",
            "safe bounded repository-relative path",
            "invalid path",
        ));
    }
    if amendment.predecessor.source_digest.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-SOURCE-DIGEST-BLANK",
            "arguments.params.source_amendment.predecessor.source_digest",
            "nonblank predecessor source_digest",
            "blank",
        ));
    }
    if amendment.predecessor.source_digest.len() > 128 {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PREDECESSOR-SOURCE-DIGEST-LIMIT",
            "arguments.params.source_amendment.predecessor.source_digest",
            "at most 128 UTF-8 bytes",
            format!("bytes={}", amendment.predecessor.source_digest.len()),
        ));
    }
    if !valid_relative_source_path(&successor.path) {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-SUCCESSOR-PATH",
            "arguments.params.source_amendment.successor.path",
            "safe bounded repository-relative path",
            "invalid path",
        ));
    }
    if !valid_relative_source_path(&artifact.name) {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-ARTIFACT-PATH",
            "arguments.params.source_amendment.successor.artifact.name",
            "safe bounded repository-relative artifact name",
            "invalid path",
        ));
    }
    if successor.path != artifact.name {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-PATH-NAME-MATCH",
            "arguments.params.source_amendment.successor.artifact.name",
            "name matching successor path",
            "mismatched",
        ));
    }
    if !valid_media_type(&artifact.media_type) {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-MEDIA",
            "arguments.params.source_amendment.successor.artifact.media_type",
            "valid bounded media type",
            "invalid media type",
        ));
    }
    if artifact.body.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-BODY-BLANK",
            "arguments.params.source_amendment.successor.artifact.body",
            "nonblank artifact body",
            "blank",
        ));
    }
    if artifact.body.len() > MAX_PIPELINE_OUTPUT_BYTES {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-BODY-SIZE",
            "arguments.params.source_amendment.successor.artifact.body",
            "at most 2097152 UTF-8 bytes",
            format!("bytes={}", artifact.body.len()),
        ));
    }
    if artifact.digest.len() != 64 {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-DIGEST-SIZE",
            "arguments.params.source_amendment.successor.artifact.digest",
            "64 bytes",
            format!("bytes={}", artifact.digest.len()),
        ));
    }
    if !artifact
        .digest
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-DIGEST-HEX",
            "arguments.params.source_amendment.successor.artifact.digest",
            "lowercase hexadecimal SHA256",
            "invalid characters",
        ));
    }
    if let Some(reference) = &artifact.reference {
        if reference.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-SOURCE-AMENDMENT-REFERENCE-BLANK",
                "arguments.params.source_amendment.successor.artifact.reference",
                "nonblank reference when present",
                "blank",
            ));
        }
        if reference.len() > MAX_SOURCE_PATH_BYTES {
            return Err(schema_refusal(
                "WP6-SOURCE-AMENDMENT-REFERENCE-SIZE",
                "arguments.params.source_amendment.successor.artifact.reference",
                format!("at most {MAX_SOURCE_PATH_BYTES} UTF-8 bytes"),
                format!("bytes={}", reference.len()),
            ));
        }
    }
    if amendment.authorization_scope.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-AUTHORIZATION-SCOPE-BLANK",
            "arguments.params.source_amendment.authorization_scope",
            "nonblank authorization_scope",
            "blank",
        ));
    }
    if amendment.authorization_scope.len() > MAX_PIPELINE_INPUT_BYTES {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-AUTHORIZATION-SCOPE-SIZE",
            "arguments.params.source_amendment.authorization_scope",
            "at most 65536 UTF-8 bytes",
            format!("bytes={}", amendment.authorization_scope.len()),
        ));
    }
    if amendment.authorization_provenance.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-AUTHORIZATION-PROVENANCE-BLANK",
            "arguments.params.source_amendment.authorization_provenance",
            "nonblank authorization_provenance",
            "blank",
        ));
    }
    if amendment.authorization_provenance.len() > MAX_PIPELINE_INPUT_BYTES {
        return Err(schema_refusal(
            "WP6-SOURCE-AMENDMENT-AUTHORIZATION-PROVENANCE-SIZE",
            "arguments.params.source_amendment.authorization_provenance",
            "at most 65536 UTF-8 bytes",
            format!("bytes={}", amendment.authorization_provenance.len()),
        ));
    }
    Ok(())
}
