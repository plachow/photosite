//! Turning the seconds in the catalogue back into a date somebody reads.
//!
//! The other direction — the text in an EXIF block into seconds — lives in
//! `photosite-image`, because parsing EXIF is that crate's job and it has no
//! business depending on the domain to do it. The two are inverses and both
//! are tested against fixed dates, which is what keeps them honest without
//! tying the crates together.
//!
//! **These are wall-clock times, not moments.** EXIF records no time zone,
//! so a capture time is the clock where the photographer stood; we read and
//! write it as if it were UTC, which returns exactly what the camera said.
//! Anything else would shift every photograph by wherever the viewer happens
//! to be sitting.

/// `2024-07-14 09:30`, which is as much as anybody wants to see next to a
/// photograph. Seconds are dropped: they matter for ordering, not for
/// reading.
pub fn format(seconds: i64) -> String {
    let (year, month, day, hour, minute, _) = civil(seconds);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// The same with the seconds, for a log or a diagnostic.
pub fn format_exact(seconds: i64) -> String {
    let (year, month, day, hour, minute, second) = civil(seconds);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Seconds since the epoch to a calendar date.
///
/// Howard Hinnant's algorithm run backwards. A date crate would do it too,
/// and would be a dependency and a build for one subtraction.
pub fn civil(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    // Rust's `/` and `%` truncate towards zero, which is wrong before 1970 —
    // `div_euclid` floors, which is what a calendar needs.
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };

    (
        year,
        month,
        day,
        (rest / 3_600) as u32,
        (rest % 3_600 / 60) as u32,
        (rest % 60) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_where_it_should_be() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(format(0), "1970-01-01 00:00");
    }

    /// The number here is the one the EXIF reader's own test builds from
    /// `2024:07:14 09:30:00`. If either side of the calendar drifts, one of
    /// the two tests fails.
    #[test]
    fn a_known_date_comes_back_whole() {
        assert_eq!(format(1_720_949_400), "2024-07-14 09:30");
        assert_eq!(civil(1_720_949_400), (2024, 7, 14, 9, 30, 0));
        assert_eq!(format_exact(946_684_800), "2000-01-01 00:00:00");
    }

    #[test]
    fn a_leap_day_is_a_day() {
        assert_eq!(civil(1_709_164_800), (2024, 2, 29, 0, 0, 0));
        // 1900 was not a leap year, whatever the divisible-by-four rule says.
        assert_eq!(civil(-2_203_891_200), (1900, 3, 1, 0, 0, 0));
    }

    #[test]
    fn a_date_before_the_epoch_does_not_go_backwards_by_a_day() {
        // Truncating division rather than flooring puts this on the 1st.
        assert_eq!(civil(-1), (1969, 12, 31, 23, 59, 59));
        assert_eq!(civil(-86_400), (1969, 12, 31, 0, 0, 0));
    }

    #[test]
    fn every_hour_of_a_day_reads_back() {
        let midnight = 1_720_915_200; // 2024-07-14 00:00:00
        for hour in 0..24 {
            let (_, _, day, read, _, _) = civil(midnight + hour * 3_600);
            assert_eq!((day, read), (14, hour as u32), "hour {hour}");
        }
    }
}
