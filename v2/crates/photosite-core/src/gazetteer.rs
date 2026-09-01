//! Turning a pair of coordinates into the name of a place, on this machine.
//!
//! v1 got this out of exiftool's bundled geolocation database, one process
//! per lookup. That is the last thing exiftool was doing for this
//! application, and this is what replaces it: a
//! [GeoNames](https://download.geonames.org/export/dump/) extract, a plain
//! tab-separated file anybody can download and read, held in memory and
//! searched with arithmetic.
//!
//! **Nothing leaves the machine, and that is the whole point.** A coordinate
//! sent to a geocoding service is a photograph's location handed to
//! somebody; the reason for looking it up at all is to describe a
//! photograph, and describing a photograph is not worth telling anybody
//! where it was taken.
//!
//! The file is not in git and not shipped: it is tens of megabytes of
//! somebody else's data, under their licence. Without it there is simply no
//! place context, and the describer says so rather than guessing.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Within this many kilometres a photograph is genuinely *in or near* a
/// place. Beyond it, the place is a landmark on the horizon and saying the
/// photograph was taken there would be putting a wrong name in a caption.
pub const NEARBY_KM: f64 = 3.0;

/// Beyond this, the nearest place stops being a trustworthy keyword at all.
/// A photograph of a mountainside is not "in" a town forty kilometres away.
pub const KEYWORD_MAX_KM: f64 = 10.0;

/// The extracts this can read, finest first. They are all the same format
/// and differ only in how small a place has to be to be left out.
pub const EXTRACTS: [&str; 4] = [
    "cities500.txt",
    "cities1000.txt",
    "cities5000.txt",
    "cities15000.txt",
];

/// One populated place.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    latitude: f64,
    longitude: f64,
    country: String,
    admin1: String,
    admin2: String,
    population: i64,
}

/// The place nearest a coordinate, and how far away it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Nearby {
    pub name: String,
    /// The district or county, where the extract carries one.
    pub subregion: Option<String>,
    /// The region, state or province.
    pub region: Option<String>,
    pub country: Option<String>,
    pub distance_km: f64,
    /// Which way the photograph lies **from the place** — so "twelve
    /// kilometres east of Brno" reads the way somebody would say it. The
    /// other direction is the one a database usually gives and it is the one
    /// that puts every description backwards.
    pub bearing: i32,
}

impl Nearby {
    /// Is the photograph close enough to say it was taken here?
    pub fn is_here(&self) -> bool {
        self.distance_km <= NEARBY_KM
    }

    /// The place names a librarian would tag the photograph with, most
    /// specific first.
    ///
    /// The place itself drops out when it is too far away — or when the
    /// position is too shaky — to honestly claim the photograph was taken
    /// there. What is left is the county, the region and the country, which
    /// stay true however far off the fix was.
    pub fn keywords(&self, include_place: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if include_place && self.distance_km <= KEYWORD_MAX_KM {
            out.push(self.name.clone());
        }

        for name in [&self.subregion, &self.region, &self.country]
            .into_iter()
            .flatten()
        {
            if !out.iter().any(|had| had.eq_ignore_ascii_case(name)) {
                out.push(name.clone());
            }
        }

        out
    }
}

/// Which way round the compass a bearing points.
pub fn compass(bearing: i32) -> &'static str {
    const POINTS: [&str; 8] = [
        "compass-north",
        "compass-north-east",
        "compass-east",
        "compass-south-east",
        "compass-south",
        "compass-south-west",
        "compass-west",
        "compass-north-west",
    ];
    let normalised = bearing.rem_euclid(360);
    POINTS[((normalised as f64 / 45.0).round() as usize) % 8]
}

/// Places, in a grid so that finding the nearest is not a walk over all of
/// them.
///
/// One bucket a degree. At this latitude that is roughly 70 by 110
/// kilometres, so a lookup reads nine buckets and a few hundred places
/// rather than a hundred and fifty thousand — which matters because a bulk
/// describe run does this once per photograph.
#[derive(Debug, Default)]
pub struct Gazetteer {
    cells: HashMap<(i32, i32), Vec<Entry>>,
    /// `CZ.52` to the region's name, and `CZ.52.CZ0524` to the county's.
    admin1: HashMap<String, String>,
    admin2: HashMap<String, String>,
    /// `CZ` to `Czechia`.
    countries: HashMap<String, String>,
    /// Which file this came out of, for the diagnostics window.
    pub source: String,
    pub places: usize,
}

impl Gazetteer {
    /// Loads whatever extract is in the folder, finest first.
    ///
    /// `None` — not an error — when there is none. A missing gazetteer means
    /// no place context and nothing else; refusing to describe photographs
    /// over it would be the wrong trade in every direction.
    pub fn load(folder: &Path) -> Result<Option<Self>> {
        let Some(name) = EXTRACTS
            .into_iter()
            .find(|name| folder.join(name).is_file())
        else {
            return Ok(None);
        };

        let places = std::fs::read_to_string(folder.join(name))
            .with_context(|| format!("cannot read {}", folder.join(name).display()))?;
        let mut gazetteer = Self {
            source: name.to_owned(),
            ..Default::default()
        };
        gazetteer.read_places(&places);

        // The three name files are optional on their own: without them a
        // place still has a name, only its county and country are codes
        // nobody wants in a caption, so they are left out entirely.
        if let Ok(text) = std::fs::read_to_string(folder.join("admin1CodesASCII.txt")) {
            gazetteer.admin1 = read_codes(&text);
        }

        if let Ok(text) = std::fs::read_to_string(folder.join("admin2Codes.txt")) {
            gazetteer.admin2 = read_codes(&text);
        }

        if let Ok(text) = std::fs::read_to_string(folder.join("countryInfo.txt")) {
            gazetteer.countries = read_countries(&text);
        }

        Ok(Some(gazetteer))
    }

    /// Builds one from text, for a test and for anybody who has the extract
    /// somewhere other than on disk.
    pub fn from_text(places: &str) -> Self {
        let mut gazetteer = Self {
            source: "text".to_owned(),
            ..Default::default()
        };
        gazetteer.read_places(places);
        gazetteer
    }

    pub fn with_codes(mut self, admin1: &str, admin2: &str, countries: &str) -> Self {
        self.admin1 = read_codes(admin1);
        self.admin2 = read_codes(admin2);
        self.countries = read_countries(countries);
        self
    }

    fn read_places(&mut self, text: &str) {
        for line in text.lines() {
            let Some(entry) = read_place(line) else {
                continue;
            };

            self.places += 1;
            self.cells
                .entry(cell(entry.latitude, entry.longitude))
                .or_default()
                .push(entry);
        }
    }

    /// The nearest place, or nothing when the gazetteer holds none near
    /// enough to be worth saying.
    ///
    /// The search widens a ring at a time until something is found or the
    /// ring is plainly too far to matter. Somewhere in the middle of an
    /// ocean genuinely has no answer, and inventing one would be worse than
    /// none.
    pub fn nearest(&self, latitude: f64, longitude: f64) -> Option<Nearby> {
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }

        let (home_x, home_y) = cell(latitude, longitude);
        let mut best: Option<(&Entry, f64)> = None;
        for ring in 0..=4i32 {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    // Only the newly reached ring; the inside was searched
                    // on the way here.
                    if ring > 0 && dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }

                    let Some(entries) = self.cells.get(&wrap(home_x + dx, home_y + dy)) else {
                        continue;
                    };

                    for entry in entries {
                        let far = distance_km(latitude, longitude, entry.latitude, entry.longitude);
                        // The larger place wins a tie, which is what makes a
                        // photograph in a suburb say the city rather than the
                        // hamlet next door.
                        let better = match best {
                            None => true,
                            Some((had, so_far)) => {
                                far < so_far - 0.5
                                    || ((far - so_far).abs() <= 0.5
                                        && entry.population > had.population)
                            }
                        };
                        if better {
                            best = Some((entry, far));
                        }
                    }
                }
            }

            // Something within the ring already searched cannot be beaten by
            // anything outside it.
            if let Some((_, far)) = best
                && far < f64::from(ring) * 60.0
            {
                break;
            }
        }

        let (entry, distance_km) = best?;
        Some(Nearby {
            name: entry.name.clone(),
            subregion: self
                .admin2
                .get(&format!(
                    "{}.{}.{}",
                    entry.country, entry.admin1, entry.admin2
                ))
                .cloned(),
            region: self
                .admin1
                .get(&format!("{}.{}", entry.country, entry.admin1))
                .cloned(),
            country: self.countries.get(&entry.country).cloned(),
            distance_km,
            bearing: bearing_degrees(entry.latitude, entry.longitude, latitude, longitude),
        })
    }
}

/// One line of a GeoNames extract.
///
/// Nineteen tab-separated columns, of which seven are wanted. A line that is
/// not that is skipped rather than refused: the files carry comments and the
/// occasional oddity, and one bad row is no reason to have no gazetteer.
fn read_place(line: &str) -> Option<Entry> {
    if line.starts_with('#') {
        return None;
    }

    let columns: Vec<&str> = line.split('\t').collect();
    if columns.len() < 15 {
        return None;
    }

    Some(Entry {
        name: columns[1].trim().to_owned(),
        latitude: columns[4].trim().parse().ok()?,
        longitude: columns[5].trim().parse().ok()?,
        country: columns[8].trim().to_owned(),
        admin1: columns[10].trim().to_owned(),
        admin2: columns[11].trim().to_owned(),
        population: columns[14].trim().parse().unwrap_or(0),
    })
}

/// `CZ.52<TAB>Královéhradecký kraj<TAB>...` — the code and the name.
fn read_codes(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let code = columns.next()?.trim();
            let name = columns.next()?.trim();
            (!code.is_empty() && !name.is_empty()).then(|| (code.to_owned(), name.to_owned()))
        })
        .collect()
}

/// `countryInfo.txt`: the two-letter code first, the country's name fifth.
fn read_countries(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('\t').collect();
            let code = columns.first()?.trim();
            let name = columns.get(4)?.trim();
            (!code.is_empty() && !name.is_empty()).then(|| (code.to_owned(), name.to_owned()))
        })
        .collect()
}

/// Which one-degree bucket a coordinate falls in.
fn cell(latitude: f64, longitude: f64) -> (i32, i32) {
    (longitude.floor() as i32, latitude.floor() as i32)
}

/// A bucket that may have run off the side of the world.
///
/// Longitude wraps, which matters for the handful of places either side of
/// the date line; latitude does not, and a bucket past the pole simply holds
/// nothing.
fn wrap(x: i32, y: i32) -> (i32, i32) {
    (((x + 180) % 360 + 360) % 360 - 180, y)
}

/// Great-circle distance in kilometres.
pub fn distance_km(from_lat: f64, from_lon: f64, to_lat: f64, to_lon: f64) -> f64 {
    const EARTH_KM: f64 = 6371.0;
    let (from_lat, to_lat) = (from_lat.to_radians(), to_lat.to_radians());
    let delta_lat = to_lat - from_lat;
    let delta_lon = (to_lon - from_lon).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + from_lat.cos() * to_lat.cos() * (delta_lon / 2.0).sin().powi(2);
    2.0 * EARTH_KM * a.sqrt().clamp(0.0, 1.0).asin()
}

/// The compass bearing from one point to another, 0 at north.
pub fn bearing_degrees(from_lat: f64, from_lon: f64, to_lat: f64, to_lon: f64) -> i32 {
    let (from_lat, to_lat) = (from_lat.to_radians(), to_lat.to_radians());
    let delta_lon = (to_lon - from_lon).to_radians();
    let y = delta_lon.sin() * to_lat.cos();
    let x = from_lat.cos() * to_lat.sin() - from_lat.sin() * to_lat.cos() * delta_lon.cos();
    let degrees = y.atan2(x).to_degrees();
    (((degrees.round() as i32) % 360) + 360) % 360
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few real rows, in the format the files come in.
    const PLACES: &str = "\
3067696\tPrague\tPrague\tPraha\t50.08804\t14.42076\tP\tPPLC\tCZ\t\t52\t\t\t\t1165581\t\t202\tEurope/Prague\t2023-01-01
3078610\tBrno\tBrno\tBrünn\t49.19522\t16.60796\tP\tPPLA\tCZ\t\t78\tCZ0642\t\t\t369559\t\t223\tEurope/Prague\t2023-01-01
3068160\tPlzen\tPlzen\tPilsen\t49.7475\t13.37759\tP\tPPLA\tCZ\t\t60\t\t\t\t164180\t\t311\tEurope/Prague\t2023-01-01
# a comment line
2988507\tParis\tParis\tParis\t48.85341\t2.3488\tP\tPPLC\tFR\t\t11\t75\t\t\t2138551\t\t42\tEurope/Paris\t2023-01-01
";

    const ADMIN1: &str = "CZ.52\tKralovehradecky kraj\tKralovehradecky kraj\t3339541\nCZ.78\tSouth Moravian\tSouth Moravian\t3078609\n";
    const ADMIN2: &str = "CZ.78.CZ0642\tOkres Brno-mesto\tOkres Brno-mesto\t3078610\n";
    const COUNTRIES: &str = "#ISO\tISO3\tISO-Numeric\tfips\tCountry\nCZ\tCZE\t203\tEZ\tCzechia\nFR\tFRA\t250\tFR\tFrance\n";

    fn gazetteer() -> Gazetteer {
        Gazetteer::from_text(PLACES).with_codes(ADMIN1, ADMIN2, COUNTRIES)
    }

    #[test]
    fn a_comment_and_a_short_line_are_skipped_rather_than_refused() {
        let places = Gazetteer::from_text(
            "# a comment\nnot enough columns\n3067696\tPrague\tPrague\t\t50.08804\t14.42076\tP\tPPLC\tCZ\t\t52\t\t\t\t1165581\n",
        );
        assert_eq!(places.places, 1);
    }

    #[test]
    fn standing_in_a_city_finds_that_city() {
        let found = gazetteer().nearest(50.0875, 14.4213).unwrap();
        assert_eq!(found.name, "Prague");
        assert!(found.is_here(), "{} km", found.distance_km);
        assert_eq!(found.country.as_deref(), Some("Czechia"));
        assert_eq!(found.region.as_deref(), Some("Kralovehradecky kraj"));
    }

    #[test]
    fn the_county_comes_through_where_the_extract_has_one() {
        let found = gazetteer().nearest(49.1952, 16.608).unwrap();
        assert_eq!(found.name, "Brno");
        assert_eq!(found.subregion.as_deref(), Some("Okres Brno-mesto"));
        assert_eq!(found.region.as_deref(), Some("South Moravian"));
    }

    /// The direction has to read the way somebody would say it: a
    /// photograph east of a town is "east of" it, not "west of" it. Getting
    /// this backwards is the classic mistake and it is invisible until
    /// somebody who knows the place reads the caption.
    #[test]
    fn the_direction_is_from_the_place_to_the_photograph() {
        // A degree of longitude east of Prague, which is roughly 70 km.
        let found = gazetteer().nearest(50.088, 15.42).unwrap();
        assert_eq!(found.name, "Prague");
        assert_eq!(compass(found.bearing), "compass-east");
        assert!(
            (found.distance_km - 71.0).abs() < 5.0,
            "{} km",
            found.distance_km
        );
    }

    #[test]
    fn every_quarter_of_the_compass_reads_the_right_way() {
        let places = gazetteer();
        // A third of a degree out, which keeps Prague the nearest place in
        // every direction — this test is about the compass, not about which
        // city wins.
        for (latitude, longitude, expected) in [
            (50.388, 14.4208, "compass-north"),
            (49.788, 14.4208, "compass-south"),
            (50.088, 14.7208, "compass-east"),
            (50.088, 14.1208, "compass-west"),
        ] {
            let found = places.nearest(latitude, longitude).unwrap();
            assert_eq!(found.name, "Prague", "{latitude},{longitude}");
            assert_eq!(compass(found.bearing), expected, "{latitude},{longitude}");
        }
    }

    #[test]
    fn a_distant_place_is_not_where_the_photograph_was_taken() {
        let found = gazetteer().nearest(50.088, 15.42).unwrap();
        assert!(!found.is_here());
        // And it does not become a keyword either.
        assert!(!found.keywords(true).contains(&"Prague".to_owned()));
        assert_eq!(found.keywords(true), ["Kralovehradecky kraj", "Czechia"]);
    }

    #[test]
    fn a_shaky_position_leaves_the_place_out_and_keeps_the_country() {
        let found = gazetteer().nearest(50.0875, 14.4213).unwrap();
        assert!(found.keywords(true).contains(&"Prague".to_owned()));
        assert!(!found.keywords(false).contains(&"Prague".to_owned()));
        assert!(found.keywords(false).contains(&"Czechia".to_owned()));
    }

    #[test]
    fn a_name_is_not_repeated_when_the_region_shares_it() {
        let found = Nearby {
            name: "Paris".to_owned(),
            subregion: Some("Paris".to_owned()),
            region: Some("Ile-de-France".to_owned()),
            country: Some("France".to_owned()),
            distance_km: 1.0,
            bearing: 0,
        };
        assert_eq!(found.keywords(true), ["Paris", "Ile-de-France", "France"]);
    }

    /// The middle of an ocean has no answer, and inventing one would put a
    /// town from another continent into a caption.
    #[test]
    fn somewhere_with_nothing_near_it_has_no_answer() {
        assert!(gazetteer().nearest(0.0, -140.0).is_none());
    }

    #[test]
    fn coordinates_that_are_not_coordinates_have_no_answer() {
        let places = gazetteer();
        assert!(places.nearest(95.0, 14.0).is_none());
        assert!(places.nearest(50.0, 400.0).is_none());
    }

    /// A photograph in a suburb should say the city rather than the hamlet
    /// half a kilometre nearer.
    #[test]
    fn the_larger_place_wins_where_two_are_equally_close() {
        let places = Gazetteer::from_text(
            "1\tLittle Hamlet\tLittle Hamlet\t\t50.0800\t14.4200\tP\tPPL\tCZ\t\t52\t\t\t\t120\n\
             2\tBig City\tBig City\t\t50.0805\t14.4205\tP\tPPLC\tCZ\t\t52\t\t\t\t900000\n",
        );
        assert_eq!(places.nearest(50.0802, 14.4202).unwrap().name, "Big City");
    }

    #[test]
    fn a_folder_with_no_extract_in_it_is_not_an_error() {
        let empty = tempfile::tempdir().unwrap();
        assert!(Gazetteer::load(empty.path()).unwrap().is_none());
    }

    #[test]
    fn the_finest_extract_in_the_folder_is_the_one_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cities15000.txt"), PLACES).unwrap();
        std::fs::write(dir.path().join("cities1000.txt"), PLACES).unwrap();
        let places = Gazetteer::load(dir.path()).unwrap().unwrap();
        assert_eq!(places.source, "cities1000.txt");
    }

    #[test]
    fn the_names_are_read_when_they_are_there_and_missed_when_they_are_not() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cities1000.txt"), PLACES).unwrap();
        let bare = Gazetteer::load(dir.path()).unwrap().unwrap();
        let found = bare.nearest(50.0875, 14.4213).unwrap();
        assert_eq!(found.name, "Prague");
        assert_eq!(found.country, None, "a country code is not a country name");

        std::fs::write(dir.path().join("countryInfo.txt"), COUNTRIES).unwrap();
        let named = Gazetteer::load(dir.path()).unwrap().unwrap();
        assert_eq!(
            named.nearest(50.0875, 14.4213).unwrap().country.as_deref(),
            Some("Czechia")
        );
    }

    #[test]
    fn the_distance_is_the_one_on_the_ground() {
        // Prague to Brno, which is about 185 km.
        let far = distance_km(50.08804, 14.42076, 49.19522, 16.60796);
        assert!((far - 185.0).abs() < 5.0, "{far} km");
    }

    #[test]
    fn a_place_and_itself_are_no_distance_apart() {
        assert!(distance_km(50.0, 14.0, 50.0, 14.0) < 1e-9);
    }

    #[test]
    fn the_compass_answers_for_every_angle() {
        for degrees in 0..360 {
            let _ = compass(degrees);
        }

        assert_eq!(compass(0), "compass-north");
        assert_eq!(compass(90), "compass-east");
        assert_eq!(compass(180), "compass-south");
        assert_eq!(compass(270), "compass-west");
        assert_eq!(compass(360), "compass-north");
        assert_eq!(compass(-90), "compass-west");
    }
}
