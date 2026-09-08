//! Journal-day arithmetic.
//!
//! A journal day runs from [`DAY_START_HOUR`] to one minute before it the next
//! morning, in the entry's own local time. An entry written at 01:00 belongs to
//! the previous day: you were still up working on it.
//!
//! Every date shown anywhere in the app comes from [`journal_date`]. Taking a
//! calendar date off a timestamp directly is always a bug.

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, TimeDelta};

/// Hour at which a new journal day begins, in local time.
///
/// TODO: promote to a global user setting once user settings exist. It is
/// deliberately not per-project and never stored on disk.
pub const DAY_START_HOUR: u32 = 5;

/// The journal day a timestamp belongs to.
///
/// Shifting back by the start hour before taking the date is what makes the
/// whole rule fall out, with no branching on the hour.
///
/// The offset carried by `created` is load-bearing: the date is read in the
/// timezone the entry was written in, so it keeps its meaning when the file is
/// later read on a machine elsewhere. Converting to UTC first would break that.
pub fn journal_date_at(created: DateTime<FixedOffset>, day_start_hour: u32) -> NaiveDate {
    (created - TimeDelta::hours(day_start_hour as i64)).date_naive()
}

/// [`journal_date_at`] with the app's configured start hour.
pub fn journal_date(created: DateTime<FixedOffset>) -> NaiveDate {
    journal_date_at(created, DAY_START_HOUR)
}

/// The journal day currently in progress, in the machine's local timezone.
///
/// Used to default a new entry's date, so opening the editor at 02:00 offers
/// yesterday, which is nearly always what was meant.
pub fn today() -> NaiveDate {
    journal_date(Local::now().fixed_offset())
}

/// The "Day N" number for a journal date: the project's `start_date` is Day 1.
///
/// Entries before the start date yield zero or negative numbers. That is
/// reported honestly rather than clamped; the UI offers to move `start_date`
/// back instead of lying about the count.
pub fn day_number(start_date: NaiveDate, date: NaiveDate) -> i64 {
    (date - start_date).num_days() + 1
}

/// Whole days between two journal dates, for the timeline's gap connectors.
pub fn days_between(a: NaiveDate, b: NaiveDate) -> i64 {
    (b - a).num_days()
}

/// `Sep 05, 2026` - zero-padded so the width never changes.
///
/// The video draws this on every frame, where a varying width would read as a
/// flicker in a label that is supposed to sit still.
pub fn format_real_world(date: NaiveDate) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {:02}, {}",
        MONTHS[(date.month() - 1) as usize],
        date.day(),
        date.year()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse an RFC 3339 timestamp, panicking on malformed test input.
    fn ts(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).expect("test timestamp should be valid RFC 3339")
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("test date should be valid")
    }

    #[test]
    fn late_night_belongs_to_the_previous_day() {
        // Still up working on the 10th.
        assert_eq!(journal_date(ts("2026-07-11T01:00:00+02:00")), d(2026, 7, 10));
    }

    #[test]
    fn after_waking_belongs_to_the_new_day() {
        assert_eq!(journal_date(ts("2026-07-11T10:00:00+02:00")), d(2026, 7, 11));
    }

    #[test]
    fn sleeping_between_two_entries_splits_them_across_days() {
        // Nine hours apart, but deliberately different journal days: the user
        // went to bed in between.
        let before_bed = journal_date(ts("2026-07-11T01:00:00+02:00"));
        let after_waking = journal_date(ts("2026-07-11T10:00:00+02:00"));
        assert_ne!(before_bed, after_waking);
        assert_eq!(days_between(before_bed, after_waking), 1);
    }

    #[test]
    fn evening_and_the_small_hours_share_one_journal_day() {
        // These two stack on the timeline with no gap connector between them.
        assert_eq!(
            journal_date(ts("2026-07-10T23:00:00+02:00")),
            journal_date(ts("2026-07-11T01:30:00+02:00")),
        );
    }

    #[test]
    fn the_boundary_is_exactly_five_am() {
        assert_eq!(journal_date(ts("2026-07-11T04:59:59+02:00")), d(2026, 7, 10));
        assert_eq!(journal_date(ts("2026-07-11T05:00:00+02:00")), d(2026, 7, 11));
    }

    #[test]
    fn the_stored_offset_survives_being_read_elsewhere() {
        // Written at 23:30 in Paris. Read anywhere, it is still a Paris
        // evening, and still the 10th.
        let paris = ts("2026-07-10T23:30:00+02:00");
        assert_eq!(journal_date(paris), d(2026, 7, 10));

        // The same instant expressed in Tokyo's offset is a different local
        // wall-clock reading and lands on a different journal day. Storing the
        // offset is precisely what lets us never perform that conversion.
        let tokyo = FixedOffset::east_opt(9 * 3600).expect("+09:00 is a valid offset");
        assert_eq!(journal_date(paris.with_timezone(&tokyo)), d(2026, 7, 11));
    }

    #[test]
    fn day_count_is_unaffected_by_a_dst_change_in_the_span() {
        // A summer entry (+02:00) and a winter one (+01:00) are still counted
        // in whole local days: no hour is gained or lost in the arithmetic.
        let start = journal_date(ts("2026-08-01T12:00:00+02:00"));
        let after = journal_date(ts("2026-12-01T12:00:00+01:00"));
        assert_eq!(days_between(start, after), 122);
    }

    #[test]
    fn a_spring_forward_night_does_not_skip_a_journal_day() {
        // Europe/Paris 2027-03-28: 02:00 jumps to 03:00. The evening before is
        // +01:00, the next morning +02:00; they stay consecutive journal days.
        let saturday_night = journal_date(ts("2027-03-28T01:00:00+01:00"));
        let sunday_morning = journal_date(ts("2027-03-28T10:00:00+02:00"));
        assert_eq!(saturday_night, d(2027, 3, 27));
        assert_eq!(sunday_morning, d(2027, 3, 28));
        assert_eq!(days_between(saturday_night, sunday_morning), 1);
    }

    #[test]
    fn hour_zero_degrades_to_plain_calendar_dates() {
        assert_eq!(
            journal_date_at(ts("2026-07-11T01:00:00+02:00"), 0),
            d(2026, 7, 11)
        );
        assert_eq!(
            journal_date_at(ts("2026-07-11T23:59:59+02:00"), 0),
            d(2026, 7, 11)
        );
    }

    #[test]
    fn start_date_is_day_one() {
        let start = d(2026, 6, 1);
        assert_eq!(day_number(start, d(2026, 6, 1)), 1);
        assert_eq!(day_number(start, d(2026, 6, 2)), 2);
        assert_eq!(day_number(start, d(2026, 7, 9)), 39);
    }

    #[test]
    fn entries_before_the_start_date_are_reported_honestly() {
        let start = d(2026, 6, 1);
        assert_eq!(day_number(start, d(2026, 5, 31)), 0);
        assert_eq!(day_number(start, d(2026, 5, 30)), -1);
    }

    #[test]
    fn several_entries_in_one_journal_day_share_a_number() {
        let start = d(2026, 6, 1);
        let morning = journal_date(ts("2026-06-10T09:00:00+02:00"));
        let evening = journal_date(ts("2026-06-10T22:00:00+02:00"));
        let small_hours = journal_date(ts("2026-06-11T02:00:00+02:00"));
        assert_eq!(day_number(start, morning), 10);
        assert_eq!(day_number(start, evening), 10);
        assert_eq!(day_number(start, small_hours), 10);
    }

    #[test]
    fn real_world_dates_are_a_constant_width() {
        assert_eq!(format_real_world(d(2026, 9, 5)), "Sep 05, 2026");
        assert_eq!(format_real_world(d(2026, 9, 15)), "Sep 15, 2026");
        assert_eq!(format_real_world(d(2026, 12, 31)), "Dec 31, 2026");
        for day in 1..=28 {
            assert_eq!(format_real_world(d(2026, 2, day)).len(), 12);
        }
    }
}
