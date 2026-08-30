//! Odolnost parserů proti odpadu.
//!
//! Reálná knihovna je plná souborů, které nejsou tím, co tvrdí jejich
//! přípona — ve zkušební sadě o 57 606 fotkách byly takové tři. Tenhle test
//! nekontroluje, že se něco přečte správně. Kontroluje jedinou věc, na které
//! záleží: **že to nespadne.**

use photosite_image::exif;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Libovolné bajty. Nejčastější reálný případ: soubor je něco úplně jiného.
    #[test]
    fn cokoliv_neshodi_ctecku(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = exif::read(&bytes);
    }

    /// Bajty, které začínají jako JPEG. Tady se parser dostane hlouběji, takže
    /// je tahle varianta zajímavější než čistě náhodná.
    #[test]
    fn tvari_se_jako_jpeg(rest in prop::collection::vec(any::<u8>(), 0..4096)) {
        let mut raw = vec![0xFF_u8, 0xD8, 0xFF, 0xE1];
        raw.extend_from_slice(&rest);
        let _ = exif::read(&raw);
    }

    /// Poškozené APP1: správná hlavička, náhodné tělo. Sem parser vleze úplně
    /// a čte offsety z dat, kterým nesmí věřit.
    #[test]
    fn poskozeny_app1(len in any::<u16>(), body in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut raw = vec![0xFF_u8, 0xD8, 0xFF, 0xE1];
        raw.extend_from_slice(&len.to_be_bytes());
        raw.extend_from_slice(b"Exif\x00\x00");
        raw.extend_from_slice(&body);
        let meta = exif::read(&raw);
        // Když parser náhled ohlásí, musí ležet uvnitř souboru — jinak by na
        // něm volající spadl místo něj.
        if let Some(thumbnail) = meta.thumbnail {
            prop_assert!(thumbnail.offset + thumbnail.len <= raw.len());
        }

        prop_assert!((1..=8).contains(&meta.orientation));
    }
}
