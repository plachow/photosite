//! Rozložení doků.
//!
//! Plochy aplikace nejsou v kreslicí vrstvě zadrátované vedle sebe, ale
//! popsané **stromem, který je daty**. Dnešní uspořádání je jen jeho výchozí
//! hodnota; „informace o fotce pod náhledem" je změna jednoho řetězce, ne
//! zásah do kreslení.
//!
//! ```text
//! h(0.16, tree, h(0.66, gallery, v(0.62, preview, info)))
//!  │      │                       └ svisle: náhled nahoře, informace pod ním
//!  │      └ vedle sebe: strom vlevo, zbytek vpravo
//!  └ podíl první části; druhá dostane, co zbude
//! ```
//!
//! Tvar je textový schválně. Do nastavení jde jedním řádkem, dá se přečíst
//! i ručně opravit a diff je vidět na první pohled — zanořené tabulky v TOML
//! by na třech úrovních zanoření byly nečitelné.
//!
//! **Dok se nesmí dát zavřít omylem.** Každý má nejmenší velikost a dělítko
//! pod ni nepustí. Než to platilo, šel náhledový panel přetáhnout na osm
//! pixelů, uložilo se to do nastavení a zpátky ho nedostalo nic: klikání na
//! dlaždice fungovalo dál, jen nebylo kam kreslit.

use std::collections::HashSet;
use std::fmt;

/// Jedna plocha, kterou lze do rozložení postavit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dock {
    /// Stabilní klíč. Do nastavení jde tenhle, ne název — název se smí
    /// kdykoliv přeložit.
    pub id: &'static str,
    /// Klíč do překladu. Ani tady nejsou texty.
    pub title_key: &'static str,
    /// Nejmenší velikost v bodech. Dělítko pod ni nepustí.
    pub min: f64,
}

pub const DOCKS: &[Dock] = &[
    Dock {
        id: "tree",
        title_key: "dock-tree",
        min: 120.0,
    },
    Dock {
        id: "gallery",
        title_key: "dock-gallery",
        min: 240.0,
    },
    Dock {
        id: "preview",
        title_key: "dock-preview",
        min: 160.0,
    },
    Dock {
        id: "info",
        title_key: "dock-info",
        min: 90.0,
    },
];

pub fn dock(id: &str) -> Option<&'static Dock> {
    DOCKS.iter().find(|dock| dock.id == id)
}

/// Výchozí rozložení: strom vlevo, mřížka uprostřed, náhled a informace
/// ve sloupci vpravo.
pub const DEFAULT: &str = "h(0.16, tree, h(0.66, gallery, v(0.62, preview, info)))";

/// Jak jsou obě části poskládané.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Vedle sebe, dělítko je svislé.
    Across,
    /// Pod sebou, dělítko je vodorovné.
    Down,
}

impl Axis {
    fn tag(self) -> char {
        match self {
            Axis::Across => 'h',
            Axis::Down => 'v',
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Pane(String),
    Split {
        axis: Axis,
        /// Podíl první části, 0 až 1.
        ratio: f64,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Layout::Pane(id) => f.write_str(id),
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => write!(f, "{}({ratio:.2}, {first}, {second})", axis.tag()),
        }
    }
}

impl Layout {
    /// Všechny plochy zleva doprava, shora dolů.
    pub fn panes(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }

    fn collect<'a>(&'a self, into: &mut Vec<&'a str>) {
        match self {
            Layout::Pane(id) => into.push(id),
            Layout::Split { first, second, .. } => {
                first.collect(into);
                second.collect(into);
            }
        }
    }

    /// Je v téhle větvi vidět aspoň něco? Větev, ze které je všechno schované,
    /// nedostane místo ani dělítko.
    pub fn visible(&self, hidden: &HashSet<&str>) -> bool {
        match self {
            Layout::Pane(id) => !hidden.contains(id.as_str()),
            Layout::Split { first, second, .. } => first.visible(hidden) || second.visible(hidden),
        }
    }

    /// Nejmenší rozumná velikost podél osy, i s dělítky uvnitř.
    ///
    /// Podél téže osy se sčítá, napříč se bere to větší — dva doky pod sebou
    /// potřebují každý svou výšku, ale šířku sdílejí.
    pub fn min_along(&self, axis: Axis, hidden: &HashSet<&str>, splitter: f64) -> f64 {
        match self {
            Layout::Pane(id) => {
                if hidden.contains(id.as_str()) {
                    0.0
                } else {
                    dock(id).map(|dock| dock.min).unwrap_or(0.0)
                }
            }
            Layout::Split {
                axis: split,
                first,
                second,
                ..
            } => {
                if !first.visible(hidden) {
                    return second.min_along(axis, hidden, splitter);
                }

                if !second.visible(hidden) {
                    return first.min_along(axis, hidden, splitter);
                }

                let a = first.min_along(axis, hidden, splitter);
                let b = second.min_along(axis, hidden, splitter);
                if *split == axis {
                    a + b + splitter
                } else {
                    a.max(b)
                }
            }
        }
    }

    /// Podíl tak, aby se obě části vešly nad svoje minimum.
    ///
    /// Když je místa málo na obojí, vyhraje uložený poměr — jinak by se dok
    /// při zmenšování okna přilepil k okraji a už se nepustil.
    pub fn clamp_ratio(first_min: f64, second_min: f64, total: f64, ratio: f64) -> f64 {
        let ratio = ratio.clamp(0.0, 1.0);
        if total <= 0.0 {
            return ratio;
        }

        let low = (first_min / total).clamp(0.0, 1.0);
        let high = (1.0 - second_min / total).clamp(0.0, 1.0);
        if low > high {
            return ratio;
        }

        ratio.clamp(low, high)
    }

    /// Přepíše podíl na uzlu dané cesty. Cesta je posloupnost odboček:
    /// `false` je první část, `true` druhá.
    pub fn set_ratio(&mut self, path: &[bool], value: f64) {
        let Layout::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return;
        };

        match path.split_first() {
            None => *ratio = value.clamp(0.0, 1.0),
            Some((true, rest)) => second.set_ratio(rest, value),
            Some((false, rest)) => first.set_ratio(rest, value),
        }
    }
}

// --------------------------------------------------------------------- čtení

/// Přečte rozložení. Chyba nese důvod, ať je v logu vidět, co je špatně.
pub fn parse(text: &str) -> Result<Layout, String> {
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
    };
    let layout = reader.layout()?;
    reader.space();
    if reader.at < reader.text.len() {
        return Err(format!("přebývá text od znaku {}", reader.at));
    }

    check(&layout)?;
    Ok(layout)
}

/// Rozložení z nastavení, nebo výchozí, když je pokažené.
///
/// Nečitelné rozložení nesmí shodit aplikaci ani ji nechat bez mřížky. Že se
/// spadlo na výchozí, jde do logu — tiše se to stát nesmí.
pub fn parse_or_default(text: &str) -> Layout {
    let text = if text.trim().is_empty() {
        DEFAULT
    } else {
        text
    };
    match parse(text) {
        Ok(layout) => layout,
        Err(duvod) => {
            tracing::warn!(
                rozlozeni = text,
                duvod,
                "rozložení nedává smysl, beru výchozí"
            );
            parse(DEFAULT).expect("výchozí rozložení musí být platné")
        }
    }
}

fn check(layout: &Layout) -> Result<(), String> {
    let panes = layout.panes();
    for id in &panes {
        if dock(id).is_none() {
            return Err(format!("neznámá plocha {id}"));
        }
    }

    let mut seen = HashSet::new();
    for id in &panes {
        if !seen.insert(*id) {
            return Err(format!("plocha {id} je v rozložení dvakrát"));
        }
    }

    // Mřížka je důvod, proč aplikace existuje. Rozložení bez ní je překlep.
    if !seen.contains("gallery") {
        return Err("rozložení neobsahuje mřížku".to_owned());
    }

    Ok(())
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, want: u8) -> Result<(), String> {
        if self.peek() != Some(want) {
            return Err(format!("na znaku {} chybí {}", self.at, want as char));
        }

        self.at += 1;
        Ok(())
    }

    fn word(&mut self) -> Result<String, String> {
        self.space();
        let from = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_alphanumeric() || self.text[self.at] == b'_')
        {
            self.at += 1;
        }

        if from == self.at {
            return Err(format!("na znaku {} chybí název plochy", self.at));
        }

        Ok(String::from_utf8_lossy(&self.text[from..self.at]).into_owned())
    }

    fn number(&mut self) -> Result<f64, String> {
        self.space();
        let from = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_digit() || self.text[self.at] == b'.')
        {
            self.at += 1;
        }

        String::from_utf8_lossy(&self.text[from..self.at])
            .parse()
            .map_err(|_| format!("na znaku {from} chybí podíl"))
    }

    fn layout(&mut self) -> Result<Layout, String> {
        let word = self.word()?;
        let axis = match word.as_str() {
            "h" => Some(Axis::Across),
            "v" => Some(Axis::Down),
            _ => None,
        };

        // `h` a `v` jsou dělení jen tehdy, když za nimi stojí závorka.
        match axis.filter(|_| self.peek() == Some(b'(')) {
            None => Ok(Layout::Pane(word)),
            Some(axis) => {
                self.eat(b'(')?;
                let ratio = self.number()?;
                self.eat(b',')?;
                let first = self.layout()?;
                self.eat(b',')?;
                let second = self.layout()?;
                self.eat(b')')?;
                Ok(Layout::Split {
                    axis,
                    ratio: ratio.clamp(0.0, 1.0),
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }
}

// ------------------------------------------------------------ co je schované

/// Schované plochy z nastavení. Neznámé jméno se zahodí, ne aby kvůli němu
/// zmizelo všechno ostatní.
pub fn hidden(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| dock(id).is_some())
        .map(str::to_owned)
        .collect()
}

/// Zpátky do tvaru pro nastavení, v pořadí registru — ať se soubor nemění jen
/// proto, že se něco zaplo a zase vyplo.
pub fn hidden_to_text(list: &[String]) -> String {
    DOCKS
        .iter()
        .map(|dock| dock.id)
        .filter(|id| list.iter().any(|hidden| hidden == id))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nic() -> HashSet<&'static str> {
        HashSet::new()
    }

    #[test]
    fn vychozi_rozlozeni_je_platne() {
        let layout = parse(DEFAULT).expect("výchozí rozložení musí projít");
        assert_eq!(layout.panes(), vec!["tree", "gallery", "preview", "info"]);
    }

    #[test]
    fn kazda_plocha_v_registru_ma_preklad() {
        for dock in DOCKS {
            assert!(
                crate::i18n::has(dock.title_key),
                "plocha {} odkazuje na chybějící klíč {}",
                dock.id,
                dock.title_key
            );
        }
    }

    #[test]
    fn plochy_se_nejmenuji_jako_deleni() {
        // `h` a `v` jsou v zápisu vyhrazené; plocha s takovým jménem by se
        // nedala od dělení odlišit.
        for dock in DOCKS {
            assert!(dock.id != "h" && dock.id != "v", "{}", dock.id);
        }
    }

    #[test]
    fn zapis_a_cteni_se_potkaji() {
        for text in [
            DEFAULT,
            "gallery",
            "v(0.50, gallery, info)",
            "h(0.30, v(0.50, tree, info), gallery)",
        ] {
            let layout = parse(text).unwrap();
            let znovu = layout.to_string();
            assert_eq!(parse(&znovu).unwrap(), layout, "{text} → {znovu}");
        }
    }

    #[test]
    fn nesmysl_neshodi_aplikaci_a_necha_mrizku() {
        for text in [
            "",
            "h(0.5, gallery",
            "h(gallery, info)",
            "neznamo",
            "h(0.5, gallery, gallery)",
            "h(0.5, tree, info)",
            "((((",
        ] {
            let layout = parse_or_default(text);
            assert!(
                layout.panes().contains(&"gallery"),
                "{text} nechalo rozložení bez mřížky"
            );
        }
    }

    #[test]
    fn dvakrat_tataz_plocha_je_chyba() {
        assert!(parse("h(0.5, gallery, gallery)").is_err());
    }

    #[test]
    fn rozlozeni_bez_mrizky_je_chyba() {
        assert!(parse("h(0.5, tree, preview)").is_err());
    }

    #[test]
    fn minimum_se_podel_osy_scita_a_napric_bere_vetsi() {
        let layout = parse("h(0.5, tree, gallery)").unwrap();
        let tree = dock("tree").unwrap().min;
        let gallery = dock("gallery").unwrap().min;
        assert_eq!(
            layout.min_along(Axis::Across, &nic(), 6.0),
            tree + gallery + 6.0
        );
        assert_eq!(layout.min_along(Axis::Down, &nic(), 6.0), tree.max(gallery));
    }

    #[test]
    fn schovana_plocha_si_misto_nedrzi() {
        let layout = parse("h(0.5, tree, gallery)").unwrap();
        let hidden = HashSet::from(["tree"]);
        assert_eq!(
            layout.min_along(Axis::Across, &hidden, 6.0),
            dock("gallery").unwrap().min,
            "schovaný dok nesmí brát místo ani si účtovat dělítko"
        );
    }

    #[test]
    fn delitko_nepusti_dok_pod_jeho_minimum() {
        // Přesně tohle šlo dřív: náhled přetažený na osm pixelů, uložený do
        // nastavení, a zpátky ho nedostalo nic.
        let ratio = Layout::clamp_ratio(240.0, 160.0, 1000.0, 0.99);
        assert!(ratio * 1000.0 <= 840.0 + 1e-9);
        assert!(1000.0 - ratio * 1000.0 >= 160.0 - 1e-9);

        let ratio = Layout::clamp_ratio(240.0, 160.0, 1000.0, 0.0);
        assert!(ratio * 1000.0 >= 240.0 - 1e-9);
    }

    #[test]
    fn v_tesnem_okne_rozhoduje_pomer_a_ne_minima() {
        // Když se obojí nevejde, nesmí se podíl zaseknout na kraji.
        let ratio = Layout::clamp_ratio(600.0, 600.0, 500.0, 0.4);
        assert!((ratio - 0.4).abs() < 1e-9);
    }

    #[test]
    fn podil_se_da_prepsat_podle_cesty() {
        let mut layout = parse(DEFAULT).unwrap();
        layout.set_ratio(&[true, true], 0.25);
        let Layout::Split { second, .. } = &layout else {
            panic!("výchozí rozložení má být dělení")
        };
        let Layout::Split { second, .. } = second.as_ref() else {
            panic!("druhá část má být dělení")
        };
        let Layout::Split { ratio, .. } = second.as_ref() else {
            panic!("náhled a informace mají být dělení")
        };
        assert_eq!(*ratio, 0.25);
    }

    #[test]
    fn schovane_plochy_tam_a_zpatky() {
        assert_eq!(hidden("info,preview"), vec!["info", "preview"]);
        assert_eq!(hidden(" info , , neznamo "), vec!["info"]);
        assert_eq!(hidden_to_text(&hidden("info,preview")), "preview,info");
        assert_eq!(hidden_to_text(&[]), "");
    }
}
