//! Nativní dialog pro výběr složky.
//!
//! Celý modul existuje kvůli dvěma věcem, které se nedají obejít.
//!
//! **Dialog se zakládá na hlavním vlákně, ale čeká se na něj jinde.** macOS
//! umí ukázat panel přišpendlený k oknu jedině tehdy, když si o něj řekne
//! hlavní vlákno; odjinud spadne na modální okno uprostřed obrazovky, nebo
//! rovnou na paniku. Čekat na výsledek tam ale nesmíme: člověk prochází disk
//! klidně minutu a po celou tu dobu by se nepřekreslila jediná dlaždice —
//! Windows takové okno po pár sekundách označí za nereagující a zašedne mu
//! záhlaví. Budoucnost tedy vzniká tady a dokončí se ve vlákně vedle.
//!
//! **Otevřený smí být nejvýš jeden.** Ctrl+O zmáčknuté podruhé jinak postaví
//! druhý dialog nad první a jeden z nich zůstane viset i po výběru.

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// Dialog, který je právě otevřený.
#[derive(Debug)]
pub struct Picker {
    from_dialog: Receiver<Option<PathBuf>>,
}

/// Co dialog zatím řekl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Pořád je otevřený.
    Waiting,
    /// Zavřel se bez výběru. Nic se nestalo a nic se nehlásí — zrušit dialog
    /// je běžná odpověď, ne chyba.
    Cancelled,
    Picked(PathBuf),
}

/// Otevře dialog. Volat výhradně z hlavního vlákna, tedy zevnitř vykreslování.
pub fn ask(ctx: &egui::Context, title: String, start: Option<PathBuf>) -> Picker {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }

    // Tady, na hlavním vlákně. Přesunout tenhle řádek do vlákna níž vypadá
    // jako zjednodušení a na macOS to rozbije.
    let opened = dialog.pick_folder();
    let (to_ui, from_dialog) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let picked = pollster::block_on(opened);
        let path = picked.map(|handle| handle.path().to_path_buf());
        tracing::info!(vybrano = ?path, "dialog zavřen");
        // Chyba znamená jediné: okno se mezitím zavřelo a výsledek nemá kam jít.
        let _ = to_ui.send(path);
        // Bez tohohle by odpověď ležela v kanálu až do nejbližšího
        // překreslení, tedy podle `loading.idle_repaint_ms` klidně čtvrt
        // sekundy po tom, co člověk klikl na Vybrat.
        ctx.request_repaint();
    });

    Picker { from_dialog }
}

impl Picker {
    pub fn answer(&self) -> Answer {
        match self.from_dialog.try_recv() {
            Ok(Some(folder)) => Answer::Picked(folder),
            Ok(None) => Answer::Cancelled,
            Err(TryRecvError::Empty) => Answer::Waiting,
            // Vlákno skončilo, aniž by odpovědělo. Brát to jako „pořád
            // otevřený" by znamenalo, že Ctrl+O už do konce běhu nezabere.
            Err(TryRecvError::Disconnected) => {
                tracing::warn!("dialog skončil bez odpovědi");
                Answer::Cancelled
            }
        }
    }
}

/// Kde dialog otevřít.
///
/// Nejlepší je složka, na kterou se člověk právě dívá; když ji mezitím někdo
/// smazal nebo odpojil disk, tak nejbližší existující nadřazená. Začít o patro
/// výš je pořád blíž než tam, kam dialog skočí sám. Když není otevřená žádná,
/// vezme se ta poslední z nastavení — po restartu je to jediná stopa, kterou
/// o člověku máme.
pub fn start_dir(current: Option<&Path>, last: Option<&str>) -> Option<PathBuf> {
    let wanted = current
        .map(Path::to_path_buf)
        .or_else(|| last.filter(|text| !text.is_empty()).map(PathBuf::from))?;

    wanted
        .ancestors()
        .find(|path| path.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zacne_tam_kde_prave_jsme() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            start_dir(Some(dir.path()), None).as_deref(),
            Some(dir.path())
        );
    }

    #[test]
    fn po_smazane_slozce_se_jde_o_patro_vys() {
        let dir = tempfile::tempdir().unwrap();
        let pryc = dir.path().join("2019").join("léto");
        assert_eq!(start_dir(Some(&pryc), None).as_deref(), Some(dir.path()));
    }

    #[test]
    fn bez_otevrene_slozky_se_vezme_posledni() {
        let dir = tempfile::tempdir().unwrap();
        let last = dir.path().to_string_lossy().into_owned();
        assert_eq!(start_dir(None, Some(&last)).as_deref(), Some(dir.path()));
    }

    #[test]
    fn otevrena_slozka_ma_prednost_pred_posledni() {
        let dir = tempfile::tempdir().unwrap();
        let jinam = tempfile::tempdir().unwrap();
        let last = jinam.path().to_string_lossy().into_owned();
        assert_eq!(
            start_dir(Some(dir.path()), Some(&last)).as_deref(),
            Some(dir.path())
        );
    }

    #[test]
    fn kdyz_neni_nic_rozhodne_se_dialog_sam() {
        // `None` znamená „neříkej mu nic", ne „začni v kořeni disku".
        assert_eq!(start_dir(None, None), None);
        assert_eq!(start_dir(None, Some("")), None);
        assert_eq!(start_dir(None, Some("Q:/disk, který tu není/fotky")), None);
    }
}
