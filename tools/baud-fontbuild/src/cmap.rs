//! cmap: every encoding record, its subtable's format and language, and
//! the whole mapping.

use std::collections::BTreeMap;

use read_fonts::tables::cmap::{CmapSubtable, MapVariant};
use read_fonts::{FontRef, ReadError, TableProvider};

use crate::dump::{Dump, Tokens};

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A mapping to glyph 0 is dropped: it is the missing glyph, which an
/// unmapped character gets anyway, and a format 4 subtable spells its
/// closing 0xFFFF segment that way.
pub fn dump(font: &FontRef) -> Result<Dump, ReadError> {
    let cmap = font.cmap()?;
    let mut d = Dump::default();
    d.put("version", cmap.version().to_string());
    for (k, record) in cmap.encoding_records().iter().enumerate() {
        let head = format!(
            "platform={} encoding={}",
            record.platform_id() as u16,
            record.encoding_id()
        );
        let sub = match record.subtable(cmap.offset_data()) {
            Ok(s) => s,
            Err(e) => {
                d.put(format!("record {k}"), format!("{head} unreadable:{e}"));
                continue;
            }
        };
        let format = format_of(&sub);
        d.put(
            format!("record {k}"),
            format!("{head} format={format} language={}", sub.language()),
        );
        let mut t = Tokens::default();
        match &sub {
            CmapSubtable::Format14(uvs) => {
                for (cp, selector, variant) in uvs.iter() {
                    t.push(match variant {
                        MapVariant::UseDefault => format!("U+{cp:04X}+U+{selector:04X}>default"),
                        MapVariant::Variant(g) => {
                            format!("U+{cp:04X}+U+{selector:04X}>{}", g.to_u32())
                        }
                    });
                }
            }
            CmapSubtable::Format2(_) | CmapSubtable::Format8(_) | CmapSubtable::Format15(_) => {
                let bytes = sub.offset_data().as_bytes();
                t.push(format!("not-expanded:fnv64={:016x}", fnv(bytes)));
            }
            _ => {
                let mut map = BTreeMap::new();
                for (cp, g) in sub.iter() {
                    if g.to_u32() != 0 {
                        map.entry(cp).or_insert(g.to_u32());
                    }
                }
                for (cp, g) in map {
                    t.push(format!("U+{cp:04X}>{g}"));
                }
            }
        }
        d.put(format!("record {k} map"), t.done());
    }
    Ok(d)
}

fn format_of(sub: &CmapSubtable) -> u16 {
    match sub {
        CmapSubtable::Format0(_) => 0,
        CmapSubtable::Format2(_) => 2,
        CmapSubtable::Format4(_) => 4,
        CmapSubtable::Format6(_) => 6,
        CmapSubtable::Format8(_) => 8,
        CmapSubtable::Format10(_) => 10,
        CmapSubtable::Format12(_) => 12,
        CmapSubtable::Format13(_) => 13,
        CmapSubtable::Format14(_) => 14,
        CmapSubtable::Format15(_) => 15,
    }
}
