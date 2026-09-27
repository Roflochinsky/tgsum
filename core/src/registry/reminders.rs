use chrono::NaiveDate;
use serde::Serialize;

use super::{compiled_connectors, parse_date, Registry, ReviewState};

/// Maintainer work only; neither a runtime permission nor a source fetch.
#[derive(Debug, Serialize)]
pub struct ReviewReminder<'a> {
    pub connector: &'a str,
    pub owner: &'a str,
    pub state: ReviewState,
    pub last_policy_reviewed_at: Option<&'a str>,
    pub next_review_due_at: &'a str,
    pub days_until_due: Option<i64>,
    /// Determined by registered code, not editable enabled_operations metadata.
    pub shipping_implementation: bool,
    pub evidence: &'a str,
    pub sources: &'a [String],
}

impl Registry {
    /// Includes overdue/unknown reviews and upcoming deadlines (inclusive).
    /// Call repository validation as well to detect invalid/future evidence.
    pub fn review_reminders(&self, today: NaiveDate, within_days: u16) -> Vec<ReviewReminder<'_>> {
        let compiled = compiled_connectors();
        let mut reminders = self
            .connectors
            .iter()
            .filter_map(|entry| {
                let state = entry.review_state(today);
                let days = parse_date(&entry.next_review_due_at)
                    .ok()
                    .map(|due| (due - today).num_days());
                if state == ReviewState::Current && days.is_some_and(|d| d > i64::from(within_days))
                {
                    return None;
                }
                Some(ReviewReminder {
                    connector: &entry.id,
                    owner: &entry.owner,
                    state,
                    last_policy_reviewed_at: entry.last_policy_reviewed_at.as_deref(),
                    next_review_due_at: &entry.next_review_due_at,
                    days_until_due: days,
                    shipping_implementation: compiled.iter().any(|c| c.profile_id == entry.id),
                    evidence: &entry.evidence,
                    sources: &entry.sources,
                })
            })
            .collect::<Vec<_>>();
        reminders.sort_by_key(|r| (r.days_until_due, r.connector));
        reminders
    }
}
