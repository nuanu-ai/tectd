use super::*;

pub(super) fn artifact_refusal(
    code: RefusalCode,
    rule: &'static str,
    path: impl Into<String>,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Error {
    let (next_action, required) = if code == RefusalCode::InputSchemaInvalid {
        ("correct_input_and_retry", "schema_valid_input")
    } else {
        ("correct_output", "output")
    };
    Error::Refused(Box::new(
        Refusal::new(code)
            .with_message(code.message())
            .with_rule(rule)
            .with_path(path)
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action(next_action)
            .with_required(required),
    ))
}

pub(super) fn duplicate_index<T: Ord>(values: impl IntoIterator<Item = T>) -> Option<usize> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .enumerate()
        .find_map(|(index, value)| (!seen.insert(value)).then_some(index))
}

pub(super) fn applies(requirement: &PipelineArtifactRequirement, verdict: Option<&str>) -> bool {
    requirement
        .when_verdict
        .as_deref()
        .is_none_or(|required| verdict == Some(required))
}

pub(super) fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

pub(super) fn valid_pattern(value: &str) -> bool {
    let count = value.bytes().filter(|byte| *byte == b'*').count();
    if count > 1 {
        return false;
    }
    let without_wildcard = value.replace('*', "x");
    valid_name(&without_wildcard)
}

pub(super) fn pattern_matches(pattern: &str, name: &str) -> bool {
    if !valid_name(name) {
        return false;
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        let middle_end = name.len().saturating_sub(suffix.len());
        name.starts_with(prefix)
            && name.ends_with(suffix)
            && name.len() >= prefix.len() + suffix.len()
            && !name[prefix.len()..middle_end].contains('/')
    } else {
        pattern == name
    }
}
