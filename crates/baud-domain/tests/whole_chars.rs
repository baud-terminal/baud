//! **A run cut at its end keeps its whole UTF-8 characters**
//! (`whole_chars`): every cut inside a two-, three- or four-byte final
//! character drops what of it arrived, and a run ending on a whole one is
//! left as it is.

use baud_domain::whole_chars;

#[test]
fn a_cut_inside_a_final_character_drops_it() {
    for ch in ["é", "€", "😀"] {
        let whole = [b"ab".as_slice(), ch.as_bytes()].concat();
        assert_eq!(whole_chars(&whole), whole.as_slice(), "{ch} whole");
        for cut in 3..whole.len() {
            assert_eq!(whole_chars(&whole[..cut]), b"ab", "{ch} cut to {cut}");
        }
    }
    assert_eq!(whole_chars(b""), b"", "empty");
    assert_eq!(whole_chars(b"abc"), b"abc", "ASCII");
}
