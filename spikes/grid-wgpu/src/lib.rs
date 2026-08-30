//! Formát balíku náhledů. Sdílený packerem a prohlížečem.
//!
//! Dva soubory a index. Celý smysl je, aby prohlížeč po startu nemusel sáhnout
//! na jediný z 57 606 původních JPEGů:
//!
//! * `mips.pack`  — rezidentní vrstva, zapsaná **rovnou v rozvržení atlasu**,
//!   takže start je `write_texture` z mmapu a nula práce na CPU.
//! * `thumbs.pack` — ostré 256px náhledy jako JPEG, jeden souvislý soubor.
//! * `index.bin`  — hlavička a pole záznamů pevné délky.

use bytemuck::{Pod, Zeroable};

pub const MAGIC: [u8; 8] = *b"PSGRID01";

/// Ostrý náhled; 256 px se na 4K stránku vejde 16×16krát.
pub const THUMB_SIZE: u32 = 256;
/// Rezidentní vrstva. 32 px je kompromis — 57 606 fotek vyjde na 4 stránky,
/// tedy 268 MB VRAM. Při 64 px by to byly 4× tolik.
pub const MIP_SIZE: u32 = 32;
/// Hrana atlasové stránky.
pub const ATLAS_PAGE: u32 = 4096;

pub const MIPS_PER_ROW: u32 = ATLAS_PAGE / MIP_SIZE;
pub const MIPS_PER_PAGE: u32 = MIPS_PER_ROW * MIPS_PER_ROW;
pub const THUMBS_PER_ROW: u32 = ATLAS_PAGE / THUMB_SIZE;
pub const THUMBS_PER_PAGE: u32 = THUMBS_PER_ROW * THUMBS_PER_ROW;
pub const PAGE_BYTES: usize = (ATLAS_PAGE as usize) * (ATLAS_PAGE as usize) * 4;

/// Jeden záznam indexu, 24 bajtů. `tw`/`th` je skutečný rozměr náhledu uvnitř
/// buňky 256×256 (náhled se do buňky vejde se zachovaným poměrem stran a sedí
/// v levém horním rohu), `mw`/`mh` totéž pro mip.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Entry {
    pub offset: u64,
    pub len: u32,
    pub tw: u16,
    pub th: u16,
    pub mw: u8,
    pub mh: u8,
    pub pad: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Header {
    pub magic: [u8; 8],
    pub version: u32,
    pub count: u32,
    pub thumb_size: u32,
    pub mip_size: u32,
    pub atlas_page: u32,
    pub pad: u32,
}

impl Header {
    pub const BYTES: usize = std::mem::size_of::<Header>();

    pub fn new(count: u32) -> Self {
        Self {
            magic: MAGIC,
            version: 1,
            count,
            thumb_size: THUMB_SIZE,
            mip_size: MIP_SIZE,
            atlas_page: ATLAS_PAGE,
            pad: 0,
        }
    }
}

/// Počet stránek rezidentního atlasu pro daný počet fotek.
pub fn mip_pages(count: u32) -> u32 {
    count.div_ceil(MIPS_PER_PAGE).max(1)
}

/// Kam v rezidentním atlasu padne fotka číslo `i`: stránka a buňka v ní.
pub fn mip_slot(i: u32) -> (u32, u32, u32) {
    let page = i / MIPS_PER_PAGE;
    let cell = i % MIPS_PER_PAGE;
    (page, cell % MIPS_PER_ROW, cell / MIPS_PER_ROW)
}

/// Kam v ostrém atlasu padne buňka číslo `cell`: stránka a pozice v ní.
pub fn thumb_slot(cell: u32) -> (u32, u32, u32) {
    let page = cell / THUMBS_PER_PAGE;
    let in_page = cell % THUMBS_PER_PAGE;
    (page, in_page % THUMBS_PER_ROW, in_page / THUMBS_PER_ROW)
}

/// Přečte `index.bin` na hlavičku a záznamy.
pub fn read_index(path: &std::path::Path) -> anyhow::Result<(Header, Vec<Entry>)> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.len() >= Header::BYTES, "index.bin je useknutý");
    let header: Header = *bytemuck::from_bytes(&bytes[..Header::BYTES]);
    anyhow::ensure!(header.magic == MAGIC, "index.bin má cizí magic");
    let entries: &[Entry] = bytemuck::cast_slice(&bytes[Header::BYTES..]);
    anyhow::ensure!(
        entries.len() == header.count as usize,
        "index.bin slibuje {} záznamů, obsahuje {}",
        header.count,
        entries.len()
    );
    Ok((header, entries.to_vec()))
}
