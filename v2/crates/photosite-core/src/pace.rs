//! Where a long run stands, in one line.
//!
//! A describe run or a face sweep is minutes to hours, and the only question
//! anybody has while it goes is whether to wait or go and do something
//! else. So the line says how far it is, how fast it goes, how long is left
//! and — the one number that answers the question — when it will be done.

use crate::t;
use std::time::Duration;

/// `Processing 148/2300 (6.4%), avg. 5.2 s/photo, remaining 3:20:15, ETA 7:30 PM`
///
/// `attempted` is how many photographs were actually worked on, which is
/// what the pace is measured over; `done` counts the ones skipped as well,
/// which is what the progress is measured over. Until something has been
/// worked on there is no pace to speak of, and the line says only how far.
pub fn status(done: u64, total: u64, attempted: u64, elapsed: Duration) -> String {
    status_at(done, total, attempted, elapsed, crate::time::local_now())
}

/// The same, with the clock handed in, so it can be tested against a fixed
/// moment.
pub fn status_at(done: u64, total: u64, attempted: u64, elapsed: Duration, now: i64) -> String {
    let percent = if total == 0 {
        0.0
    } else {
        done as f64 * 100.0 / total as f64
    };
    let percent = format!("{percent:.1}");
    if attempted == 0 || elapsed.is_zero() {
        return t!(
            "pace-starting",
            done = done as i64,
            total = total as i64,
            percent = percent
        );
    }

    let each = elapsed.as_secs_f64() / attempted as f64;
    let left = total.saturating_sub(done) as f64 * each;
    t!(
        "pace-status",
        done = done as i64,
        total = total as i64,
        percent = percent,
        each = format!("{each:.1}"),
        remaining = crate::time::format_span(left),
        eta = crate::time::format_clock(now + left.round() as i64)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_says_how_far_how_fast_how_long_and_when() {
        let now = crate::time::from_civil(2024, 7, 14, 16, 22, 45);
        let said = status_at(148, 2300, 148, Duration::from_secs_f64(148.0 * 5.2), now);
        assert_eq!(
            said,
            "Processing 148/2300 (6.4%), avg. 5.2 s/photo, remaining 3:06:30, ETA 7:29 PM"
        );
    }

    #[test]
    fn before_anything_was_worked_on_there_is_no_pace() {
        let said = status_at(3, 10, 0, Duration::from_secs(7), 0);
        assert_eq!(said, "Processing 3/10 (30.0%)");
    }

    #[test]
    fn skipped_photographs_count_towards_the_distance_but_not_the_pace() {
        let now = crate::time::from_civil(2024, 7, 14, 10, 0, 0);
        // Ten done, of which two were worked on at four seconds each.
        let said = status_at(10, 20, 2, Duration::from_secs(8), now);
        assert!(said.contains("avg. 4.0 s/photo"), "{said}");
        assert!(said.contains("remaining 0:40"), "{said}");
        assert!(said.contains("ETA 10:00 AM"), "{said}");
    }

    #[test]
    fn an_empty_run_does_not_divide_by_nothing() {
        assert_eq!(
            status_at(0, 0, 0, Duration::ZERO, 0),
            "Processing 0/0 (0.0%)"
        );
    }
}
