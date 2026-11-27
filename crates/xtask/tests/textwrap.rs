//! `xtask::textwrap` against Python 3.13's `textwrap.fill`, called as the
//! NOTICE calls it (`width=76`, `break_on_hyphens=False`,
//! `break_long_words=False`). Each expected value is what Python printed
//! for the case: tabs at every column, `\r\n`, the vertical tab and form
//! feed, a no-break space inside a word, U+2003 alone as a word, words
//! longer than the line, indents as wide as it or wider, empty and
//! whitespace-only text, and sixty generated strings over those
//! characters.

use xtask::html::Count;
use xtask::textwrap::Wrap;

pub const CASES: [(&str, &str, &str, &str); 86] = [
    ("", "", "", ""),
    ("   ", "", "", ""),
    ("  lead and trail  ", "", "", "  lead and trail"),
    ("one", "  Files:   ", "           ", "  Files:   one"),
    ("a\u{9}b\u{9}c\u{9}d", "", "", "a       b       c       d"),
    (
        "\u{9}x",
        "  Holder:  ",
        "           ",
        "  Holder:          x",
    ),
    (
        "ab\u{9}c\u{a}d\u{9}e\u{d}f\u{9}g",
        "",
        "",
        "ab      c d       e f       g",
    ),
    (
        "line one\u{a}line two\u{d}\u{a}line three\u{b}four\u{c}five",
        "",
        "",
        "line one line two  line three four five",
    ),
    (
        "word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word ",
        "",
        "",
        "word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word\u{a}word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word word\u{a0}word",
    ),
    (
        "x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y ",
        "",
        "",
        "x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003}\u{a}y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y x \u{2003} y",
    ),
    ("\u{2003} a", "", "", "\u{2003} a"),
    (
        "a \u{2003}\u{2003}\u{2003} b",
        "",
        "",
        "a \u{2003}\u{2003}\u{2003} b",
    ),
    (
        "short LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL tail",
        "",
        "",
        "short\u{a}LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL\u{a}tail",
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
        "  From:    ",
        "           ",
        "  From:    LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL",
    ),
    (
        "a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a ",
        "",
        "",
        "a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a\u{a}a a",
    ),
    (
        "a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  ",
        "",
        "",
        "a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a  a\u{a}a  a  a  a  a  a  a  a  a  a  a  a  a  a",
    ),
    (
        "hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long ",
        "",
        "",
        "hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long\u{a}hyphen-ated-words-that-are-long hyphen-ated-words-that-are-long\u{a}hyphen-ated-words-that-are-long",
    ),
    (
        "x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y ",
        "",
        "",
        "x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y\u{a}x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y x\u{1c}y",
    ),
    (
        "caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} ",
        "",
        "",
        "caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9}\u{a}\u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e} caf\u{e9} \u{65e5}\u{672c}\u{8a9e}",
    ),
    (
        "\u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} ",
        "",
        "",
        "\u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600}\u{a}\u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600} \u{1f600}",
    ),
    ("exactly", "", "", "exactly"),
    (
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww",
        "",
        "",
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww",
    ),
    (
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww",
        "",
        "",
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww",
    ),
    (
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww z",
        "",
        "",
        "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww\u{a}z",
    ),
    (
        "a b",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxa\u{a}yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyb",
    ),
    (
        "a b c",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxa\u{a}b c",
    ),
    (
        "ggggggggggggggggggggg\u{e9}\u{65e5}\u{e9}bb\u{e9}\u{65e5}\u{9}aaaaaaaaaaaaaaaaaaaaaaaaaaa  eeeeeeeeeeeeeeeeeeccccddddddddddddbddddddddddddddddggggggggggggggggggggggggg\u{65e5}",
        "  Files:   ",
        "           ",
        "  Files:   ggggggggggggggggggggg\u{e9}\u{65e5}\u{e9}bb\u{e9}\u{65e5}    aaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a}           eeeeeeeeeeeeeeeeeeccccddddddddddddbddddddddddddddddggggggggggggggggggggggggg\u{65e5}",
    ),
    (
        " dddhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\u{2003}b\u{a0}-\u{2003}\u{1c}\u{e9}fffffffffffhhhhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{a}\u{1c}\u{a}hhhhhhhhhhfffffff",
        "  Licence: ",
        "",
        "  Licence: dddhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\u{2003}b\u{a0}-\u{2003}\u{1c}\u{e9}fffffffffffhhhhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{a}\u{1c} hhhhhhhhhhfffffff",
    ),
    (
        "\u{a}\u{9}\u{2003}\u{2003}\u{a}gggggggggggggggggggggggggggggggggggggggggggggggg \u{1c}\u{2003}ccccccccc -fffffaaaaaaaaaaaaaaagggggggggggggb\u{a0}bhhhhhh\u{65e5}abbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{a0}\u{a}ffffffffffffffff\u{2003}hhhhhhhhhhhhhhhhb",
        "",
        "",
        "         \u{2003}\u{2003} gggggggggggggggggggggggggggggggggggggggggggggggg \u{1c}\u{2003}ccccccccc\u{a}-fffffaaaaaaaaaaaaaaagggggggggggggb\u{a0}bhhhhhh\u{65e5}abbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{a0}\u{a}ffffffffffffffff\u{2003}hhhhhhhhhhhhhhhhb",
    ),
    (
        "hhhhhhhhhhhhhhhhhhhhhhhhhhhaaaaaaafffffaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbffffffffffffffffffffffffffffff\u{9}fffffffffffffffffffff\u{9}ggggggggggggggggggggggggdddddddddddddddddaeeeeeeeeeeeeeeee\u{65e5}hhhhhhhhhhhhhhhhhhhhhhhhhhffffffffffffb\u{9}\u{2003}aaaaaaaaaaaaaaaaffffffffffffffffffffffffff",
        "  Licence: ",
        "",
        "  Licence: hhhhhhhhhhhhhhhhhhhhhhhhhhhaaaaaaafffffaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbffffffffffffffffffffffffffffff\u{a}fffffffffffffffffffff\u{a}ggggggggggggggggggggggggdddddddddddddddddaeeeeeeeeeeeeeeee\u{65e5}hhhhhhhhhhhhhhhhhhhhhhhhhhffffffffffffb\u{a}\u{2003}aaaaaaaaaaaaaaaaffffffffffffffffffffffffff",
    ),
    (
        "ddddddddddddddddggggggggggggggggggggggggggbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggbccccc\u{65e5}",
        "  Files:   ",
        "",
        "  Files:   ddddddddddddddddggggggggggggggggggggggggggbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggbccccc\u{65e5}",
    ),
    (
        "hhhhhhhhhhhhhhhhhhhhhhccccccccccccccccccabbbbbbbbbbbbbbbbbccccccccccccccdddddddddddddddddddddddddddaaaaaaaaa\u{e9}\u{65e5}\u{e9} -gggggggggggggggggggggggggggcccccccccccccccccc\u{e9}\u{2003}aaaaaaaaaaaaaaaaaaaaaaaaacccccbffffffffffffffffffffffhhhhhhhhhhhhhhhhhhhhhhhhhhaaaaaaaaahhhhhhhhhhhhhhhhhhb\u{65e5}ddddddddddddddddddddddd\u{e9}hhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeeeeeeddddddddddddddddddddddddddd\u{a0}\u{2003}\u{1c}b\u{a}cccccccccccccccccccccccfffff ",
        "  Files:   ",
        "",
        "  Files:   hhhhhhhhhhhhhhhhhhhhhhccccccccccccccccccabbbbbbbbbbbbbbbbbccccccccccccccdddddddddddddddddddddddddddaaaaaaaaa\u{e9}\u{65e5}\u{e9}\u{a}-gggggggggggggggggggggggggggcccccccccccccccccc\u{e9}\u{2003}aaaaaaaaaaaaaaaaaaaaaaaaacccccbffffffffffffffffffffffhhhhhhhhhhhhhhhhhhhhhhhhhhaaaaaaaaahhhhhhhhhhhhhhhhhhb\u{65e5}ddddddddddddddddddddddd\u{e9}hhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeeeeeeddddddddddddddddddddddddddd\u{a0}\u{2003}\u{1c}b\u{a}cccccccccccccccccccccccfffff",
    ),
    (
        "\u{2003}\u{1c}ccccccccccccccccccccccc\u{e9}\u{a0}-",
        "",
        "           ",
        "\u{2003}\u{1c}ccccccccccccccccccccccc\u{e9}\u{a0}-",
    ),
    ("\u{2003}", "  Files:   ", "", ""),
    (
        "\u{65e5}b\u{9}bbbacccccccccggggggggggggggggggggggggggggeeeeeeeeeeeee\u{e9}fff baeeedddb-ggggggggggggggggggggggggggggggccddddee\u{a}dddddddddd\u{1c}",
        "",
        "           ",
        "\u{65e5}b      bbbacccccccccggggggggggggggggggggggggggggeeeeeeeeeeeee\u{e9}fff\u{a}           baeeedddb-ggggggggggggggggggggggggggggggccddddee dddddddddd\u{1c}",
    ),
    (
        "ee\u{e9}ddddddddddddddddd\u{2003}\u{1c}\u{2003}ggggggggggggggggg\u{9}fffffffcccccccccccccaaaaaaaaaaaaaaaaaaaaaaaaaaabeeeeeeeeeeeeeebggggggggggggggggggggggggggggeeeeeeeeeeeeeeeeeeee\u{a} \u{2003}-ffffffff\u{a}",
        "",
        "           ",
        "ee\u{e9}ddddddddddddddddd\u{2003}\u{1c}\u{2003}ggggggggggggggggg\u{a}           fffffffcccccccccccccaaaaaaaaaaaaaaaaaaaaaaaaaaabeeeeeeeeeeeeeebggggggggggggggggggggggggggggeeeeeeeeeeeeeeeeeeee\u{a}           \u{2003}-ffffffff",
    ),
    (
        "\u{a0}\u{a}ddddddddaaab\u{65e5}a\u{1c}\u{65e5}ccccccccccccccccccccccggggggggggggggggggggggggg",
        "  Files:   ",
        "           ",
        "  Files:   \u{a0} ddddddddaaab\u{65e5}a\u{1c}\u{65e5}ccccccccccccccccccccccggggggggggggggggggggggggg",
    ),
    (
        "\u{65e5}aaaaaaaaaaaaaaaaaaaaaaaaaaaggggggggggggggggggggggggccccccccccccccccccccccccccccccaaaaaaaaaaaaaaaaaaaaaaaaaaaddd bbbbbbbbbbbbbaaaaaaaaaaaaaaaaaaaaa",
        "",
        "",
        "\u{65e5}aaaaaaaaaaaaaaaaaaaaaaaaaaaggggggggggggggggggggggggccccccccccccccccccccccccccccccaaaaaaaaaaaaaaaaaaaaaaaaaaaddd\u{a}bbbbbbbbbbbbbaaaaaaaaaaaaaaaaaaaaa",
    ),
    (
        "\u{2003}bbbbbbbbbbbbbbbbbbbbbbhhhhhhhhheeeeeeeeddddddddhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbeeeeeeeeeeeeeeeeeeeeeeeee\u{1c}bbbbbbbbbbbbbbbbbbbb\u{a}eeeeeeeeeeeeeeeeeeeeaaaaaaaaaaaaaaaa\u{a}bbbbbbbbbbbbbbbbbbbbbbb\u{2003}\u{e9}\u{2003}bddddddddddhbhhhhhhhhh\u{9}b\u{e9}-\u{1c}bbbbbbbbbbbbbbbbbbbbbbb\u{2003}hhhhhhhhhhhhh",
        "",
        "",
        "\u{2003}bbbbbbbbbbbbbbbbbbbbbbhhhhhhhhheeeeeeeeddddddddhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbeeeeeeeeeeeeeeeeeeeeeeeee\u{1c}bbbbbbbbbbbbbbbbbbbb\u{a}eeeeeeeeeeeeeeeeeeeeaaaaaaaaaaaaaaaa\u{a}bbbbbbbbbbbbbbbbbbbbbbb\u{2003}\u{e9}\u{2003}bddddddddddhbhhhhhhhhh\u{a}b\u{e9}-\u{1c}bbbbbbbbbbbbbbbbbbbbbbb\u{2003}hhhhhhhhhhhhh",
    ),
    ("", "  Files:   ", "           ", ""),
    (
        " \u{a0}--bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbaeeeeeeeee\u{a0}\u{65e5}\u{a0}aaaaaaaaa\u{1c} \u{a}--ggggggggggggggggggggggggggggg\u{1c}\u{e9}bbgggggggggggggggccccccccccccccccccccchhcccccc-",
        "  Files:   ",
        "           ",
        "  Files:    \u{a0}--bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbaeeeeeeeee\u{a0}\u{65e5}\u{a0}aaaaaaaaa\u{1c}\u{a}           --ggggggggggggggggggggggggggggg\u{1c}\u{e9}bbgggggggggggggggccccccccccccccccccccchhcccccc-",
    ),
    (
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeegggg \u{e9}hhhhhhhhhhhhhhhhhh-hhhhhhhhhhhhhh\u{9} b-\u{65e5}aggggggggggggg\u{e9}",
        "",
        "           ",
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeegggg \u{e9}hhhhhhhhhhhhhhhhhh-hhhhhhhhhhhhhh\u{a}           b-\u{65e5}aggggggggggggg\u{e9}",
    ),
    (
        "a\u{65e5}ccccccccccccccccccccccddd\u{9}\u{1c}\u{a}aaaaa\u{2003}h\u{e9}hhhhhhhhddddd\u{1c}\u{1c}hhha",
        "",
        "",
        "a\u{65e5}ccccccccccccccccccccccddd     \u{1c} aaaaa\u{2003}h\u{e9}hhhhhhhhddddd\u{1c}\u{1c}hhha",
    ),
    (
        "eeeeegggggggggggggggggggggggbbb\u{65e5}\u{a}\u{65e5}\u{e9}\u{2003}-dddddddddddddddddee\u{2003}ggg\u{1c}-aggggggggggggdbbbbbbb\u{9}\u{9}\u{9}\u{a}\u{65e5} hhhhhhhhhhhhhhaaaaaaaaaaaaaaaaaaaa\u{a0}acccccccccccccca\u{2003}ffffffffffffffffffffffffbfffffff",
        "",
        "           ",
        "eeeeegggggggggggggggggggggggbbb\u{65e5}\u{a}           \u{65e5}\u{e9}\u{2003}-dddddddddddddddddee\u{2003}ggg\u{1c}-aggggggggggggdbbbbbbb\u{a}           \u{65e5}\u{a}           hhhhhhhhhhhhhhaaaaaaaaaaaaaaaaaaaa\u{a0}acccccccccccccca\u{2003}ffffffffffffffffffffffffbfffffff",
    ),
    ("\u{a0}fffffffffffffff", "", "", "\u{a0}fffffffffffffff"),
    ("", "", "           ", ""),
    (
        "bddddddddddddd\u{a}ggg\u{2003}",
        "",
        "           ",
        "bddddddddddddd ggg\u{2003}",
    ),
    (
        "ddddddddddd\u{2003}\u{a0}\u{1c}aaaaaaaaaaaaabaaaaaaaaabffffffffffff\u{65e5}-ebbbbbbbbbbbbbbbbbhhhhhhhhhhhhhhhhhhhhhhhhh\u{a}hhhhhceeeeeeeeeeeeeeeeeeeeeeeeeeecccccccccccccccccccc-\u{65e5}\u{9} bhhhhhhhhhhhhhhhhhhccccccccccccccbbbbbbbbbdddd\u{2003} \u{65e5}ddddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbbbbbbb",
        "  Files:   ",
        "           ",
        "  Files:   ddddddddddd\u{2003}\u{a0}\u{1c}aaaaaaaaaaaaabaaaaaaaaabffffffffffff\u{65e5}-ebbbbbbbbbbbbbbbbbhhhhhhhhhhhhhhhhhhhhhhhhh\u{a}           hhhhhceeeeeeeeeeeeeeeeeeeeeeeeeeecccccccccccccccccccc-\u{65e5}\u{a}           bhhhhhhhhhhhhhhhhhhccccccccccccccbbbbbbbbbdddd\u{2003}\u{a}           \u{65e5}ddddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbbbbbbb",
    ),
    (
        "fffffffffddddddddddddddd\u{9}\u{a}ddddddddddd\u{a}dddddddddddddddddddddhh\u{2003}ddddddddddddddddddddddddddd-\u{a}a\u{65e5}b \u{a}",
        "  Licence: ",
        "",
        "  Licence: fffffffffddddddddddddddd         ddddddddddd\u{a}dddddddddddddddddddddhh\u{2003}ddddddddddddddddddddddddddd- a\u{65e5}b",
    ),
    (
        "fffffff-\u{9}aaaaaaaaaaaaaaaaaaaaddddddddddddddddddddddddddd-",
        "  Files:   ",
        "           ",
        "  Files:   fffffff-        aaaaaaaaaaaaaaaaaaaaddddddddddddddddddddddddddd-",
    ),
    (
        "bbbbbbb\u{2003}bbbbbbbbbbbbbb\u{a0}ccccccccccccccccccccccccccccccccccgggggggggggg\u{65e5}gggggggggggggg-",
        "  Licence: ",
        "",
        "  Licence: bbbbbbb\u{2003}bbbbbbbbbbbbbb\u{a0}ccccccccccccccccccccccccccccccccccgggggggggggg\u{65e5}gggggggggggggg-",
    ),
    (
        "d b- agggffffffffffffffffffffffffcccccccccccc\u{e9}b\u{2003}ddddddddddahhhhhhhhhhh\u{1c}\u{65e5}cccccccccccccccccccccdddddddddddddddddddd\u{9}ccccccccccccccccccc\u{a0}ccccccccccccc \u{9}",
        "",
        "",
        "d b-\u{a}agggffffffffffffffffffffffffcccccccccccc\u{e9}b\u{2003}ddddddddddahhhhhhhhhhh\u{1c}\u{65e5}cccccccccccccccccccccdddddddddddddddddddd\u{a}ccccccccccccccccccc\u{a0}ccccccccccccc",
    ),
    (
        "\u{65e5}\u{1c}ggggggggggggggggggggggghhhhhhhhhhhhhhhhha\u{2003}\u{2003}hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{2003}b\u{a0}\u{2003}aabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggggggggcbbbbbbb\u{2003}",
        "  Files:   ",
        "",
        "  Files:   \u{65e5}\u{1c}ggggggggggggggggggggggghhhhhhhhhhhhhhhhha\u{2003}\u{2003}hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{2003}b\u{a0}\u{2003}aabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggggggggcbbbbbbb\u{2003}",
    ),
    (
        "-eeeeee\u{65e5}\u{2003}\u{e9}hhhhhhhddddddddddd\u{9} eeeeeeeeeeeeeeeeeeeeee\u{a0}\u{a}\u{e9}-",
        "  Files:   ",
        "",
        "  Files:   -eeeeee\u{65e5}\u{2003}\u{e9}hhhhhhhddddddddddd     eeeeeeeeeeeeeeeeeeeeee\u{a0} \u{e9}-",
    ),
    (
        "ggggggggggggggggggggggggeeeeeeeeeeeeeccccccccccccb aaaaaaaaaaeeeeeeeeeeffffffffffffffffffffffffa\u{a}ggggggggggggggaaaaa\u{65e5}aa-\u{e9}",
        "  Files:   ",
        "",
        "  Files:   ggggggggggggggggggggggggeeeeeeeeeeeeeccccccccccccb\u{a}aaaaaaaaaaeeeeeeeeeeffffffffffffffffffffffffa ggggggggggggggaaaaa\u{65e5}aa-\u{e9}",
    ),
    (
        "ccccccc\u{2003}adddddddddddddddddddddddb eeeeeeeeeeeeeaaffffffffffffffffffffhhhhhhhhhhhhhhhhhhhhhhhhhhhha\u{e9} abddddd\u{e9}gggggggggggggggggggggggggggeeeahhhhhhhhhhhhhhhhhhhhhhhgggggggggggggggggggggggggggg\u{2003}\u{1c}\u{9}",
        "",
        "           ",
        "ccccccc\u{2003}adddddddddddddddddddddddb\u{a}           eeeeeeeeeeeeeaaffffffffffffffffffffhhhhhhhhhhhhhhhhhhhhhhhhhhhha\u{e9}\u{a}           abddddd\u{e9}gggggggggggggggggggggggggggeeeahhhhhhhhhhhhhhhhhhhhhhhgggggggggggggggggggggggggggg\u{2003}\u{1c}",
    ),
    (
        "bbbbbbbbbbbeeeeeeeeeeeeeeeeeeeeeee\u{1c}ggggggggggggggggggggggeeeeeeeeeedddaaaaaa\u{9}ddddddfffffffffffffffffffffffffff\u{1c}hhhhhhhhhhhhhhhhaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "",
        "           ",
        "bbbbbbbbbbbeeeeeeeeeeeeeeeeeeeeeee\u{1c}ggggggggggggggggggggggeeeeeeeeeedddaaaaaa\u{a}           ddddddfffffffffffffffffffffffffff\u{1c}hhhhhhhhhhhhhhhhaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ),
    (
        "eeeeeeeeeeeeeeeeeeeeeeeeee\u{65e5}cccccb  aa\u{1c}bbbbbbbbbbbbbbbbbbbbbbbb\u{65e5}dddddddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbbbbbbbbbbbgggg\u{9}",
        "",
        "",
        "eeeeeeeeeeeeeeeeeeeeeeeeee\u{65e5}cccccb\u{a}aa\u{1c}bbbbbbbbbbbbbbbbbbbbbbbb\u{65e5}dddddddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbbbbbbbbbbbgggg",
    ),
    (
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbeeeeeeeeeeeeeeee",
        "",
        "",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbeeeeeeeeeeeeeeee",
    ),
    (
        "dddddddddd\u{a0}-\u{a}-hhhhhhhhhhhhhhhhhhhhhhhhhhhh",
        "  Files:   ",
        "",
        "  Files:   dddddddddd\u{a0}- -hhhhhhhhhhhhhhhhhhhhhhhhhhhh",
    ),
    (
        "\u{e9}ffffffffffffffffdddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbcccccccccccccc\u{9}a\u{2003} fffffffffffffffffffffffffffccccccccccdddddddddddddddd\u{1c}hhhhhhhhhhhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbb\u{a0}bbbbbbbbbbbbbbaaaaaaaaaaaa\u{a}\u{e9}ggggggggggggggggggggggggggggghhhhhaaaaaaaaaaaaccccccccccccccccccccccccccccffffff",
        "  Files:   ",
        "           ",
        "  Files:   \u{e9}ffffffffffffffffdddddddddddddddddddddddbbbbbbbbbbbbbbbbbbbcccccccccccccc\u{a}           a\u{2003}\u{a}           fffffffffffffffffffffffffffccccccccccdddddddddddddddd\u{1c}hhhhhhhhhhhhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbb\u{a0}bbbbbbbbbbbbbbaaaaaaaaaaaa\u{a}           \u{e9}ggggggggggggggggggggggggggggghhhhhaaaaaaaaaaaaccccccccccccccccccccccccccccffffff",
    ),
    (
        "ccccccccccc\u{9}eeeeeeeeeecccccccccccccccccccccccc\u{9}ffffff\u{9}b\u{1c}\u{a0} eeeeeeeeeeeeeebbbbbbbbbb\u{a0}a",
        "  Files:   ",
        "           ",
        "  Files:   ccccccccccc     eeeeeeeeeecccccccccccccccccccccccc      ffffff\u{a}           b\u{1c}\u{a0} eeeeeeeeeeeeeebbbbbbbbbb\u{a0}a",
    ),
    (
        "eeeeeeeeeeeeeee\u{a}gggggggggggggggggggggggggggggggggggggggggggggggggggg\u{1c}ddddddddddddddddddddddb-bdddddddddddddddddddddddddd\u{1c}\u{a0}a",
        "  Licence: ",
        "           ",
        "  Licence: eeeeeeeeeeeeeee\u{a}           gggggggggggggggggggggggggggggggggggggggggggggggggggg\u{1c}ddddddddddddddddddddddb-bdddddddddddddddddddddddddd\u{1c}\u{a0}a",
    ),
    (
        "cccccccccccccccccccccccccccccaaaaaaaaaaaaabb\u{9}\u{9}bbbbbbbbbbbbbbbbbbbbbbbbbbbbdddddddddddddddddddddddafffffffffffffffff\u{2003}\u{1c}\u{e9}bbbbbbbbbbbbbbbbbbbbbbbbfffffffffffffffffffff\u{a}a\u{a0}fffffffffffffffffff\u{9}\u{a0}ddddddddddddddddddddddddddhhhhhhhbdddddddddddddddddddddddddddddddddddddddddddffffffffffffffffffffffgggggggggggggggcccccccccccccccccccccccccffffffffffffffffffffffffffeeeeeeeeeeeeeeeeeeeeeee\u{a}cccccccccccccccc\u{a}",
        "  Files:   ",
        "",
        "  Files:   cccccccccccccccccccccccccccccaaaaaaaaaaaaabb\u{a}bbbbbbbbbbbbbbbbbbbbbbbbbbbbdddddddddddddddddddddddafffffffffffffffff\u{2003}\u{1c}\u{e9}bbbbbbbbbbbbbbbbbbbbbbbbfffffffffffffffffffff\u{a}a\u{a0}fffffffffffffffffff\u{a}\u{a0}ddddddddddddddddddddddddddhhhhhhhbdddddddddddddddddddddddddddddddddddddddddddffffffffffffffffffffffgggggggggggggggcccccccccccccccccccccccccffffffffffffffffffffffffffeeeeeeeeeeeeeeeeeeeeeee\u{a}cccccccccccccccc",
    ),
    (
        "\u{2003}\u{1c}-\u{a}aaafffffffffffffffffffffffffffffffffffffffffffffffaaaaaaaeeeeeeeeecccccccccccccccccccccccccccc\u{2003} \u{a0}ccccccccccccccccccccbbbbbbbbbbbbbbbbbbbbbbeeeeeee\u{9}hhhhhhhhhhhhhhhhhhhhhhbbbbbbbbb",
        "  Files:   ",
        "",
        "  Files:   \u{2003}\u{1c}-\u{a}aaafffffffffffffffffffffffffffffffffffffffffffffffaaaaaaaeeeeeeeeecccccccccccccccccccccccccccc\u{2003}\u{a}\u{a0}ccccccccccccccccccccbbbbbbbbbbbbbbbbbbbbbbeeeeeee\u{a}hhhhhhhhhhhhhhhhhhhhhhbbbbbbbbb",
    ),
    (
        "\u{e9}\u{2003}hhhhhhhh\u{e9}aaaaaahhhhhhhhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeee\u{a0}",
        "  Files:   ",
        "",
        "  Files:   \u{e9}\u{2003}hhhhhhhh\u{e9}aaaaaahhhhhhhhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeee\u{a0}",
    ),
    (
        "affffffffffffffffffffffffffhhhhhhhhhhhhhhhhcc\u{a0}fffffffffffffff\u{e9}dddddddddd\u{a0}a",
        "  Files:   ",
        "           ",
        "  Files:   affffffffffffffffffffffffffhhhhhhhhhhhhhhhhcc\u{a0}fffffffffffffff\u{e9}dddddddddd\u{a0}a",
    ),
    (
        "gggggggggggeeeeeeeeeeeeeeeeeeeeeeeeeeeedddddddddddddddddddddb-cccccccccccccccccccbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggggggggggggggggggggggggggggggggggggggga\u{2003}aaaaaaaaaaaaaaaaaaaaaaaaaagggggggggggggggggggg\u{1c}bbbbbbb\u{1c}  ggggggggggggggggggggggggg\u{1c} ",
        "  Files:   ",
        "           ",
        "  Files:   gggggggggggeeeeeeeeeeeeeeeeeeeeeeeeeeeedddddddddddddddddddddb-cccccccccccccccccccbbbbbbbbbbbbbbbbbbbbbbbbbbgggggggggggggggggggggggggggggggggggggggggggggggggggga\u{2003}aaaaaaaaaaaaaaaaaaaaaaaaaagggggggggggggggggggg\u{1c}bbbbbbb\u{1c}\u{a}           ggggggggggggggggggggggggg\u{1c}",
    ),
    (
        "a\u{a0}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a0}ggggggggggggggg\u{1c}\u{65e5}ccccccccccccccccbbbdddddddddddddddddddddddddddddaabbbbbbbbbbbbbbbbbbbbbbbbbbbbba\u{65e5} fffffffffffffffffffffffff",
        "  Licence: ",
        "",
        "  Licence: a\u{a0}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a0}ggggggggggggggg\u{1c}\u{65e5}ccccccccccccccccbbbdddddddddddddddddddddddddddddaabbbbbbbbbbbbbbbbbbbbbbbbbbbbba\u{65e5}\u{a}fffffffffffffffffffffffff",
    ),
    (
        "\u{e9}hhhhhhhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeeeeeea\u{1c}",
        "  Licence: ",
        "",
        "  Licence: \u{e9}hhhhhhhhhhhhhhhhhhhhhheeeeeeeeeeeeeeeeeeeeeeeeeeeeeea\u{1c}",
    ),
    (
        "\u{65e5}\u{2003}ffffffffffffhhhhhhhhhhhhhhhhcccccccccccccccccccccccccc\u{1c}\u{a0}\u{2003}ffffffffff\u{65e5}ffffffffffffffffffffffffffffaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a}ddddddddddddd\u{a0}dddddddddddddddddddddddddda\u{a}\u{65e5}aaaaaaaaaaccccccccchhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\u{9}",
        "  Files:   ",
        "",
        "  Files:   \u{65e5}\u{2003}ffffffffffffhhhhhhhhhhhhhhhhcccccccccccccccccccccccccc\u{1c}\u{a0}\u{2003}ffffffffff\u{65e5}ffffffffffffffffffffffffffffaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a}ddddddddddddd\u{a0}dddddddddddddddddddddddddda\u{a}\u{65e5}aaaaaaaaaaccccccccchhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh",
    ),
    (
        "\u{9}aaaaaaaaaaaaaaaaaaaaaaaaaa\u{e9}-dddddddddddddeeeeeeeeeeeeeeeeeeeeeeeeeeeeeffffffffffffffffdddddddb\u{a}\u{65e5}\u{e9}ddhhhhhhhhhhhhfffffffffffffffffffffb\u{65e5}\u{a}aaaa\u{65e5}\u{65e5}\u{a}\u{2003}cccccccccfffffff",
        "",
        "           ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaa\u{e9}-dddddddddddddeeeeeeeeeeeeeeeeeeeeeeeeeeeeeffffffffffffffffdddddddb\u{a}           \u{65e5}\u{e9}ddhhhhhhhhhhhhfffffffffffffffffffffb\u{65e5} aaaa\u{65e5}\u{65e5} \u{2003}cccccccccfffffff",
    ),
    (
        "ahhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbbbbbbbbggggggggggggggggggggggggggggggb",
        "  Files:   ",
        "           ",
        "  Files:   ahhhhhhhhhhhhhhhhbbbbbbbbbbbbbbbbbbbbbbbbbbbbggggggggggggggggggggggggggggggb",
    ),
    (
        "bgggggg \u{9}dddddd\u{a}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaeeeeeeeeeeeeeeeeeeeeeeeeeehh-ddddddddddddddddddddddhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a0}\u{2003}\u{2003} aaaaaaaaaaaaaaadddddddddddddddddddddddddd\u{9}\u{65e5}cccccccccccccccccccccccccbgggggggggggggggggggggggggggb-\u{9}\u{1c}-a\u{2003}ccccccccccccccceeeeeeeeeeeeee \u{65e5}",
        "  Files:   ",
        "           ",
        "  Files:   bgggggg         dddddd\u{a}           aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaeeeeeeeeeeeeeeeeeeeeeeeeeehh-ddddddddddddddddddddddhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a0}\u{2003}\u{2003}\u{a}           aaaaaaaaaaaaaaadddddddddddddddddddddddddd\u{a}           \u{65e5}cccccccccccccccccccccccccbgggggggggggggggggggggggggggb-\u{a}           \u{1c}-a\u{2003}ccccccccccccccceeeeeeeeeeeeee \u{65e5}",
    ),
    (
        "b\u{2003}\u{e9}\u{1c}hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a}ffffffffffffffdddddddddddddddddddddddddddddd\u{a0} ",
        "",
        "           ",
        "b\u{2003}\u{e9}\u{1c}hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a}           ffffffffffffffdddddddddddddddddddddddddddddd\u{a0}",
    ),
    (
        "aaaaaaaaaaaaaaafffffffffffffffffaeeeeeeadddddddddccccccccccccccccccccccccccc\u{9}dddddddddddddddddddd",
        "",
        "",
        "aaaaaaaaaaaaaaafffffffffffffffffaeeeeeeadddddddddccccccccccccccccccccccccccc\u{a}dddddddddddddddddddd",
    ),
    (
        "eeeeee\u{65e5}ddddddddddddddddddda\u{e9}afffffffffff\u{1c}hhh\u{2003}\u{1c} ff-aaaaaaaaaaaahhhhhhhhhhhhhhhhh-fffffffffffffffffffffffffgggggggggggggggggggaaaaaaaaaahhhhhhhhhhhhhhhcb \u{a}a\u{9}\u{65e5}hhhhhhhhhhhhhhhhh\u{2003}baaaaaaaaa\u{2003}eeee\u{a0}dddddddddddddddddddddddddddd\u{1c}gggggg",
        "",
        "           ",
        "eeeeee\u{65e5}ddddddddddddddddddda\u{e9}afffffffffff\u{1c}hhh\u{2003}\u{1c}\u{a}           ff-aaaaaaaaaaaahhhhhhhhhhhhhhhhh-fffffffffffffffffffffffffgggggggggggggggggggaaaaaaaaaahhhhhhhhhhhhhhhcb\u{a}           a\u{a}           \u{65e5}hhhhhhhhhhhhhhhhh\u{2003}baaaaaaaaa\u{2003}eeee\u{a0}dddddddddddddddddddddddddddd\u{1c}gggggg",
    ),
    (
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a0}-fffffffffffffffffffffffffff\u{e9}\u{e9}\u{1c}dddddddddddddddddddddddddddd\u{1c}bbbbbbbbbbbb\u{e9}ddddd\u{a0}hhhhhhhhhhhhhhhhhhhhha\u{1c}eeeeeeeeeeeeeeeeeeeeeaaaaaaaaaaaaaaaaaaaabggggggggeeee\u{1c}aeee",
        "  Files:   ",
        "",
        "  Files:   aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{a0}-fffffffffffffffffffffffffff\u{e9}\u{e9}\u{1c}dddddddddddddddddddddddddddd\u{1c}bbbbbbbbbbbb\u{e9}ddddd\u{a0}hhhhhhhhhhhhhhhhhhhhha\u{1c}eeeeeeeeeeeeeeeeeeeeeaaaaaaaaaaaaaaaaaaaabggggggggeeee\u{1c}aeee",
    ),
    (
        " ggggggggggggggggggg\u{9}eeeeeeeeeeeeeeeeeeeeeeeeeee\u{65e5}\u{a0}-\u{e9}\u{2003}\u{a}-\u{e9}gccccccccccccccccccccccccccccffffffffffffffffff\u{a}\u{9}abfffffffffffffffgggggggggggggggggggggggggggbcccccccccccccc-\u{9}eeeeeeeeeeeeeeeeeeeeeeeeeeebbbbbbbbbbbbbbbbbbbbbbbbhhhhhhhhh",
        "  Licence: ",
        "",
        "  Licence:  ggggggggggggggggggg    eeeeeeeeeeeeeeeeeeeeeeeeeee\u{65e5}\u{a0}-\u{e9}\u{2003}\u{a}-\u{e9}gccccccccccccccccccccccccccccffffffffffffffffff\u{a}abfffffffffffffffgggggggggggggggggggggggggggbcccccccccccccc-\u{a}eeeeeeeeeeeeeeeeeeeeeeeeeeebbbbbbbbbbbbbbbbbbbbbbbbhhhhhhhhh",
    ),
    (
        "aaaaaaaaaaaaaabbbbbbbbbbbbbbbb\u{65e5}\u{a}bbbbbbbbbbbbbhhhhhhhhhheeeeeeeeeeee\u{e9}fhhhhhhhhhhhhh ccccccccccccccdddfffffffffffddddddddddd\u{a0}a\u{65e5}eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeggggggggggggggggggggggggggggggafffffffffffffffbbbbbbbbbbbbbbbbb\u{a0}",
        "  Files:   ",
        "           ",
        "  Files:   aaaaaaaaaaaaaabbbbbbbbbbbbbbbb\u{65e5}\u{a}           bbbbbbbbbbbbbhhhhhhhhhheeeeeeeeeeee\u{e9}fhhhhhhhhhhhhh\u{a}           ccccccccccccccdddfffffffffffddddddddddd\u{a0}a\u{65e5}eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeggggggggggggggggggggggggggggggafffffffffffffffbbbbbbbbbbbbbbbbb\u{a0}",
    ),
    (
        "ccccccccccccccccccccccccccccc\u{a0}\u{2003}fffffffffffffffffffffffbbbbbb-eeeeeeeeeeeeeeeee\u{1c}fffffffffffffffffffffffffffggggggggggggggggggggg\u{a}dddddddddddddddddgggggg\u{65e5}fffffffffffffffffffaaaaaaaaaaaaaaaaaaaaaaaa\u{e9}\u{a}baaaaaaa\u{e9}ccccccccccccccccccc\u{65e5} b hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a0}aaaaaaaaaaaaaaaaaaaaa\u{65e5}\u{9} \u{1c}\u{65e5}",
        "",
        "           ",
        "ccccccccccccccccccccccccccccc\u{a0}\u{2003}fffffffffffffffffffffffbbbbbb-eeeeeeeeeeeeeeeee\u{1c}fffffffffffffffffffffffffffggggggggggggggggggggg\u{a}           dddddddddddddddddgggggg\u{65e5}fffffffffffffffffffaaaaaaaaaaaaaaaaaaaaaaaa\u{e9}\u{a}           baaaaaaa\u{e9}ccccccccccccccccccc\u{65e5} b\u{a}           hhhhhhhhhhhhhhhhhhhhhhhhhhh\u{a0}aaaaaaaaaaaaaaaaaaaaa\u{65e5}       \u{1c}\u{65e5}",
    ),
    (
        "\u{a0}\u{9}aaaaaaaaaaaaaaa\u{9}a\u{65e5}fhhhhhhhhhh\u{a}hhh\u{a0}dddddddddddddd",
        "  Files:   ",
        "           ",
        "  Files:   \u{a0}       aaaaaaaaaaaaaaa a\u{65e5}fhhhhhhhhhh hhh\u{a0}dddddddddddddd",
    ),
    (
        "\u{9}  \u{a}-\u{e9}fffffffffffffbbbbbbbbbbbbbbffffffffffffffffff\u{9}-aa  \u{a}cccccccccccccccccc\u{9}-\u{a0}\u{65e5}\u{2003}dddddddddddddddddddddddddddd eeeeeeeeeeeeeeeeeeeeffffffffffffffffff\u{65e5}ccccccccccccccccccccccccccccbbbbbbbbbbbbbbbbbbgcccccccccc",
        "",
        "           ",
        "           -\u{e9}fffffffffffffbbbbbbbbbbbbbbffffffffffffffffff -aa\u{a}           cccccccccccccccccc      -\u{a0}\u{65e5}\u{2003}dddddddddddddddddddddddddddd\u{a}           eeeeeeeeeeeeeeeeeeeeffffffffffffffffff\u{65e5}ccccccccccccccccccccccccccccbbbbbbbbbbbbbbbbbbgcccccccccc",
    ),
    ("dddddddddddb-eeeeeee\u{a}", "", "", "dddddddddddb-eeeeeee"),
    (
        "\u{a0}\u{a0}hhhhhhhhhhhhhhhhhhhhhhhhhcccccccccccccccccccccccccccccca\u{1c}ghhhhhhhhggggggggggggbbbbbb\u{a}dddddddddddddddddddddddgggggggggccccccccccccceeeeeeeeeeeeeeeeeeeeeccccccccccccccccccchhhhhhhhhhhhhhhhhhhhhhh",
        "  Licence: ",
        "           ",
        "  Licence: \u{a0}\u{a0}hhhhhhhhhhhhhhhhhhhhhhhhhcccccccccccccccccccccccccccccca\u{1c}ghhhhhhhhggggggggggggbbbbbb\u{a}           dddddddddddddddddddddddgggggggggccccccccccccceeeeeeeeeeeeeeeeeeeeeccccccccccccccccccchhhhhhhhhhhhhhhhhhhhhhh",
    ),
    (
        "ffffffffffffffffffffffffffffff\u{1c}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbggggggggggggggggggggggggdddddddddffffffffffffff-hhhhhhhhhhhhhhhhh\u{9}\u{e9}ccccccccccccccccaaaaaaaaaaaaaaaaaaaaaaaeeeeeedddddddddddddddddda-\u{9}ccccchhhhhhhhhhhhhhhhhhhhhh\u{9}\u{2003}\u{1c}\u{a}",
        "",
        "",
        "ffffffffffffffffffffffffffffff\u{1c}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbggggggggggggggggggggggggdddddddddffffffffffffff-hhhhhhhhhhhhhhhhh\u{a}\u{e9}ccccccccccccccccaaaaaaaaaaaaaaaaaaaaaaaeeeeeedddddddddddddddddda-\u{a}ccccchhhhhhhhhhhhhhhhhhhhhh     \u{2003}\u{1c}",
    ),
];

#[test]
fn fill_is_what_python_wrote() {
    for (text, initial, subsequent, want) in CASES {
        let w = Wrap {
            width: Count(76),
            initial_indent: initial,
            subsequent_indent: subsequent,
        };
        assert_eq!(
            w.fill(text),
            want,
            "{text:?} at {initial:?} / {subsequent:?}"
        );
    }
}

#[test]
fn plain_is_the_header_and_text_wrap() {
    let text = CASES[16].0;
    let w = Wrap {
        width: Count(76),
        initial_indent: "",
        subsequent_indent: "",
    };
    assert_eq!(Wrap::PLAIN.fill(text), w.fill(text));
}
