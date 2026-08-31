//! Where a photograph was taken, and how much to believe it.
//!
//! A position in a file looks like a fact and often is not. A phone that
//! cannot see the sky asks the cell towers instead and writes the answer down
//! with the same six decimal places it would use for a satellite fix; a
//! camera holds the last fix it got and stamps it on a photograph taken
//! twenty minutes later in the next valley. Both come out as coordinates, and
//! neither says so.
//!
//! So the file's own evidence is weighed rather than trusted, and the verdict
//! is shown. It never refuses to place a photograph — it says how sure it is,
//! and lets somebody look.

use std::fmt;

/// Where a photograph was taken. Degrees, north and east positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// A position that could exist. Everything else is a misread file.
    pub fn new(latitude: f64, longitude: f64) -> Option<Self> {
        let sane = latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude);
        sane.then_some(Self {
            latitude,
            longitude,
        })
    }

    /// The place in a map's address, from a template holding `{lat}` and
    /// `{lon}`.
    ///
    /// A template and not a fixed address, because which map somebody wants
    /// is not ours to decide: it differs by country, by habit and by whether
    /// they have an account anywhere.
    ///
    /// The numbers are written plainly with a full stop, never by the
    /// locale's rules — a URL with a comma in the coordinates is a URL that
    /// takes you to the wrong continent.
    pub fn in_map(&self, template: &str) -> String {
        template
            .replace("{lat}", &format!("{:.6}", self.latitude))
            .replace("{lon}", &format!("{:.6}", self.longitude))
    }

    /// Coordinates as somebody would type them, and as this reads them back.
    pub fn typed(&self) -> String {
        format!("{:.6}, {:.6}", self.latitude, self.longitude)
    }

    /// The two halves as XMP spells them: `50,4.530000N` and
    /// `14,26.268000E`.
    ///
    /// Degrees, a comma, minutes with a decimal fraction, and a letter for
    /// the hemisphere. Not our choice — it is what `exif:GPSLatitude` is
    /// defined to hold, and a decimal degree written there is read by
    /// nothing.
    pub fn as_xmp(&self) -> (String, String) {
        (
            xmp_half(self.latitude, 'N', 'S'),
            xmp_half(self.longitude, 'E', 'W'),
        )
    }
}

fn xmp_half(value: f64, positive: char, negative: char) -> String {
    let degrees = value.abs().trunc();
    let minutes = (value.abs() - degrees) * 60.0;
    let letter = if value >= 0.0 { positive } else { negative };
    format!("{degrees:.0},{minutes:.6}{letter}")
}

/// Reads back what [`Place::as_xmp`] writes, and the whole-degree and
/// seconds forms the specification also allows.
pub fn from_xmp(latitude: &str, longitude: &str) -> Option<Place> {
    Place::new(xmp_degrees(latitude)?, xmp_degrees(longitude)?)
}

fn xmp_degrees(text: &str) -> Option<f64> {
    let text = text.trim();
    let letter = text.chars().last().filter(|c| c.is_ascii_alphabetic());
    let numbers = match letter {
        Some(_) => &text[..text.len() - 1],
        None => text,
    };

    let mut parts = numbers.split(',').map(str::trim);
    let degrees: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let seconds: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
    let value = degrees.abs() + minutes / 60.0 + seconds / 3_600.0;

    let negative = matches!(letter, Some('S' | 's' | 'W' | 'w')) || degrees < 0.0;
    Some(if negative { -value } else { value })
}

/// The map every new installation starts with. No account, no key, no
/// country it works better in than others.
pub const OPENSTREETMAP: &str =
    "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}";

impl fmt::Display for Place {
    /// With the hemisphere as a letter rather than a sign. `-14.4` is a
    /// number; `14.4378° W` is a place.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (north, east) = (self.latitude >= 0.0, self.longitude >= 0.0);
        write!(
            f,
            "{:.4}\u{b0} {}, {:.4}\u{b0} {}",
            self.latitude.abs(),
            if north { 'N' } else { 'S' },
            self.longitude.abs(),
            if east { 'E' } else { 'W' },
        )
    }
}

/// Reads back what [`Place::typed`] writes, and rather more besides.
///
/// People paste coordinates from everywhere: with a comma or a space between
/// them, with degree signs, with `N` and `E` on either end. All of it means
/// the same two numbers, and refusing any of it would be pedantry.
pub fn parse(text: &str) -> Option<Place> {
    let mut numbers: Vec<f64> = Vec::with_capacity(2);
    let mut negative = [false, false];
    let mut current = String::new();

    let flush = |current: &mut String, numbers: &mut Vec<f64>| {
        if !current.is_empty() {
            if let Ok(value) = current.parse::<f64>() {
                numbers.push(value);
            }

            current.clear();
        }
    };

    for character in text.chars() {
        match character {
            '0'..='9' | '.' => current.push(character),
            '-' if current.is_empty() => current.push('-'),
            // A letter both ends a number and says which side of nothing it
            // is on. `S` and `W` on a number already written `-51` must not
            // negate it twice.
            'S' | 's' | 'W' | 'w' => {
                flush(&mut current, &mut numbers);
                if let Some(at) = numbers.len().checked_sub(1)
                    && at < 2
                {
                    negative[at] = true;
                }
            }
            // A comma is a separator here, never a decimal point: half the
            // world writes `50,0755` and the other half `50.0755, 14.4378`,
            // and only one of those readings survives being wrong.
            _ => flush(&mut current, &mut numbers),
        }
    }

    flush(&mut current, &mut numbers);
    if numbers.len() != 2 {
        return None;
    }

    let sign = |value: f64, flip: bool| if flip { -value.abs() } else { value };
    Place::new(sign(numbers[0], negative[0]), sign(numbers[1], negative[1]))
}

/// How much a position is to be trusted. Worse is greater, so the worst of
/// several reasons is the one that stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum Verdict {
    /// Nothing to judge: the photograph says nothing about where it was.
    #[default]
    Nowhere,
    /// The camera knew where it was.
    Precise,
    /// Near enough for a pin on a map, not near enough to name a street.
    Approximate,
    /// Probably somewhere else entirely.
    Doubtful,
}

impl Verdict {
    pub const ALL: [Self; 4] = [
        Self::Nowhere,
        Self::Precise,
        Self::Approximate,
        Self::Doubtful,
    ];

    /// The stable name the settings and the filter use.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nowhere => "nowhere",
            Self::Precise => "precise",
            Self::Approximate => "approximate",
            Self::Doubtful => "doubtful",
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Nowhere => "place-nowhere",
            Self::Precise => "place-precise",
            Self::Approximate => "place-approximate",
            Self::Doubtful => "place-doubtful",
        }
    }

    /// As stored in the catalogue. A number and not the name, because it is
    /// compared and ordered there.
    pub fn as_number(self) -> i64 {
        match self {
            Self::Nowhere => 0,
            Self::Precise => 1,
            Self::Approximate => 2,
            Self::Doubtful => 3,
        }
    }

    pub fn from_number(value: i64) -> Self {
        match value {
            1 => Self::Precise,
            2 => Self::Approximate,
            3 => Self::Doubtful,
            _ => Self::Nowhere,
        }
    }

    /// Is this one worth looking at again?
    pub fn is_doubted(self) -> bool {
        matches!(self, Self::Approximate | Self::Doubtful)
    }
}

/// What the file said about how it knew where it was.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evidence<'a> {
    /// The camera's own estimate of its horizontal error, in metres.
    pub error_metres: Option<f64>,
    /// `GPS`, `CELLID`, `WLAN`, `MANUAL`, or whatever else was written.
    pub method: Option<&'a str>,
    /// When the fix was taken, in seconds since the epoch. Genuinely UTC:
    /// it is the satellites' own clock.
    pub fixed_at: Option<i64>,
    /// When the shutter fired, **also in UTC**, or nothing.
    ///
    /// Nothing, and not a guess. `DateTimeOriginal` is a wall clock with no
    /// zone attached, and comparing it against a UTC fix means comparing two
    /// clocks that are hours apart on purpose. On a real folder of Czech
    /// photographs that read as a one-hour-stale fix on every single one, and
    /// a badge that fires on everything says nothing at all. So the age of a
    /// fix is judged only when the file said what its clock was set to.
    pub taken_at: Option<i64>,
}

/// Why a position was doubted.
///
/// Kept and stored rather than worked out again, because the evidence it
/// rests on is in the file and the file is not opened to draw a tooltip.
/// Naming the reason is the difference between a badge somebody acts on and
/// one they learn to ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The camera said itself that it was not sure.
    Error,
    /// The fix came from the towers or the network, not the sky.
    Method,
    /// It was already old when the shutter fired.
    Stale,
}

impl Reason {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Error => "place-because-error",
            Self::Method => "place-because-method",
            Self::Stale => "place-because-stale",
        }
    }

    pub fn as_number(self) -> i64 {
        match self {
            Self::Error => 1,
            Self::Method => 2,
            Self::Stale => 3,
        }
    }

    pub fn from_number(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::Error),
            2 => Some(Self::Method),
            3 => Some(Self::Stale),
            _ => None,
        }
    }
}

/// A verdict, and the strongest reason for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgement {
    pub verdict: Verdict,
    pub because: Option<Reason>,
}

/// Beyond this the camera is telling us it does not really know: a satellite
/// fix is a handful of metres, and anything above a city block came from
/// somewhere else.
const VAGUE_METRES: f64 = 25.0;

/// And beyond this it is a guess. Cell triangulation in open country is
/// wrong by this much and more.
const HOPELESS_METRES: f64 = 500.0;

/// A fix this old was taken somewhere else. Two minutes at walking pace is a
/// couple of streets; in a car it is a different village.
const STALE_SECONDS: i64 = 120;

/// And this old is a different part of the day.
const ANCIENT_SECONDS: i64 = 20 * 60;

/// Weighs up what the file said.
///
/// Every reason is considered and the worst one stands, because they are not
/// alternatives: a stale fix from a cell tower is both stale and vague, and
/// the photograph is no better placed for our having found two reasons rather
/// than one.
pub fn judge(evidence: Evidence<'_>) -> Judgement {
    let mut verdict = Verdict::Precise;
    let mut because = None;
    let mut worsen = |worse: Verdict, reason: Reason| {
        if worse > verdict {
            verdict = worse;
            because = Some(reason);
        }
    };

    if let Some(metres) = evidence.error_metres.filter(|metres| *metres > 0.0) {
        if metres >= HOPELESS_METRES {
            worsen(Verdict::Doubtful, Reason::Error);
        } else if metres >= VAGUE_METRES {
            worsen(Verdict::Approximate, Reason::Error);
        }
    }

    // The word the camera wrote for how it found itself. A phone indoors
    // falls back to the towers or the network and says so, and that is the
    // clearest signal there is.
    match evidence.method.map(str::to_ascii_uppercase).as_deref() {
        Some("CELLID") | Some("CELL") => worsen(Verdict::Doubtful, Reason::Method),
        Some("WLAN") | Some("WIFI") | Some("NETWORK") => {
            worsen(Verdict::Approximate, Reason::Method)
        }
        // Somebody typed it. That is not a doubt, it is the opposite of one.
        _ => {}
    }

    if let (Some(fixed), Some(taken)) = (evidence.fixed_at, evidence.taken_at) {
        let age = (taken - fixed).abs();
        if age >= ANCIENT_SECONDS {
            worsen(Verdict::Doubtful, Reason::Stale);
        } else if age >= STALE_SECONDS {
            worsen(Verdict::Approximate, Reason::Stale);
        }
    }

    Judgement { verdict, because }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_that_could_not_exist_is_not_one() {
        assert!(Place::new(50.0755, 14.4378).is_some());
        assert!(Place::new(-90.0, 180.0).is_some());
        assert!(Place::new(91.0, 0.0).is_none());
        assert!(Place::new(0.0, 181.0).is_none());
        assert!(Place::new(f64::NAN, 0.0).is_none());
    }

    #[test]
    fn a_place_reads_as_a_place_and_not_as_two_numbers() {
        let prague = Place::new(50.0755, 14.4378).unwrap();
        assert_eq!(prague.to_string(), "50.0755\u{b0} N, 14.4378\u{b0} E");

        let sydney = Place::new(-33.8688, 151.2093).unwrap();
        assert_eq!(sydney.to_string(), "33.8688\u{b0} S, 151.2093\u{b0} E");
    }

    /// The one that sends somebody to the wrong continent. A URL is not
    /// written in anybody's locale.
    #[test]
    fn a_map_address_holds_full_stops_and_the_place_it_was_given() {
        let place = Place::new(-33.8688, 151.2093).unwrap();
        let url = place.in_map(OPENSTREETMAP);
        assert!(url.contains("mlat=-33.868800"), "{url}");
        assert!(url.contains("mlon=151.209300"), "{url}");
        assert!(!url.contains(','), "a comma in the coordinates: {url}");
    }

    #[test]
    fn coordinates_are_read_back_however_they_were_typed() {
        let prague = Place::new(50.0755, 14.4378).unwrap();
        assert_eq!(parse(&prague.typed()), Some(prague));

        for text in [
            "50.0755, 14.4378",
            "50.0755 14.4378",
            "50.0755\u{b0} N, 14.4378\u{b0} E",
            "  50.0755;14.4378  ",
        ] {
            let read = parse(text).unwrap_or_else(|| panic!("{text} was refused"));
            assert!((read.latitude - 50.0755).abs() < 1e-9, "{text}: {read:?}");
            assert!((read.longitude - 14.4378).abs() < 1e-9, "{text}: {read:?}");
        }
    }

    #[test]
    fn a_hemisphere_is_read_from_either_the_sign_or_the_letter() {
        let by_sign = parse("-33.8688, -70.6693").unwrap();
        let by_letter = parse("33.8688 S, 70.6693 W").unwrap();
        assert_eq!(by_sign, by_letter);

        // And never twice. `-33.8688 S` is one southern place, not the
        // northern one it would become if the letter flipped the sign again.
        assert_eq!(parse("-33.8688 S, -70.6693 W").unwrap(), by_sign);
    }

    #[test]
    fn nonsense_is_refused_rather_than_half_read() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("somewhere nice"), None);
        assert_eq!(parse("50.0755"), None, "one number is not a place");
        assert_eq!(parse("500.1, 14.4"), None, "off the planet");
    }

    /// The form `exif:GPSLatitude` is defined to hold. A decimal degree
    /// written there is read by nothing at all.
    #[test]
    fn a_place_goes_into_xmp_and_comes_back() {
        for place in [
            Place::new(50.0755, 14.4378).unwrap(),
            Place::new(-33.8688, 151.2093).unwrap(),
            Place::new(0.0, -0.1278).unwrap(),
        ] {
            let (latitude, longitude) = place.as_xmp();
            let back = from_xmp(&latitude, &longitude)
                .unwrap_or_else(|| panic!("{latitude} {longitude} was refused"));
            assert!(
                (back.latitude - place.latitude).abs() < 1e-6
                    && (back.longitude - place.longitude).abs() < 1e-6,
                "{place:?} became {back:?} through {latitude} {longitude}"
            );
        }

        let prague = Place::new(50.0755, 14.4378).unwrap();
        assert_eq!(prague.as_xmp().0, "50,4.530000N");
        assert_eq!(prague.as_xmp().1, "14,26.268000E");
    }

    #[test]
    fn the_other_xmp_spellings_are_read_too() {
        // Whole degrees, and degrees-minutes-seconds, both allowed.
        let whole = from_xmp("50N", "14E").unwrap();
        assert_eq!((whole.latitude, whole.longitude), (50.0, 14.0));

        let dms = from_xmp("50,4,32N", "14,26,16E").unwrap();
        assert!((dms.latitude - 50.075_555).abs() < 1e-5, "{dms:?}");
    }

    #[test]
    fn a_camera_that_knew_where_it_was_is_believed() {
        let judgement = judge(Evidence {
            error_metres: Some(4.0),
            method: Some("GPS"),
            fixed_at: Some(1_000),
            taken_at: Some(1_010),
        });
        assert_eq!(judgement.verdict, Verdict::Precise);
        assert_eq!(judgement.because, None);
    }

    #[test]
    fn a_position_with_nothing_said_about_it_is_taken_at_its_word() {
        // Most cameras write no error, no method and no fix time. Doubting
        // every one of them would make the badge meaningless.
        assert_eq!(judge(Evidence::default()).verdict, Verdict::Precise);
    }

    #[test]
    fn a_coarse_estimate_is_doubted_in_proportion() {
        let of = |metres: f64| {
            judge(Evidence {
                error_metres: Some(metres),
                ..Default::default()
            })
            .verdict
        };
        assert_eq!(of(8.0), Verdict::Precise);
        assert_eq!(of(60.0), Verdict::Approximate);
        assert_eq!(of(3_000.0), Verdict::Doubtful);
        assert_eq!(of(0.0), Verdict::Precise, "no estimate is not a bad one");
    }

    #[test]
    fn a_fix_from_the_towers_is_not_a_fix_from_the_sky() {
        let by = |method: &str| {
            judge(Evidence {
                method: Some(method),
                ..Default::default()
            })
            .verdict
        };
        assert_eq!(by("CELLID"), Verdict::Doubtful);
        assert_eq!(by("WLAN"), Verdict::Approximate);
        assert_eq!(by("wifi"), Verdict::Approximate);
        assert_eq!(by("GPS"), Verdict::Precise);
        // Somebody typed it in. That is the opposite of a doubt.
        assert_eq!(by("MANUAL"), Verdict::Precise);
    }

    #[test]
    fn a_fix_from_twenty_minutes_ago_was_taken_somewhere_else() {
        let after = |seconds: i64| {
            judge(Evidence {
                fixed_at: Some(10_000),
                taken_at: Some(10_000 + seconds),
                ..Default::default()
            })
            .verdict
        };
        assert_eq!(after(30), Verdict::Precise);
        assert_eq!(after(300), Verdict::Approximate);
        assert_eq!(after(3_600), Verdict::Doubtful);
        // The GPS clock is UTC and the shutter's is not, so a fix can look
        // as though it came after the photograph. The distance is what
        // matters, not the direction.
        assert_eq!(after(-3_600), Verdict::Doubtful);
    }

    /// Two reasons are not better than one: the photograph is no better
    /// placed for our having found both.
    #[test]
    fn the_worst_reason_is_the_one_that_stands() {
        let judgement = judge(Evidence {
            error_metres: Some(60.0),
            method: Some("CELLID"),
            fixed_at: Some(0),
            taken_at: Some(200),
        });
        assert_eq!(judgement.verdict, Verdict::Doubtful);
        assert_eq!(judgement.because, Some(Reason::Method));
    }

    #[test]
    fn a_verdict_survives_the_catalogue() {
        for verdict in Verdict::ALL {
            assert_eq!(Verdict::from_number(verdict.as_number()), verdict);
        }

        assert_eq!(Verdict::from_number(99), Verdict::Nowhere);
    }
}
