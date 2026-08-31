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

/// A calendar date to seconds since the epoch — the exact inverse of
/// [`civil`].
///
/// `photosite-image` holds its own copy of this arithmetic for reading EXIF
/// timestamps, and deliberately so: it is a leaf crate that must not have to
/// depend on the domain to parse a date out of a file. Both are ten lines of
/// a fixed algorithm and both are pinned to the same reference instants by
/// their tests, so they cannot quietly drift apart.
pub fn from_civil(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let month = month as i64;
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = shifted - era * 400;
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + second as i64
}

/// `2024-07-14` as the first second of that day.
///
/// Lenient about what it is handed, because it sits under a text field
/// somebody types into: nonsense is no date rather than an error, and the
/// filter simply does not narrow by it. Separators other than `-` are
/// accepted, since `2024/07/14` is what half of Europe types.
pub fn parse_date(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let parts: Vec<&str> = text
        .split(['-', '/', '.', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() != 3 {
        return None;
    }

    let year: i64 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(1826..=9999).contains(&year) {
        return None;
    }

    // A date that does not exist — the 31st of February — must not come back
    // as the 3rd of March. The round trip is the cheapest way to be sure.
    let seconds = from_civil(year, month, day, 0, 0, 0);
    let (back_year, back_month, back_day, ..) = civil(seconds);
    (back_year == year && back_month == month && back_day == day).then_some(seconds)
}

/// The last second of that day, for the far end of a range. A range typed as
/// `to 2024-07-14` plainly means the whole of the fourteenth.
pub fn parse_date_end(text: &str) -> Option<i64> {
    parse_date(text).map(|start| start + 86_399)
}

/// `2024-07-14`, which is what [`parse_date`] reads back.
pub fn format_date(seconds: i64) -> String {
    let (year, month, day, ..) = civil(seconds);
    format!("{year:04}-{month:02}-{day:02}")
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
    fn the_two_directions_are_inverses() {
        for seconds in [
            0,
            946_684_800,
            1_720_949_400,
            -86_400,
            1_709_164_800,
            2_000_000_000,
        ] {
            let (year, month, day, hour, minute, second) = civil(seconds);
            assert_eq!(
                from_civil(year, month, day, hour, minute, second),
                seconds,
                "{seconds}"
            );
        }
    }

    /// The same reference instant `photosite-image`'s EXIF test builds from
    /// `2024:07:14 09:30:00`. Both calendars are pinned to it.
    #[test]
    fn the_reference_instant_agrees_with_the_exif_reader() {
        assert_eq!(from_civil(2024, 7, 14, 9, 30, 0), 1_720_949_400);
    }

    #[test]
    fn a_typed_date_is_the_first_second_of_that_day() {
        let day = parse_date("2024-07-14").expect("a date");
        assert_eq!(format(day), "2024-07-14 00:00");
        assert_eq!(
            format(parse_date_end("2024-07-14").unwrap()),
            "2024-07-14 23:59"
        );
    }

    #[test]
    fn the_separators_people_actually_type_are_accepted() {
        let want = parse_date("2024-07-14");
        for text in ["2024/07/14", "2024.07.14", " 2024-7-14 ", "2024 07 14"] {
            assert_eq!(parse_date(text), want, "{text}");
        }
    }

    #[test]
    fn a_date_that_does_not_exist_is_no_date() {
        // Not the 3rd of March, which is where the arithmetic alone lands.
        assert_eq!(parse_date("2023-02-31"), None);
        assert_eq!(parse_date("2024-13-01"), None);
        assert_eq!(parse_date("not a date"), None);
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("2024-07"), None);
        // But a real leap day is real.
        assert!(parse_date("2024-02-29").is_some());
        assert_eq!(parse_date("2023-02-29"), None);
    }

    #[test]
    fn a_date_written_out_reads_back_in() {
        let day = parse_date("2019-11-11").expect("a date");
        assert_eq!(format_date(day), "2019-11-11");
        assert_eq!(parse_date(&format_date(day)), Some(day));
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
