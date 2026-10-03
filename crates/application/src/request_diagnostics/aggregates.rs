//! Constant-label duration aggregates are independent of truncated timeline retention.
use std::collections::BTreeMap;

pub(super) const LABEL_CAP: usize = 64;

#[derive(Default)]
pub(super) struct StageAggregates {
    pub labels: BTreeMap<&'static str, StageAggregate>,
    pub dropped_labels: u64,
    pub saturated: bool,
}

#[derive(Default)]
pub(super) struct StageAggregate {
    started_count: u64,
    completed_count: u64,
    cancelled_count: u64,
    inclusive_total_us: u64,
    inclusive_min_us: Option<u64>,
    inclusive_max_us: u64,
    completed_total_us: u64,
    cancelled_total_us: u64,
}

fn add(target: &mut u64, value: u64, saturated: &mut bool) {
    *saturated |= target.checked_add(value).is_none();
    *target = target.saturating_add(value);
}

impl StageAggregates {
    pub fn snapshot(&self) -> serde_json::Value {
        self.labels
            .iter()
            .map(|(label, aggregate)| {
                (
                    (*label).to_owned(),
                    serde_json::json!({
                        "started_count": aggregate.started_count,
                        "completed_count": aggregate.completed_count,
                        "cancelled_count": aggregate.cancelled_count,
                        "inclusive_total_us": aggregate.inclusive_total_us,
                        "inclusive_min_us": aggregate.inclusive_min_us,
                        "inclusive_max_us": aggregate.inclusive_max_us,
                        "completed_total_us": aggregate.completed_total_us,
                        "cancelled_total_us": aggregate.cancelled_total_us,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    pub fn start(&mut self, label: &'static str) {
        if self.labels.contains_key(label) || self.labels.len() < LABEL_CAP {
            let aggregate = self.labels.entry(label).or_default();
            add(&mut aggregate.started_count, 1, &mut self.saturated);
        } else {
            add(&mut self.dropped_labels, 1, &mut self.saturated);
        }
    }

    pub fn end(&mut self, label: &'static str, completed: bool, duration_us: u64) {
        let Some(aggregate) = self.labels.get_mut(label) else {
            // A rejected label was counted on start; terminal events do not double count loss.
            return;
        };
        if completed {
            add(&mut aggregate.completed_count, 1, &mut self.saturated);
            add(
                &mut aggregate.completed_total_us,
                duration_us,
                &mut self.saturated,
            );
        } else {
            add(&mut aggregate.cancelled_count, 1, &mut self.saturated);
            add(
                &mut aggregate.cancelled_total_us,
                duration_us,
                &mut self.saturated,
            );
        }
        add(
            &mut aggregate.inclusive_total_us,
            duration_us,
            &mut self.saturated,
        );
        aggregate.inclusive_min_us = Some(
            aggregate
                .inclusive_min_us
                .map_or(duration_us, |min| min.min(duration_us)),
        );
        aggregate.inclusive_max_us = aggregate.inclusive_max_us.max(duration_us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_counts_sums_and_extrema_are_exact_and_saturate_explicitly() {
        let mut aggregates = StageAggregates::default();
        for value in 0..1500 {
            aggregates.start("fixed");
            aggregates.end("fixed", value % 3 != 0, value);
        }
        let fixed = &aggregates.labels["fixed"];
        assert_eq!(fixed.started_count, 1500);
        assert_eq!(fixed.completed_count, 1000);
        assert_eq!(fixed.cancelled_count, 500);
        assert_eq!(fixed.inclusive_total_us, 1499 * 1500 / 2);
        assert_eq!(
            fixed.completed_total_us + fixed.cancelled_total_us,
            fixed.inclusive_total_us
        );
        assert_eq!(fixed.inclusive_min_us, Some(0));
        assert_eq!(fixed.inclusive_max_us, 1499);
        assert_eq!(aggregates.dropped_labels, 0);
        assert!(!aggregates.saturated);
        aggregates.start("fixed");
        aggregates.end("fixed", true, u64::MAX);
        assert_eq!(aggregates.labels["fixed"].inclusive_total_us, u64::MAX);
        assert!(aggregates.saturated);
    }
}
