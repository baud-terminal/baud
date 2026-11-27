//! Would merging the width and grapheme tries actually be smaller?
//!
//! Both index stage-1 by `cp >> 8`. A merged leaf needs the grapheme byte
//! (4 bits class, 4 bits lookback) plus width (2 bits) = 10 bits, so 2 bytes
//! per codepoint unless the bits can be recovered. This computes the real
//! block counts rather than assuming.
use baud_domain::{AmbiguousWidth, Codepoint, GraphemeClass, Lookback, Width};
use std::collections::HashMap;

#[test]
fn report_merged_trie_size() {
    let aw = AmbiguousWidth::Narrow;
    let mut wblocks: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut bblocks: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut mblocks: HashMap<Vec<u16>, usize> = HashMap::new();
    let mut lb_bits = 0u8;
    let mut classes = 0usize;

    for hi in 0..(0x110000 >> 8) {
        let (mut w, mut b, mut m) = (Vec::new(), Vec::new(), Vec::new());
        for lo in 0..256 {
            let cp = Codepoint::new(((hi << 8) | lo) as u32);
            let (wd, cl, lb) = match cp {
                Some(c) => {
                    let (cl, lb) = GraphemeClass::of_with_lookback(c);
                    (Width::of(c, aw), cl, lb)
                }
                None => (Width::NARROW, GraphemeClass::Other, Lookback::NONE),
            };
            let wcode = wd.cells() as u8;
            let ccode = cl as u8;
            let lcode = lb.bits();
            lb_bits |= lcode;
            classes = classes.max(ccode as usize);
            w.push(wcode);
            b.push((ccode & 0xf) | (lcode << 4));
            m.push(u16::from(ccode & 0xf) | (u16::from(lcode) << 4) | (u16::from(wcode) << 8));
        }
        let n = wblocks.len();
        wblocks.entry(w).or_insert(n);
        let n = bblocks.len();
        bblocks.entry(b).or_insert(n);
        let n = mblocks.len();
        mblocks.entry(m).or_insert(n);
    }
    // How many distinct (class, lookback, width) triples actually occur? If it
    // is 256 or fewer, a merged leaf is one byte indexing a palette -- which is
    // one lookup instead of two *and* smaller, where packing the fields is
    // neither.
    let mut triples: HashMap<(u8, u8, u8), usize> = HashMap::new();
    for cp in 0..0x110000u32 {
        if let Some(c) = Codepoint::new(cp) {
            let (cl, lb) = GraphemeClass::of_with_lookback(c);
            let n = triples.len();
            triples
                .entry((cl as u8, lb.bits(), Width::of(c, aw).cells() as u8))
                .or_insert(n);
        }
    }
    println!(
        "\n  distinct (class, lookback, width) triples: {}",
        triples.len()
    );
    println!("  fits a one-byte palette index: {}", triples.len() <= 256);
    let idx = 0x110000 >> 8;
    let cur = idx + wblocks.len() * 64 + idx + bblocks.len() * 256;
    let merged = idx + mblocks.len() * 512;
    println!("\n  distinct width blocks    {}", wblocks.len());
    println!("  distinct break blocks    {}", bblocks.len());
    println!("  distinct MERGED blocks   {}", mblocks.len());
    println!(
        "  lookback bits in use     {:#06b} ({} bits)",
        lb_bits,
        8 - lb_bits.leading_zeros()
    );
    println!("  max class index          {}", classes);
    println!("\n  current  two tries: {cur:>7} bytes");
    println!(
        "  merged, 2 B leaf  : {merged:>7} bytes   ({:+.0}%)",
        100.0 * (merged as f64 / cur as f64 - 1.0)
    );
    let packed = idx + mblocks.len() * 256;
    println!("  merged, 1 B leaf  : {packed:>7} bytes   (needs class+lookback+width in 8 bits)");
}
