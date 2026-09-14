fn digits(value: &[u8]) -> Option<u32> {
    value.iter().try_fold(0_u32, |number, byte| {
        byte.is_ascii_digit()
            .then_some(number * 10 + u32::from(byte - b'0'))
    })
}

fn leap(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Strict RFC3339 timestamp parser used for validation and ordering.
pub(crate) fn parse_rfc3339(value: &str) -> Option<(i64, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = digits(bytes.get(0..4)?)?;
    let month = digits(bytes.get(5..7)?)?;
    let day = digits(bytes.get(8..10)?)?;
    let hour = digits(bytes.get(11..13)?)?;
    let minute = digits(bytes.get(14..16)?)?;
    let second = digits(bytes.get(17..19)?)?;
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let zone_start = if matches!(bytes.last(), Some(b'Z' | b'z')) {
        bytes.len() - 1
    } else {
        bytes.iter().rposition(|byte| matches!(byte, b'+' | b'-'))?
    };
    if zone_start < 19 {
        return None;
    }
    let fraction = bytes.get(19..zone_start)?;
    let nanos = if fraction.is_empty() {
        0
    } else {
        if fraction[0] != b'.'
            || fraction.len() == 1
            || fraction.len() > 10
            || !fraction[1..].iter().all(u8::is_ascii_digit)
        {
            return None;
        }
        let number = digits(&fraction[1..])?;
        number * 10_u32.pow(u32::try_from(10 - fraction.len()).ok()?)
    };

    let offset = if matches!(bytes.last(), Some(b'Z' | b'z')) {
        if zone_start + 1 != bytes.len() {
            return None;
        }
        0_i64
    } else {
        let zone = bytes.get(zone_start..)?;
        if zone.len() != 6 || zone[3] != b':' {
            return None;
        }
        let hours = digits(&zone[1..3])?;
        let minutes = digits(&zone[4..6])?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        let seconds = i64::from(hours * 3600 + minutes * 60);
        if zone[0] == b'-' { -seconds } else { seconds }
    };
    let local = days_from_civil(i64::from(year), i64::from(month), i64::from(day)) * 86_400
        + i64::from(hour * 3600 + minute * 60 + second);
    Some((local - offset, nanos))
}

#[cfg(test)]
mod tests {
    use super::parse_rfc3339;

    #[test]
    fn parses_offsets_and_rejects_invalid_calendar_dates() {
        assert_eq!(
            parse_rfc3339("2026-09-14T08:00:00+08:00"),
            parse_rfc3339("2026-09-14T00:00:00Z")
        );
        assert!(parse_rfc3339("2026-02-29T00:00:00Z").is_none());
        assert!(parse_rfc3339("not-a-timestamp").is_none());
    }
}
