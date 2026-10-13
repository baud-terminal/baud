import sys
p = sys.argv[1]
s = open(p).read()
a1 = """            let class = combining_class(seq[i]).0;
            let mut j = i;
            while class != 0 && j > 0 {
                let before = combining_class(seq[j - 1]).0;"""
b1 = """            let class = sort_class(seq[i]);
            let mut j = i;
            while class != 0 && j > 0 {
                let before = sort_class(seq[j - 1]);"""
assert s.count(a1) == 1, "sort anchor"
s = s.replace(a1, b1)
a2 = """/// A canonical combining class (`[[source: unicode-ch3]]` D104): zero for"""
b2 = """/// HarfBuzz's modified combining classes, Hebrew only: the fixed-position
/// classes 10-26 permuted into SBL order (`[[source: harfbuzz-hb-unicode]]`,
/// hb-unicode.hh:320-349 at 64e2b53). Every other class is itself. Indexed
/// by the canonical class; built at compile time.
const SORT_CLASS: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        t[i] = i as u8;
        i += 1;
    }
    t[10] = 22; // sheva
    t[11] = 15; // hataf segol
    t[12] = 16; // hataf patah
    t[13] = 17; // hataf qamats
    t[14] = 23; // hiriq
    t[15] = 18; // tsere
    t[16] = 19; // segol
    t[17] = 20; // patah
    t[18] = 21; // qamats, qamats qatan
    t[19] = 14; // holam, holam haser for vav
    t[20] = 24; // qubuts
    t[21] = 12; // dagesh
    t[22] = 25; // meteg
    t[23] = 13; // rafe
    t[24] = 10; // shin dot
    t[25] = 11; // sin dot
    t[26] = 26; // point varika
    t
};

/// The key the mark sort orders by: HarfBuzz's modified class (arm C2).
fn sort_class(c: Codepoint) -> u8 {
    SORT_CLASS[usize::from(combining_class(c).0)]
}

/// A canonical combining class (`[[source: unicode-ch3]]` D104): zero for"""
assert s.count(a2) == 1, "doc anchor"
s = s.replace(a2, b2)
open(p, "w").write(s)
print("patched")
