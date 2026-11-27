//! The sequence table, `OUT_DIR/sequence_data.rs`: the sequences the
//! Unicode Standard names, as a trie the cluster store steps through one
//! codepoint at a time -- UTS #51's RGI emoji sequences
//! (`[[source: emoji17-sequences]]`, `[[source: emoji17-zwj-sequences]]`)
//! and the standardized emoji variation sequences
//! (`[[source: emoji-variation-sequences]]`). Every prefix of one is a
//! node, up to a base and [`CAPACITY`] continuations, the most a cluster
//! slot holds.
//!
//! An edge -- a parent cell's content word and the next codepoint -- is
//! found by a perfect hash built here: one bucket seed, one entry, one
//! compare. No probe sequence and no search at run time. Its buckets are
//! placed by `baud_tablegen::place`, the fullest first, each taking the
//! first seed from 0 up that places its keys.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd;
use baud_tablegen::{
    Buckets, Check, Emit, Errors, Input, Kind, PerLine, Slot, Slots, Source, ValueSet, place,
};
use std::collections::{HashMap, HashSet};

pub const SEQUENCES: Input = Input {
    file: "emoji17-sequences.txt",
    key: "emoji17-sequences",
    header: "# emoji-sequences.txt",
};

pub const ZWJ_SEQUENCES: Input = Input {
    file: "emoji17-zwj-sequences.txt",
    key: "emoji17-zwj-sequences",
    header: "# emoji-zwj-sequences.txt",
};

pub const VARIATION_SEQUENCES: Input = Input {
    file: "unicode-emoji-variation-sequences.txt",
    key: "emoji-variation-sequences",
    header: "# emoji-variation-sequences.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 3] = [SEQUENCES, ZWJ_SEQUENCES, VARIATION_SEQUENCES];

/// Each file's `type_field` values (UTS #51, `[[source: uts51-emoji]]`), and
/// the sequences of two codepoints or more it lists.
const TYPES: [(&[&str], usize); 3] = [
    (
        &[
            "Basic_Emoji",
            "Emoji_Keycap_Sequence",
            "RGI_Emoji_Flag_Sequence",
            "RGI_Emoji_Tag_Sequence",
            "RGI_Emoji_Modifier_Sequence",
        ],
        1146,
    ),
    (&["RGI_Emoji_ZWJ_Sequence"], 1614),
    (&["text style", "emoji style"], 742),
];

/// The distinct sequences of the three files together: a basic emoji with
/// its presentation selector is in the first and the third.
const DISTINCT: usize = 3295;

/// `ClusterSlot::CAPACITY`: continuations after the base.
pub const CAPACITY: usize = 8;

/// `Content::CLUSTERS` and `ClusterId::SEQUENCES`: a node's cell word is
/// CLUSTERS + SEQUENCES + node. The const asserts at the foot of
/// `crates/baud-domain/src/sequence.rs` (WORD0 == Content::cluster_word(
/// ClusterId::SEQUENCES), NODES <= 0x7FFF) hold these to the Rust
/// constants.
const CLUSTERS: u32 = 0x0012_0000;
const SEQUENCE_IDS: u32 = 0x8000;
pub const WORD0: u32 = CLUSTERS + SEQUENCE_IDS;
// A key is the parent's word above the codepoint, 21 bits each.
const _: () = assert!(WORD0 + 0x7FFF < 1 << 21);

/// The perfect hash: 2^BUCKET_BITS seeds, 2^EDGE_BITS entries.
pub const BUCKET_BITS: u32 = 11;
pub const EDGE_BITS: u32 = 13;
pub const M1: u64 = 0x9E37_79B9_7F4A_7C15;
pub const M2: u64 = 0xD6E8_FEB8_6659_FD93;

pub fn bucket(key: u64) -> usize {
    usize::try_from(key.wrapping_mul(M1) >> (64 - BUCKET_BITS)).expect("11 bits")
}

pub fn slot(key: u64, seed: u16) -> Slot {
    let at = (key ^ u64::from(seed)).wrapping_mul(M2) >> (64 - EDGE_BITS);
    Slot(usize::try_from(at).expect("13 bits"))
}

/// The sequences of two codepoints or more `src` lists, each row's type
/// checked against `types`; a sequence listed twice is refused, and a range
/// row -- single codepoints, no continuation -- is read and left out.
fn sequences(src: &Source, types: &[&str], errs: &mut Errors) -> Vec<Vec<u32>> {
    let names = ValueSet::closed(&format!("{} type_field", src.path()), "UTS #51", types);
    let mut out = Vec::new();
    let mut seen: HashMap<Vec<u32>, usize> = HashMap::new();
    for row in src.rows() {
        if errs.take(row.expect(3)).is_none() || errs.take(names.check(&row, 1)).is_none() {
            continue;
        }
        if row.field(0).contains("..") {
            errs.take(row.range(0));
            continue;
        }
        let Some(cps) = errs.take(row.code_points(0, 0)) else {
            continue;
        };
        if cps.len() < 2 {
            continue;
        }
        if let Some(line) = seen.get(&cps) {
            errs.push(row.error(
                Kind::Duplicate,
                0,
                format!("listed already on line {line} ({:?})", src.line(*line)),
            ));
            continue;
        }
        seen.insert(cps.clone(), row.line());
        out.push(cps);
    }
    out
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok(files) = <[Source; 3]>::try_from(loaded) else {
        return Err(errs);
    };
    let mut seqs: HashSet<Vec<u32>> = HashSet::new();
    for (src, (types, want)) in files.iter().zip(TYPES) {
        ucd::emoji_version(src, &mut errs);
        let listed = sequences(src, types, &mut errs);
        errs.count(
            src.path(),
            "sequences of two codepoints or more",
            listed.len(),
            want,
        );
        seqs.extend(listed);
    }
    errs.count(
        files[0].path(),
        "distinct sequences in the three files",
        seqs.len(),
        DISTINCT,
    );
    if !errs.is_empty() {
        return Err(errs);
    }

    // Every prefix of two codepoints or more, up to a base and CAPACITY
    // continuations, by length and then codepoints.
    let mut nodes: Vec<Vec<u32>> = seqs
        .iter()
        .flat_map(|s| (2..=s.len().min(1 + CAPACITY)).map(|n| s[..n].to_vec()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    nodes.sort_by(|a, b| (a.len(), a).cmp(&(b.len(), b)));
    let table = &files[0];
    errs.require(Check::from(nodes.len() <= 0x7FFF), || {
        table.error(
            Kind::Invariant,
            format!("{} nodes do not fit 15 bits", nodes.len()),
        )
    });
    let index: HashMap<&[u32], usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_slice(), i))
        .collect();

    // Each node's codepoints, as a span of one flat array: a node shares
    // the span of the first longest sequence it begins.
    let parents: HashSet<&[u32]> = nodes.iter().map(|s| &s[..s.len() - 1]).collect();
    let mut leaves: Vec<&Vec<u32>> = nodes
        .iter()
        .filter(|s| !parents.contains(s.as_slice()))
        .collect();
    leaves.sort();
    let mut flat: Vec<u32> = Vec::new();
    let mut span: HashMap<&[u32], usize> = HashMap::new();
    for s in leaves {
        let off = flat.len();
        flat.extend(s);
        for n in 2..=s.len() {
            span.entry(&s[..n]).or_insert(off);
        }
    }
    errs.require(
        Check::from(nodes.iter().all(|s| span.contains_key(s.as_slice()))),
        || table.error(Kind::Invariant, "a node lies in no span"),
    );
    errs.require(Check::from(flat.len() < 1 << 16), || {
        table.error(
            Kind::Invariant,
            format!("{} span codepoints do not fit 16 bits", flat.len()),
        )
    });
    if !errs.is_empty() {
        return Err(errs);
    }

    // The content word of the cell a continuation attaches to.
    let word = |parent: &[u32]| -> u64 {
        if parent.len() == 1 {
            u64::from(parent[0])
        } else {
            u64::from(WORD0) + u64::try_from(index[parent]).expect("15 bits")
        }
    };
    let keys: Vec<u64> = nodes
        .iter()
        .map(|s| (word(&s[..s.len() - 1]) << 21) | u64::from(s[s.len() - 1]))
        .collect();
    errs.require(
        Check::from(keys.iter().collect::<HashSet<_>>().len() == keys.len()),
        || table.error(Kind::Duplicate, "two edges have one key"),
    );
    let buckets: Vec<usize> = keys.iter().map(|&k| bucket(k)).collect();
    let Some(hash) = errs.take(place(
        "sequence edges",
        &buckets,
        Buckets(1 << BUCKET_BITS),
        Slots(1 << EDGE_BITS),
        0..=u16::MAX,
        |k, seed| slot(keys[k], seed),
    )) else {
        return Err(errs);
    };
    // The key above the node's number plus one, so an empty entry, zero,
    // matches no key.
    let edges: Vec<u64> = hash
        .slots
        .iter()
        .map(|s| {
            s.map_or(0, |n| {
                (keys[n] << 16) | (u64::try_from(n).expect("15 bits") + 1)
            })
        })
        .collect();

    // The blocks (codepoint >> 8) any continuation in a sequence falls in:
    // a continuation outside them -- a Hebrew point, a Devanagari sign, a
    // kana voicing mark -- cannot step the trie, and one bit says so.
    let mut continues = vec![0u8; 0x1100 / 8];
    for s in &nodes {
        let b = usize::try_from(s[s.len() - 1] >> 8).expect("a block");
        continues[b >> 3] |= 1 << (b & 7);
    }
    let packed: Vec<u32> = nodes
        .iter()
        .map(|s| {
            u32::try_from(span[s.as_slice()]).expect("16 bits")
                | (u32::try_from(s.len()).expect("a short node") << 16)
        })
        .collect();

    let mut out = Emit::new(
        "sequence_data",
        "crates/baud-domain/build.rs (build/sequence.rs)",
        &[&files[0], &files[1], &files[2]],
    );
    out.code(&format!(
        "// {} sequences of two codepoints or more, {} prefixes of a base and up\n\
         // to {CAPACITY} continuations, {} codepoints of spans.\n\
         \n\
         /// The first node's cell word: `Content::CLUSTERS + ClusterId::SEQUENCES`.\n\
         pub const WORD0: u32 = {WORD0:#x};\n\
         /// Nodes, each a prefix of two codepoints or more.\n\
         pub const NODES: usize = {};\n\
         /// Bucket and entry bits of the perfect hash.\n\
         pub const BUCKET_BITS: u32 = {BUCKET_BITS};\n\
         pub const EDGE_BITS: u32 = {EDGE_BITS};\n\
         pub const M1: u64 = {M1:#x};\n\
         pub const M2: u64 = {M2:#x};",
        seqs.len(),
        nodes.len(),
        flat.len(),
        nodes.len(),
    ));
    out.array(
        "NODE_SPAN",
        "u32",
        &[
            "A node's span of `SPANS`: the offset in the low 16 bits, the length in",
            "codepoints above them.",
        ],
        &packed,
        PerLine(8),
        |v| format!("{v:#08x}"),
    );
    out.array(
        "SPANS",
        "u32",
        &[
            "The nodes' codepoints: each longest sequence once, its prefixes sharing",
            "its span.",
        ],
        &flat,
        PerLine(8),
        |v| format!("{v:#07x}"),
    );
    out.array(
        "SEEDS",
        "u16",
        &["Each bucket's seed."],
        &hash.seeds,
        PerLine(16),
        u16::to_string,
    );
    out.array(
        "EDGES",
        "u64",
        &[
            "Each entry: the key -- the parent's word above the codepoint, 21 bits",
            "each -- above the child node plus one; zero where no edge lands.",
        ],
        &edges,
        PerLine(4),
        |v| format!("{v:#x}"),
    );
    out.array(
        "CONTINUES",
        "u8",
        &[
            "One bit a block of 256 codepoints: set where some sequence continues",
            "with a codepoint of that block.",
        ],
        &continues,
        PerLine(16),
        |v| format!("{v:#04x}"),
    );
    errs.or(out)
}
