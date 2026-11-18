//! **fontconfig's font directories, read from its own configuration**, with no
//! libfontconfig: on Linux the configuration is what says which fonts are
//! installed, and where one is found its directories are the scan's list. Read
//! from fontconfig 2.15.0's source, which the user documentation
//! (`[[source: fontconfig-user]]`) describes more narrowly in places:
//!
//! * **The file.** `FONTCONFIG_FILE`, else `fonts.conf`; a name that is not
//!   absolute is looked for along `FONTCONFIG_PATH` and then the configuration
//!   directory, one that starts with `~` is `HOME` and the rest
//!   (`[[source: fontconfig-fccfg-2-15-0]]` `FcConfigGetFilename`,
//!   `FcConfigGetPath`), and a file that cannot be opened for reading is
//!   passed over for the next, as `FcConfigFileExists` passes over one
//!   that `access(R_OK)` refuses; baud opens the file to ask, which waits
//!   on a named FIFO that has no writer yet, where `access` does not. A
//!   root file that is a named pipe is read under the name given, as
//!   `FcConfigRealFilename` leaves it, so `FONTCONFIG_FILE=<(...)` works.
//!   The directory is `SYSCONFDIR/fonts`
//!   (`[[source: fontconfig-configure-2-15-0]]`, `--with-baseconfigdir`);
//!   baud takes `/etc/fonts`, and a fontconfig built for another directory is
//!   reached through the two variables, as fontconfig itself reads them.
//! * **`<include>`** reads a file, or a directory's files whose names begin
//!   with an ASCII digit and end in `.conf`, in byte order; a file already
//!   read, by its real path, is not read again; `prefix="xdg"` puts the name
//!   under `$XDG_CONFIG_HOME` (`[[source: fontconfig-fcxml-2-15-0]]`
//!   `FcParseInclude`, `FcConfigParseAndLoadDir`, `_FcConfigParse`). The
//!   user's own `fonts.conf` is reached this way, through the system's
//!   `conf.d/50-user.conf` (`[[source: fontconfig-50-user-conf-2-15-0]]`).
//! * **`<dir>`** adds a directory. `prefix="xdg"` adds it under
//!   `$XDG_DATA_HOME` and under every `$XDG_DATA_DIRS` entry, `"relative"`
//!   under the including file's own directory, `"default"` or `"cwd"` (or
//!   none) as written, relative to the working directory (`FcParseDir`
//!   and the prefix rules it calls). A leading `~` is `HOME` and the rest,
//!   and the name is made absolute and canonical
//!   (`[[source: fontconfig-fcstr-2-15-0]]` `FcStrCopyFilename`,
//!   `FcStrCanonAbsoluteFilename`). A directory added again moves to the end
//!   (`FcStrSetAddFilenamePairWithSalt`: "Override maps with new one if
//!   exists"). `<remap-dir>` adds its directory the same way, and
//!   `<reset-dirs/>` removes every directory added so far.
//! * **The variables** are read as fontconfig reads them: an `XDG_*_HOME`
//!   that is set counts even when empty, and with none set, `HOME` and
//!   `/.local/share` or `/.config`. A `<dir prefix="xdg">` under an empty
//!   `XDG_DATA_HOME` is its name alone, made absolute against the working
//!   directory, as `FcStrBuildFilename` writes no `/` after an empty first
//!   part; an `<include prefix="xdg">` under an empty `XDG_CONFIG_HOME` is
//!   `/` and its name, as `FcParseInclude` writes the `/` itself.
//!
//! * **`<selectfont>`** (`[[source: fontconfig-fcxml-2-15-0]]`
//!   `FcParseAcceptRejectFont`, `[[source: fontconfig-fccfg-2-15-0]]`
//!   `FcConfigAddCache`): each `<glob>` and `<pattern>` of an `<acceptfont>`
//!   or `<rejectfont>`, in any file read, is a rule, and an accepting rule
//!   wins. A glob, made absolute as a `<dir>` is, tests every font file and
//!   every directory below a configured one by the path the scan spells
//!   ([`Select::path`]); the user documentation's "globs only apply to
//!   directories" is not what the code does. A pattern tests each face as
//!   `FcListPatternMatchAny` does (`[[source: fontconfig-fclist-2-15-0]]`),
//!   over [`FaceView`]: the objects baud reads from a face, family, full
//!   name, PostScript name, file, index, outline, scalable, colour, variable
//!   and font format, each as FreeType and fontconfig set it.
//! * **`<alias>`** (`[[source: fontconfig-fcxml-2-15-0]]` `FcParseAlias`,
//!   `[[source: fontconfig-fccfg-2-15-0]]` `FcConfigSubstitute`): each, in
//!   any file read, is a rule that puts other families before, after and
//!   at the end of a family it names, each bound as the rule says, which
//!   a name is looked up through in the order fontconfig's match ranks
//!   them ([`Aliases`]).
//!
//! **Departures in the patterns, open.** A pattern that names any other
//! object -- `lang`, `charset`, `style`, `weight` and the rest a face has --
//! matches no face, where fontconfig tests it. A `<const>` is not resolved,
//! which matters only for `index`. A variable face is one face here, kept
//! while fontconfig would keep either its variable pattern or its named
//! instances' (`variable` false), with the instances' own names not read.
//! Names are the records baud's name reader decodes, which leaves out a
//! few platform encodings fontconfig transcodes, and strings are compared
//! by default caseless match with spaces dropped, close to fontconfig's
//! case folding. `<match target="scan">` edits, which fontconfig applies
//! to a face's pattern before `<selectfont>` tests it
//! (`[[source: fontconfig-user]]` `<selectfont>`,
//! `[[source: fontconfig-fcdir-2-15-0]]` `FcFileScanFontConfig`), are not
//! read, so a pattern is tested against the face as FreeType reads it.
//!
//! The XML is read by a small reader of its own that knows elements,
//! attributes, character data, the five predefined entities and character
//! references, comments, CDATA, processing instructions and a DOCTYPE; at the
//! first thing it cannot read it stops, keeping what came before. That is
//! fontconfig's reading only of a file included with `ignore_missing="yes"`,
//! as `conf.d` and the user's `fonts.conf` are: its streaming parser has
//! acted on what came before, and the error is not passed up.
//!
//! **Two departures, open.** fontconfig drops a whole configuration that
//! fails -- a malformed root file, or an `<include>` without
//! `ignore_missing` that is missing or malformed -- and loads
//! `FcInitFallbackConfig` instead (`[[source: fontconfig-fcinit-2-15-0]]`);
//! baud keeps what it read before the failure. And when no `fonts.conf` is
//! found, that fallback reads its default directories, `$XDG_DATA_HOME/fonts`,
//! the system's `conf.d` and the user's `fontconfig/conf.d` and
//! `fontconfig/fonts.conf`; baud uses its fixed list and reads no
//! configuration.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The configuration directory baud looks in: `SYSCONFDIR/fonts` with the
/// `/etc` a system's own fontconfig reads.
pub const CONFIG_DIR: &str = "/etc/fonts";

/// What fontconfig reads from the process to find and read its files.
pub struct Env {
    pub home: Option<OsString>,
    pub xdg_data_home: Option<OsString>,
    pub xdg_data_dirs: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    /// `FONTCONFIG_FILE`.
    pub file: Option<OsString>,
    /// `FONTCONFIG_PATH`.
    pub path: Option<OsString>,
    pub config_dir: PathBuf,
    pub cwd: Option<PathBuf>,
    pub locale: Locale,
}

impl Env {
    /// This process's.
    pub fn from_process() -> Env {
        let var = std::env::var_os;
        Env {
            home: var("HOME"),
            xdg_data_home: var("XDG_DATA_HOME"),
            xdg_data_dirs: var("XDG_DATA_DIRS"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            file: var("FONTCONFIG_FILE"),
            path: var("FONTCONFIG_PATH"),
            config_dir: CONFIG_DIR.into(),
            cwd: std::env::current_dir().ok(),
            locale: Locale {
                fc_lang: var("FC_LANG"),
                lc_all: var("LC_ALL"),
                lc_ctype: var("LC_CTYPE"),
                lang: var("LANG"),
            },
        }
    }
}

/// **The variables fontconfig reads its default languages from**, as the
/// process had them when the configuration was read.
#[derive(Clone, Default)]
pub struct Locale {
    /// `FC_LANG`.
    pub fc_lang: Option<OsString>,
    /// `LC_ALL`.
    pub lc_all: Option<OsString>,
    /// `LC_CTYPE`.
    pub lc_ctype: Option<OsString>,
    /// `LANG`.
    pub lang: Option<OsString>,
}

impl Locale {
    /// **The `lang` values fontconfig puts in a pattern before its
    /// `<alias>` rules test it**: `FcGetDefaultLangs`
    /// (`[[source: fontconfig-fcdefault-2-15-0]]` line 50) reads the first
    /// of `FC_LANG`, `LC_ALL`, `LC_CTYPE` (unless it is just `UTF-8`) and
    /// `LANG` that is set and not empty, splits it at `:`, and keeps each
    /// part `FcLangNormalize` makes a tag of (`FcStrSetAddLangs`,
    /// `[[source: fontconfig-fcstr-2-15-0]]` line 1499), or `en` when none
    /// is; `FcConfigSubstituteWithPat`
    /// (`[[source: fontconfig-fccfg-2-15-0]]` lines 1985-2033) adds them in
    /// order, stopping at the first the pattern already holds, caseless, or
    /// at `und`.
    pub fn langs(&self) -> Vec<String> {
        let set = |v: &Option<OsString>| v.clone().filter(|v| !v.is_empty());
        let ctype = self
            .lc_ctype
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("UTF-8"));
        let langs = set(&self.fc_lang)
            .or_else(|| set(&self.lc_all))
            .or_else(|| set(&ctype))
            .or_else(|| set(&self.lang));
        let mut tags: Vec<String> = Vec::new();
        let text = langs.map(|l| l.to_string_lossy().into_owned());
        for part in text.iter().flat_map(|t| t.split(':')) {
            match normalize_lang(part) {
                Some(t) if !tags.contains(&t) => tags.push(t),
                _ => {}
            }
        }
        if tags.is_empty() {
            tags.push("en".to_owned());
        }
        let mut pattern: Vec<String> = Vec::new();
        for t in tags {
            if pattern
                .iter()
                .any(|v| caseless(v, &t) || caseless(v, "und"))
            {
                break;
            }
            pattern.push(t);
        }
        pattern
    }
}

/// **A locale name as fontconfig makes a language tag of it**
/// (`FcLangNormalize`, `[[source: fontconfig-fclang-2-15-0]]` line 182):
/// `C`, `C.UTF-8`, `C.utf8` and `POSIX` are `en`; otherwise
/// `language[_territory][.codeset][@modifier]`, the codeset dropped, the
/// territory joined by `-`, kept only where fontconfig's own table has the
/// tag with it ([`LANG_TAGS`]), the modifier likewise; a language not in
/// the table is kept whole, lowercased. An empty name, a language of other
/// than two or three letters, or a territory of other than two or three
/// (or a `z` and up to four), is no tag.
fn normalize_lang(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if ["C", "C.UTF-8", "C.utf8", "POSIX"]
        .iter()
        .any(|c| caseless(name, c))
    {
        return Some("en".to_owned());
    }
    let (rest, modifier) = match name.split_once('@') {
        Some((r, m)) => (r, Some(m)),
        None => (name, None),
    };
    let rest = rest.split_once('.').map_or(rest, |(r, _)| r);
    let (language, territory) = match rest.split_once('_').or_else(|| rest.split_once('-')) {
        Some((l, t)) => (l, Some(t)),
        None => (rest, None),
    };
    let tag_len = |s: &str| (2..=3).contains(&s.len());
    let region = |t: &str| tag_len(t) || (t.starts_with('z') && t.len() < 5);
    if !tag_len(language) || !territory.is_none_or(region) {
        return None;
    }
    let with = |t: Option<&str>, m: Option<&str>| {
        let mut s = language.to_owned();
        for (mark, part) in [('-', t), ('@', m)] {
            if let Some(p) = part {
                s.push(mark);
                s.push_str(p);
            }
        }
        s
    };
    let known = |s: &str| LANG_TAGS.iter().any(|tag| caseless(tag, s));
    let tried = [
        territory.map(|t| with(Some(t), modifier)),
        modifier.map(|m| with(None, Some(m))),
        Some(language.to_owned()),
    ];
    let found = tried.into_iter().flatten().find(|s| known(s));
    Some(found.unwrap_or_else(|| with(territory, modifier).to_lowercase()))
}

/// **The language tags fontconfig's table holds**: the `.orth` files
/// `fc-lang/meson.build` builds it from
/// (`[[source: fontconfig-fc-lang-meson-2-15-0]]`), each name's `_` written
/// `-`, as `FcLangSetIndex` compares them, caseless.
const LANG_TAGS: [&str; 279] = [
    "aa", "ab", "af", "agr", "ak", "am", "an", "anp", "ar", "as", "ast", "av", "ay", "ayc",
    "az-az", "az-ir", "ba", "be", "bem", "ber-dz", "ber-ma", "bg", "bh", "bhb", "bho", "bi", "bin",
    "bm", "bn", "bo", "br", "brx", "bs", "bua", "byn", "ca", "ce", "ch", "chm", "chr", "ckb",
    "cmn", "co", "crh", "cs", "csb", "cu", "cv", "cy", "da", "de", "doi", "dsb", "dv", "dz", "ee",
    "el", "en", "eo", "es", "et", "eu", "fa", "fat", "ff", "fi", "fil", "fj", "fo", "fr", "fur",
    "fy", "ga", "gd", "gez", "gl", "gn", "gu", "gv", "ha", "hak", "haw", "he", "hi", "hif", "hne",
    "ho", "hr", "hsb", "ht", "hu", "hy", "hz", "ia", "id", "ie", "ig", "ii", "ik", "io", "is",
    "it", "iu", "ja", "jv", "ka", "kaa", "kab", "ki", "kj", "kk", "kl", "km", "kn", "ko", "kok",
    "kr", "ks", "ku-am", "ku-iq", "ku-ir", "kum", "ku-tr", "kv", "kw", "kwm", "ky", "la", "lah",
    "lb", "lez", "lg", "li", "lij", "ln", "lo", "lt", "lv", "lzh", "mag", "mai", "mfe", "mg", "mh",
    "mhr", "mi", "miq", "mjw", "mk", "ml", "mn-cn", "mni", "mn-mn", "mnw", "mo", "mr", "ms", "mt",
    "my", "na", "nan", "nb", "nds", "ne", "ng", "nhn", "niu", "nl", "nn", "no", "nqo", "nr", "nso",
    "nv", "ny", "oc", "om", "or", "os", "ota", "pa", "pap-an", "pap-aw", "pa-pk", "pl", "ps-af",
    "ps-pk", "pt", "qu", "quz", "raj", "rif", "rm", "rn", "ro", "ru", "rw", "sa", "sah", "sat",
    "sc", "sco", "sd", "se", "sel", "sg", "sgs", "sh", "shn", "shs", "si", "sid", "sk", "sl", "sm",
    "sma", "smj", "smn", "sms", "sn", "so", "sq", "sr", "ss", "st", "su", "sv", "sw", "syr", "szl",
    "ta", "tcy", "te", "tg", "th", "the", "ti-er", "ti-et", "tig", "tk", "tl", "tn", "to", "tpi",
    "tr", "ts", "tt", "tw", "ty", "tyv", "ug", "uk", "und-zmth", "und-zsye", "unm", "ur", "uz",
    "ve", "vi", "vo", "vot", "wa", "wae", "wal", "wen", "wo", "xh", "yap", "yi", "yo", "yue",
    "yuw", "za", "zh-cn", "zh-hk", "zh-mo", "zh-sg", "zh-tw", "zu",
];

/// **The font directories fontconfig's configuration names**, in its
/// order; `None` when no configuration file is found, where fontconfig
/// would load its fallback configuration (one of the module's open
/// departures).
#[cfg(test)]
pub fn dirs(env: &Env) -> Option<Vec<PathBuf>> {
    load(env).map(|c| c.dirs)
}

/// What fontconfig's configuration says of the fonts: the directories to
/// scan, in order, and which of what they hold to keep ([`Select`]).
pub struct Config {
    pub dirs: Vec<PathBuf>,
    pub select: Select,
    pub aliases: Aliases,
}

/// **The configuration, read**: [`dirs`], and the `<selectfont>` and
/// `<alias>` rules of every file read; `None` as [`dirs`] is.
pub fn load(env: &Env) -> Option<Config> {
    let name = env.file.clone().unwrap_or_else(|| "fonts.conf".into());
    let mut load = Load {
        env,
        read: HashSet::new(),
        dirs: Vec::new(),
        select: Select::default(),
        aliases: Aliases::of_locale(&env.locale),
        pending: Pending::default(),
    };
    let root = load.filename(&name)?;
    load.parse_root(&root);
    Some(Config {
        dirs: load.dirs,
        select: load.select,
        aliases: load.aliases,
    })
}

/// A reading of the configuration in progress.
struct Load<'e> {
    env: &'e Env,
    /// The files and directories read, by real path.
    read: HashSet<PathBuf>,
    dirs: Vec<PathBuf>,
    select: Select,
    aliases: Aliases,
    /// The parts of a `<selectfont>` or `<alias>` rule read but not yet
    /// taken by the element they are inside.
    pending: Pending,
}

/// What fontconfig's parse stack holds between a rule's parts and the
/// element that takes them: a `<patelt>`'s values, a `<pattern>`'s
/// elements, an `<acceptfont>`'s or `<rejectfont>`'s globs and patterns;
/// an `<alias>`'s own families, its `<prefer>`, `<accept>` and `<default>`,
/// the families inside one of those, its `<test>`s, and the values inside
/// a `<test>`.
#[derive(Default)]
struct Pending {
    values: Vec<Value>,
    elements: Vec<Patelt>,
    selectors: Vec<Selector>,
    families: Vec<String>,
    alias_families: Vec<String>,
    prefer: Option<Vec<String>>,
    accept: Option<Vec<String>>,
    default: Option<Vec<String>>,
    alias_tests: Vec<TestRead>,
    test_values: Vec<Literal>,
}

/// A value inside a `<test>`: a `<string>`'s text, or any other expression.
enum Literal {
    Str(String),
    Other,
}

/// An alias's `<test>` as read: one baud evaluates, or one it does not.
enum TestRead {
    Evaluated(Test),
    Unevaluated,
}

/// **An `<alias>`'s `<test>`** (`FcParseTest`,
/// `[[source: fontconfig-fcxml-2-15-0]]` line 2704): the object it reads,
/// how it compares, which of the object's values must match, and the one
/// string it compares them with.
struct Test {
    qual: Qual,
    object: Object,
    compare: Compare,
    blanks: Blanks,
    string: String,
}

/// A test's `qual`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Qual {
    Any,
    All,
    First,
    NotFirst,
}

/// The pattern object a test reads: the three a pattern baud substitutes
/// holds, or one it does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Object {
    Family,
    Lang,
    Prgname,
    Absent,
}

/// A test's `compare` (`FcConfigLexCompare`, line 2698), as
/// `FcConfigCompareValue` (`[[source: fontconfig-fccfg-2-15-0]]` line 955)
/// applies it to two strings: `less`, `less_eq`, `more` and `more_eq`
/// compare no string, so are never true.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Compare {
    Eq,
    NotEq,
    Contains,
    NotContains,
    Ordered,
}

/// Whether a test's `ignore-blanks` is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Blanks {
    Kept,
    Ignored,
}

impl Test {
    /// **The test, read** from its attributes and the values inside it:
    /// `Err` where fontconfig drops it with a warning, so the alias goes
    /// on without it -- a `target`, `qual` or `compare` it does not know,
    /// no `name`, no value; `Ok(Unevaluated)` where it is a test baud does
    /// not evaluate: a `target` of `font` or `scan`, or an expression other
    /// than one `<string>`.
    fn read(e: &Element, values: Vec<Literal>) -> Result<TestRead, ()> {
        let target = match e.attr("target") {
            None | Some("default" | "pattern") => true,
            Some("font" | "scan") => false,
            Some(_) => return Err(()),
        };
        let qual = match e.attr("qual") {
            None | Some("any") => Qual::Any,
            Some("all") => Qual::All,
            Some("first") => Qual::First,
            Some("not_first") => Qual::NotFirst,
            Some(_) => return Err(()),
        };
        let object = match e.attr("name").ok_or(())? {
            "family" => Object::Family,
            "lang" => Object::Lang,
            "prgname" => Object::Prgname,
            _ => Object::Absent,
        };
        let compare = match e.attr("compare") {
            None | Some("eq") => Compare::Eq,
            Some("not_eq") => Compare::NotEq,
            Some("contains") => Compare::Contains,
            Some("not_contains") => Compare::NotContains,
            Some("less" | "less_eq" | "more" | "more_eq") => Compare::Ordered,
            Some(_) => return Err(()),
        };
        let blanks = match e.attr("ignore-blanks").map(name_bool) {
            Some(Fc3::True) => Blanks::Ignored,
            _ => Blanks::Kept,
        };
        let string = match (values.len(), values.into_iter().next()) {
            (0, _) => return Err(()),
            (1, Some(Literal::Str(s))) if target => s,
            _ => return Ok(TestRead::Unevaluated),
        };
        Ok(TestRead::Evaluated(Test {
            qual,
            object,
            compare,
            blanks,
            string,
        }))
    }

    /// Whether `value` compares with the test's string.
    fn compares(&self, value: &str) -> bool {
        let equal = match self.blanks {
            Blanks::Ignored => blank_caseless(value, &self.string),
            Blanks::Kept => caseless(value, &self.string),
        };
        match self.compare {
            Compare::Eq => equal,
            Compare::NotEq => !equal,
            Compare::Contains => caseless_contains(value, &self.string),
            Compare::NotContains => !caseless_contains(value, &self.string),
            Compare::Ordered => false,
        }
    }

    /// **Where the test matches `values`**, the object's values in the
    /// pattern, as `FcConfigMatchValueList` (line 1679) and its caller
    /// decide: the first value that compares (under `all`, the first value,
    /// every one comparing), `first` and `not_first` held to it being, or
    /// not being, the first; `None` when the rule fails. An object the
    /// pattern does not hold matches under `all` alone, at no value.
    fn at(&self, values: &[&str]) -> Option<Matched> {
        if values.is_empty() {
            return (self.qual == Qual::All).then_some(Matched::Nowhere);
        }
        let mut at = None;
        for (k, v) in values.iter().enumerate() {
            if self.compares(v) {
                at = at.or(Some(k));
                if self.qual != Qual::All {
                    break;
                }
            } else if self.qual == Qual::All {
                return None;
            }
        }
        match (at, self.qual) {
            (Some(0), Qual::NotFirst) => None,
            (Some(1..), Qual::First) => None,
            (at, _) => at.map(Matched::At),
        }
    }
}

/// Where a test matched: at a value's place, or, for an object the pattern
/// does not hold, nowhere.
#[derive(Clone, Copy)]
enum Matched {
    At(usize),
    Nowhere,
}

/// **fontconfig's `<alias>` rules, in the order they were read**
/// (`[[source: fontconfig-fcxml-2-15-0]]` `FcParseAlias`, line 1933): each
/// tests the pattern a family is asked for by and edits its family list as
/// `[[source: fontconfig-fccfg-2-15-0]]` `FcConfigSubstitute` applies it
/// ([`Aliases::order`]).
///
/// * An alias whose `binding` is not `weak`, `strong` or `same` is dropped,
///   as `FcConfigLexBinding` refuses it. Otherwise its binding is set on
///   the families it adds (`FcConfigAdd`, line 1785: `same` takes the
///   binding of the family it is placed by, weak at the end), and it
///   decides which family a face is drawn from ([`Aliases::order`]).
/// * An alias with no `<prefer>`, `<accept>` or `<default>` is no rule;
///   one with no `<family>` is an error and no rule. Of several
///   `<prefer>`s (or the others) the first is kept, as the parse stack
///   keeps the last it pops.
/// * Its `<test>`s are evaluated against the pattern baud asks with: the
///   family, bound strong, the languages of the locale ([`Locale::langs`])
///   and `prgname` `baud`, fontconfig's pattern before its rules; a test of
///   any other object finds none.
/// * Several `<family>`s, which fontconfig warns "isn't supported and may
///   not work as expected", match as `FcConfigMatchValueList` walks them:
///   only when the last is in the list.
///
/// **A departure, open.** A `<test>` whose expression is anything but one
/// `<string>`, or whose `target` is `font` or `scan`, is not evaluated, and
/// its alias is not applied, with a warning that says how many were not
/// ([`Aliases::unapplied`]).
///
/// They are read once, with the rest of the configuration, when the
/// installed fonts are first looked at; an expansion is at most
/// [`ALIAS_LIST_MAX`] families.
#[derive(Default)]
pub struct Aliases {
    rules: Vec<Alias>,
    /// The pattern's `lang` values.
    langs: Vec<String>,
    /// Each alias not applied for a test baud does not evaluate.
    unapplied: Vec<Unapplied>,
}

/// An alias not applied: one of its tests is not evaluated.
struct Unapplied;

/// One `<alias>`: its tests, the families it tests for, its binding, and
/// the three lists it adds.
struct Alias {
    tests: Vec<Test>,
    families: Vec<String>,
    binding: Binding,
    prefer: Vec<String>,
    accept: Vec<String>,
    default: Vec<String>,
}

/// An alias's `binding`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Weak,
    Strong,
    Same,
}

/// How a family in the pattern is bound: the name asked for strong, the
/// families an alias adds as its binding says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bound {
    Weak,
    Strong,
}

/// A family in the pattern, and how it is bound.
#[derive(Clone)]
struct Family {
    name: String,
    bound: Bound,
}

/// The most families an expansion holds. fontconfig sets no bound; the
/// configurations shipped with it add a few dozen at most to a generic
/// name, and a configuration that adds more is cut here rather than grow
/// the list without end.
const ALIAS_LIST_MAX: usize = 256;

impl Aliases {
    /// No rules yet, for a pattern of the locale's languages.
    fn of_locale(locale: &Locale) -> Aliases {
        Aliases {
            rules: Vec::new(),
            langs: locale.langs(),
            unapplied: Vec::new(),
        }
    }

    /// **The pattern's family list `name` becomes** under fontconfig's
    /// aliases: `name` alone at first, bound strong, then each alias in
    /// order, over the list the ones before it left. An alias applies when
    /// each of its tests matches and its family test does: the families it
    /// names are looked up in order, each where the list first holds it by
    /// `FcStrCmpIgnoreBlanksAndCase`, one missing undoing the matches
    /// before it, so it applies only when the last is there, at the first
    /// of the run of matches that ends with it. Its `<prefer>` goes before
    /// that family -- or before the family a `<test name="family">` matched
    /// first, as the rule's first family test places the edits -- its
    /// `<accept>` after it and its `<default>` at the end (`FcOpPrepend`,
    /// `FcOpAppend`, `FcOpAppendLast`). Each alias is applied once, so one
    /// that names another adds what that one adds only where that one comes
    /// later, and a cycle ends with the list. A family may appear twice, as
    /// fontconfig keeps it. At most [`ALIAS_LIST_MAX`] families.
    fn pattern(&self, name: &str) -> Vec<Family> {
        let mut list = vec![Family {
            name: name.to_owned(),
            bound: Bound::Strong,
        }];
        let langs: Vec<&str> = self.langs.iter().map(String::as_str).collect();
        for a in &self.rules {
            let names: Vec<&str> = list.iter().map(|f| f.name.as_str()).collect();
            let mut placed = None;
            let tested = a.tests.iter().all(|t| {
                let values: &[&str] = match t.object {
                    Object::Family => &names,
                    Object::Lang => &langs,
                    Object::Prgname => &["baud"],
                    Object::Absent => &[],
                };
                match t.at(values) {
                    Some(Matched::At(k)) if t.object == Object::Family => {
                        placed = placed.or(Some(k));
                        true
                    }
                    Some(_) => true,
                    None => false,
                }
            });
            let mut walked = None;
            for family in &a.families {
                match names.iter().position(|n| blank_caseless(n, family)) {
                    Some(k) => walked = walked.or(Some(k)),
                    None => walked = None,
                }
            }
            let (true, Some(walked)) = (tested, walked) else {
                continue;
            };
            let at = placed.unwrap_or(walked);
            let (by, end) = match a.binding {
                Binding::Weak => (Bound::Weak, Bound::Weak),
                Binding::Strong => (Bound::Strong, Bound::Strong),
                Binding::Same => (list[at].bound, Bound::Weak),
            };
            let bound = |names: &[String], bound: Bound| -> Vec<Family> {
                names
                    .iter()
                    .map(|n| Family {
                        name: n.clone(),
                        bound,
                    })
                    .collect()
            };
            list.splice(at..at, bound(&a.prefer, by));
            let after = at + a.prefer.len() + 1;
            list.splice(after..after, bound(&a.accept, by));
            list.extend(bound(&a.default, end));
            list.truncate(ALIAS_LIST_MAX);
        }
        list
    }

    /// **The family list `name` becomes** ([`Aliases::pattern`]), by name.
    #[cfg(test)]
    pub fn expand(&self, name: &str) -> Vec<String> {
        self.pattern(name).into_iter().map(|f| f.name).collect()
    }

    /// **The families a face is looked for by, in the order fontconfig's
    /// match ranks them** (`[[source: fontconfig-fcmatch-2-15-0]]` lines
    /// 323-350): the strong-bound families in the list's order, then the
    /// weak-bound ones, each once. A family bound strong ranks above
    /// every weak one, so a `<prefer>` bound `strong`, or `same` by a strong
    /// family, goes ahead of an installed family it is placed before, and
    /// a weak one does not. Between the two, fontconfig's match ranks a
    /// face's languages, which baud does not score.
    pub fn order(&self, name: &str) -> Vec<String> {
        let list = self.pattern(name);
        let mut out: Vec<String> = Vec::new();
        for bound in [Bound::Strong, Bound::Weak] {
            for f in list.iter().filter(|f| f.bound == bound) {
                if !out.iter().any(|o| blank_caseless(o, &f.name)) {
                    out.push(f.name.clone());
                }
            }
        }
        out
    }

    /// The families [`Aliases::order`] adds to `name`, each once, in its
    /// order: what is tried when `name` itself is no installed font.
    pub fn instead_of(&self, name: &str) -> Vec<String> {
        let mut out = self.order(name);
        out.retain(|f| !blank_caseless(f, name));
        out
    }

    /// **What to say of the aliases not applied** for a test baud does not
    /// evaluate, when there are any.
    pub fn unapplied(&self) -> Option<String> {
        match self.unapplied.len() {
            0 => None,
            n => Some(format!(
                "fontconfig: {n} <alias> rule{} not applied: a <test> with an expression \
                 other than one <string>, or with target font or scan, is not evaluated",
                if n == 1 { "" } else { "s" }
            )),
        }
    }

    /// The `<alias>` rules of one document, as a file of the configuration
    /// holding only it gives them, under no locale.
    #[cfg(test)]
    pub fn of_document(xml: &str) -> Aliases {
        Aliases::of_document_in(xml, &Locale::default())
    }

    /// [`Aliases::of_document`] under `locale`.
    #[cfg(test)]
    pub fn of_document_in(xml: &str, locale: &Locale) -> Aliases {
        let env = Env {
            home: None,
            xdg_data_home: None,
            xdg_data_dirs: None,
            xdg_config_home: None,
            file: None,
            path: None,
            config_dir: PathBuf::new(),
            cwd: None,
            locale: locale.clone(),
        };
        let mut load = Load {
            env: &env,
            read: HashSet::new(),
            dirs: Vec::new(),
            select: Select::default(),
            aliases: Aliases::of_locale(&env.locale),
            pending: Pending::default(),
        };
        load.read_bytes(xml.as_bytes(), Path::new("/fonts.conf"));
        load.aliases
    }
}

/// `FcStrCmpIgnoreBlanksAndCase`'s equality: the two names with their
/// spaces left out, by default caseless match.
fn blank_caseless(a: &str, b: &str) -> bool {
    baud_domain::caseless_eq(
        a.chars().filter(|&c| c != ' '),
        b.chars().filter(|&c| c != ' '),
    )
}

/// `FcStrCmpIgnoreCase`'s equality, by default caseless match.
fn caseless(a: &str, b: &str) -> bool {
    baud_domain::caseless_eq(a.chars(), b.chars())
}

/// `FcStrStrIgnoreCase`: whether `hay` holds `needle`, by default caseless
/// match.
fn caseless_contains(hay: &str, needle: &str) -> bool {
    let fold = |s: &str| -> Vec<char> { s.chars().flat_map(baud_domain::case_fold).collect() };
    let (hay, needle) = (fold(hay), fold(needle));
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// One `<acceptfont>` or `<rejectfont>` child.
enum Selector {
    Glob(Vec<u8>),
    Pattern(Pattern),
}

/// **`<selectfont>`, gathered over the whole configuration**
/// (`[[source: fontconfig-fccfg-2-15-0]]` `FcConfigGlobAdd`,
/// `FcConfigAcceptFilename`, `FcConfigPatternsAdd`, `FcConfigAcceptFont`):
/// a path or a face an `<acceptfont>` matches is kept, else one a
/// `<rejectfont>` matches is dropped, else it is kept -- whatever order the
/// rules came in, and `<reset-dirs/>` leaves them.
#[derive(Default)]
pub struct Select {
    accept_globs: Vec<Vec<u8>>,
    reject_globs: Vec<Vec<u8>>,
    accept: Vec<Pattern>,
    reject: Vec<Pattern>,
}

/// Whether a selector keeps a path or a face.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kept {
    Yes,
    No,
}

impl Select {
    /// `FcConfigAcceptFilename`, for a font file or a directory below a
    /// configured one, as the scan spells it: the directory, `/`, the name.
    pub fn path(&self, p: &Path) -> Kept {
        use std::os::unix::ffi::OsStrExt;
        let s = p.as_os_str().as_bytes();
        if self.accept_globs.iter().any(|g| glob_match(g, s)) {
            return Kept::Yes;
        }
        match self.reject_globs.iter().any(|g| glob_match(g, s)) {
            true => Kept::No,
            false => Kept::Yes,
        }
    }

    /// Whether any rule tests a face's pattern, so a scan reads what one
    /// needs only when it is asked.
    pub fn tests_faces(&self) -> bool {
        !(self.accept.is_empty() && self.reject.is_empty())
    }

    /// `FcConfigAcceptFont` for face `f`. A variable face is one pattern
    /// with `variable` true and its named instances more with it false;
    /// baud keeps the face while either reading keeps it.
    pub fn face(&self, f: &FaceView) -> Kept {
        let read = |variable: Has| {
            let accepted = self.accept.iter().any(|p| p.matches(f, variable));
            let rejected = self.reject.iter().any(|p| p.matches(f, variable));
            match (accepted, rejected) {
                (true, _) | (false, false) => Kept::Yes,
                (false, true) => Kept::No,
            }
        };
        match f.variable {
            Has::Yes if read(Has::Yes) == Kept::Yes => Kept::Yes,
            _ => read(Has::No),
        }
    }
}

/// `FcStrGlobMatch` (`[[source: fontconfig-fcstr-2-15-0]]`): `*` any run of
/// bytes, `/` included, `?` any one byte, every other byte itself.
fn glob_match(glob: &[u8], s: &[u8]) -> bool {
    match glob.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => {
            if rest.is_empty() {
                return true;
            }
            (0..s.len()).any(|i| glob_match(rest, &s[i..]))
        }
        Some((b'?', rest)) => s.split_first().is_some_and(|(_, t)| glob_match(rest, t)),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(b, t)| b == c && glob_match(rest, t)),
    }
}

/// Whether a face has a property, as fontconfig's booleans read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Has {
    Yes,
    No,
}

/// **A face as fontconfig's pattern of it names it**, for the objects baud
/// reads (`[[source: fontconfig-fcfreetype-2-15-0]]`
/// `FcFreeTypeQueryFaceInternal`): its families (nameIDs 21, 16 and 1),
/// full names (4 and 18), PostScript name, file, index, and FreeType's
/// view of its outlines and colour ([`Has`]); `scalable` is outline or
/// colour, as fontconfig sets it.
#[derive(Clone, Debug)]
pub struct FaceView {
    pub families: Vec<String>,
    pub full_names: Vec<String>,
    pub postscript: Option<String>,
    pub file: String,
    pub index: FcIndex,
    pub outline: Has,
    pub color: Has,
    pub variable: Has,
    pub format: Option<&'static str>,
}

impl FaceView {
    /// **Face `index` of the file at `file`, as fontconfig's pattern names
    /// it.** The families are nameIDs 21, 16 and 1 and the full names 4 and
    /// 18, every language (`FcFreeTypeQueryFaceInternal`'s name walk);
    /// `fontformat` is the FreeType driver's name, "CFF" for an `'OTTO'`
    /// face and "TrueType" for a `0x00010000` or `'true'` one
    /// (`[[source: freetype-cffobjs-2-13-3]]`, `[[source: freetype-ttobjs-2-13-3]]`),
    /// none for `'typ1'`, which neither driver opens. `outline` and `color`
    /// are FreeType's face flags (`[[source: freetype-sfobjs-2-13-3]]`
    /// `sfnt_load_face()`, `[[source: freetype-ttsbit-2-13-3]]`):
    ///
    /// * **Outlines** are `glyf`, `CFF ` or `CFF2`, none when `CBLC` or
    ///   `CBDT` is present; with outlines a face with `sbix` is flagged
    ///   SBIX rather than scalable, and a face with neither outlines nor a
    ///   bitmap strike is scalable ("it has only empty glyphs then").
    ///   FreeType clips a strike count to the table's length,
    ///   (length - 8) / 48 for `CBLC`, `EBLC` and `bloc` and
    ///   (length - 8) / 4 for `sbix`, and drops a strike whose metrics
    ///   fail to load or whose ppem is zero. baud reads `numSizes` and
    ///   `numStrikes` unclipped, so a truncated table is read as having
    ///   strikes FreeType would not see.
    /// * **Colour** is a `CBLC` sbit table, an `sbix` one when there is no
    ///   `CBLC`, `EBLC` or `bloc`, or a `COLR` or `SVG ` table.
    pub fn of(d: &baud_font::Described<'_>, file: &str, index: FcIndex) -> FaceView {
        use baud_font::{Flavour, NameId, Tag};
        let has = |t: &[u8; 4]| d.table(Tag::from_bytes(*t)).is_some();
        let text = |f: baud_font::Family<'_>| f.chars().collect::<Option<String>>();
        let names = |ids: &[NameId]| -> Vec<String> {
            ids.iter()
                .flat_map(|&id| d.names(id).filter_map(text))
                .collect()
        };
        // The one sbit table FreeType reads: `CBLC`, else `EBLC`, else
        // `bloc`, else `sbix`.
        let sbit = [b"CBLC", b"EBLC", b"bloc", b"sbix"]
            .into_iter()
            .find(|t| has(t));
        let cblc = sbit == Some(b"CBLC");
        let sbix = sbit == Some(b"sbix");
        let outlines = (has(b"glyf") || has(b"CFF ") || has(b"CFF2")) && !(cblc || has(b"CBDT"));
        let strikes = sbit.is_some_and(|t| strikes(d, t) && (sbix || sbit_data(d)));
        // With outlines, `sbix` anywhere makes the face SBIX, not scalable;
        // a face then neither scalable nor with a strike is scalable.
        let outline = match (outlines && !has(b"sbix"), strikes) {
            (true, _) | (false, false) => Has::Yes,
            (false, true) => Has::No,
        };
        let color = match cblc || sbix || has(b"COLR") || has(b"SVG ") {
            true => Has::Yes,
            false => Has::No,
        };
        FaceView {
            families: names(&[
                NameId::WWS_FAMILY,
                NameId::TYPOGRAPHIC_FAMILY,
                NameId::FAMILY,
            ]),
            full_names: names(&[NameId::MAC_FULL, NameId::FULL]),
            postscript: d.postscript_name().and_then(text),
            file: file.to_owned(),
            index,
            outline,
            color,
            variable: match has(b"fvar") {
                true => Has::Yes,
                false => Has::No,
            },
            format: match d.flavour() {
                Flavour::Cff => Some("CFF"),
                Flavour::TrueType | Flavour::AppleTrue => Some("TrueType"),
                Flavour::AppleType1 => None,
            },
        }
    }
}

/// Whether table `tag` names at least one strike, read as FreeType reads
/// it: `numSizes` (`EBLC`, `CBLC`, `bloc`) or `numStrikes` (`sbix`) at
/// offset 4, in a table at least 8 bytes long. FreeType then sets an
/// `EBLC`-style count to zero without a data table ([`sbit_data`]).
fn strikes(d: &baud_font::Described<'_>, tag: &[u8; 4]) -> bool {
    use baud_font::bytes::Offset;
    d.table(baud_font::Tag::from_bytes(*tag)).is_some_and(|t| {
        t.as_slice().len() >= 8 && t.read::<u32>(Offset::of(4u32)).is_some_and(|n| n > 0)
    })
}

/// Whether the face has a bitmap data table for its `EBLC`, `CBLC` or
/// `bloc`: `CBDT`, `EBDT` or `bdat`.
fn sbit_data(d: &baud_font::Described<'_>) -> bool {
    [b"CBDT", b"EBDT", b"bdat"]
        .iter()
        .any(|t| d.table(baud_font::Tag::from_bytes(**t)).is_some())
}

/// A face's `index` as fontconfig's pattern holds it: its place in its
/// file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FcIndex(pub i64);

/// A value a `<patelt>` holds, after `FcPopValue`: what `<string>`,
/// `<int>`, `<double>` and `<bool>` read to, the types baud does not read
/// from a face, and `Void` for an element `FcPopValue` makes nothing of.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    Str(String),
    Int(i64),
    Double(f64),
    Bool(Fc3),
    Other(Other),
}

/// fontconfig's three-valued boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fc3 {
    False,
    True,
    DontCare,
}

/// A value of a type baud does not read from a face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Other {
    CharSet,
    LangSet,
    Range,
    Matrix,
    /// An unknown `<const>`, an expression, or any element `FcPopValue`
    /// gives no value for: it ends the `<patelt>`'s reading.
    Void,
}

/// The type a pattern object holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FcType {
    Str,
    Int,
    Double,
    Range,
    Bool,
    Matrix,
    CharSet,
    LangSet,
}

/// **fontconfig's objects by name and type**: the names
/// `[[source: fontconfig-fontconfig-h-2-15-0]]` defines and the types
/// `[[source: fontconfig-fcobjs-2-15-0]]` gives them. A name not here is an
/// object of fontconfig's own making that takes any type.
const FC_OBJECTS: [(&str, FcType); 55] = [
    ("family", FcType::Str),
    ("familylang", FcType::Str),
    ("style", FcType::Str),
    ("stylelang", FcType::Str),
    ("fullname", FcType::Str),
    ("fullnamelang", FcType::Str),
    ("slant", FcType::Int),
    ("weight", FcType::Range),
    ("width", FcType::Range),
    ("size", FcType::Range),
    ("aspect", FcType::Double),
    ("pixelsize", FcType::Double),
    ("spacing", FcType::Int),
    ("foundry", FcType::Str),
    ("antialias", FcType::Bool),
    ("hintstyle", FcType::Int),
    ("hinting", FcType::Bool),
    ("verticallayout", FcType::Bool),
    ("autohint", FcType::Bool),
    ("globaladvance", FcType::Bool),
    ("file", FcType::Str),
    ("index", FcType::Int),
    ("rasterizer", FcType::Str),
    ("outline", FcType::Bool),
    ("scalable", FcType::Bool),
    ("dpi", FcType::Double),
    ("rgba", FcType::Int),
    ("scale", FcType::Double),
    ("minspace", FcType::Bool),
    ("charwidth", FcType::Int),
    ("charheight", FcType::Int),
    ("matrix", FcType::Matrix),
    ("charset", FcType::CharSet),
    ("lang", FcType::LangSet),
    ("fontversion", FcType::Int),
    ("capability", FcType::Str),
    ("fontformat", FcType::Str),
    ("embolden", FcType::Bool),
    ("embeddedbitmap", FcType::Bool),
    ("decorative", FcType::Bool),
    ("lcdfilter", FcType::Int),
    ("namelang", FcType::Str),
    ("fontfeatures", FcType::Str),
    ("prgname", FcType::Str),
    ("hash", FcType::Str),
    ("postscriptname", FcType::Str),
    ("color", FcType::Bool),
    ("symbol", FcType::Bool),
    ("fontvariations", FcType::Str),
    ("variable", FcType::Bool),
    ("fonthashint", FcType::Bool),
    ("order", FcType::Int),
    ("desktop", FcType::Str),
    ("namedinstance", FcType::Bool),
    ("fontwrapper", FcType::Str),
];

/// `FcObjectValidType` (`[[source: fontconfig-fcname-2-15-0]]`): whether
/// `object` takes `v`. A number fits an
/// integer, a double or a range; a string fits a language set too.
fn takes(object: &str, v: &Value) -> bool {
    let Some(&(_, t)) = FC_OBJECTS.iter().find(|(n, _)| *n == object) else {
        return true;
    };
    matches!(
        (t, v),
        (
            FcType::Int | FcType::Double | FcType::Range,
            Value::Int(_) | Value::Double(_)
        ) | (FcType::Str | FcType::LangSet, Value::Str(_))
            | (FcType::Bool, Value::Bool(_))
            | (FcType::Range, Value::Other(Other::Range))
            | (FcType::Matrix, Value::Other(Other::Matrix))
            | (FcType::CharSet, Value::Other(Other::CharSet))
            | (FcType::LangSet, Value::Other(Other::LangSet))
    )
}

/// `FcParsePatelt`'s loop: the values are popped last first, and the first
/// that is `Void` or that the object does not take ends it, with what came
/// before it in the file left unread.
fn patelt_values(object: &str, values: Vec<Value>) -> Vec<Value> {
    values
        .into_iter()
        .rev()
        .take_while(|v| *v != Value::Other(Other::Void) && takes(object, v))
        .collect()
}

/// One `<patelt>`: its object and the values `FcParsePatelt` kept.
#[derive(Clone, Debug)]
struct Patelt {
    object: String,
    values: Vec<Value>,
}

/// One `<pattern>`: its elements, one an object.
#[derive(Clone, Debug, Default)]
pub struct Pattern {
    elements: Vec<Patelt>,
}

impl Pattern {
    /// `FcListPatternMatchAny`: every object the pattern names is on the
    /// face, and each of its values equals one of the face's
    /// (`FcListValueListMatchAny` under `FcOpListing`). `namelang` is
    /// skipped, as fontconfig skips it. An object baud does not read from
    /// a face fails the pattern, an open departure (the module's doc).
    fn matches(&self, f: &FaceView, variable: Has) -> bool {
        self.elements.iter().all(|e| {
            if e.object == "namelang" {
                return true;
            }
            let Some(have) = face_values(f, &e.object, variable) else {
                return false;
            };
            e.values.iter().all(|p| have.iter().any(|v| listing(v, p)))
        })
    }
}

/// The face's values for `object`, or `None` for an object baud does not
/// read.
fn face_values(f: &FaceView, object: &str, variable: Has) -> Option<Vec<Value>> {
    let strs = |v: &[String]| v.iter().cloned().map(Value::Str).collect();
    let flag = |h: Has| {
        vec![Value::Bool(match h {
            Has::Yes => Fc3::True,
            Has::No => Fc3::False,
        })]
    };
    Some(match object {
        "family" => strs(&f.families),
        "fullname" => strs(&f.full_names),
        "postscriptname" => f.postscript.iter().cloned().map(Value::Str).collect(),
        "file" => vec![Value::Str(f.file.clone())],
        "index" => vec![Value::Int(f.index.0)],
        "outline" => flag(f.outline),
        "color" => flag(f.color),
        "scalable" => flag(match (f.outline, f.color) {
            (Has::No, Has::No) => Has::No,
            _ => Has::Yes,
        }),
        "variable" => flag(variable),
        "fontformat" => f.format.iter().map(|s| Value::Str((*s).into())).collect(),
        _ => return None,
    })
}

/// `FcConfigCompareValue(font, FcOpListing | FcOpFlagIgnoreBlanks,
/// pattern)`: strings equal ignoring spaces and case, booleans equal or
/// the face's `DontCare`, numbers equal with an integer promoted to a
/// double, and values of two types never.
fn listing(font: &Value, pat: &Value) -> bool {
    match (font, pat) {
        (Value::Str(a), Value::Str(b)) => baud_domain::caseless_eq(
            a.chars().filter(|&c| c != ' '),
            b.chars().filter(|&c| c != ' '),
        ),
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Int(a), Value::Double(b)) | (Value::Double(b), Value::Int(a)) => {
            // An index fits a double exactly.
            #[allow(clippy::cast_precision_loss)]
            let a = *a as f64;
            a == *b
        }
        (Value::Double(a), Value::Double(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b || *a == Fc3::DontCare,
        _ => false,
    }
}

/// `FcNameBool` (`[[source: fontconfig-fcname-2-15-0]]`): by the first letter, `t` `y` `1` true, `f` `n` `0` false,
/// `d` `x` `2` don't care, and `on` `off` `or`; anything else is false, as
/// `FcConfigLexBool` returns after its warning.
fn name_bool(s: &str) -> Fc3 {
    let mut c = s.bytes().map(|b| b.to_ascii_lowercase());
    match (c.next(), c.next()) {
        (Some(b't' | b'y' | b'1'), _) | (Some(b'o'), Some(b'n')) => Fc3::True,
        (Some(b'd' | b'x' | b'2'), _) | (Some(b'o'), Some(b'r')) => Fc3::DontCare,
        _ => Fc3::False,
    }
}

/// `strtol(s, &end, 0)` that must consume all of `s`, as `FcParseInt`
/// asks: leading space, a sign, then `0x` hexadecimal, a leading `0`
/// octal, else decimal.
fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let (radix, digits) = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        (16, h)
    } else if t.len() > 1 && t.starts_with('0') {
        (8, &t[1..])
    } else {
        (10, t)
    };
    if digits.is_empty() {
        return None;
    }
    let v = i64::from_str_radix(digits, radix).ok()?;
    let v = if neg { -v } else { v };
    // `strtol` into an `int`: fontconfig keeps the low 32 bits of a value
    // past `int`'s range; baud drops such a value, a departure only an
    // index past 2^31 reaches.
    i32::try_from(v).ok().map(i64::from)
}

/// `a` and `b` joined by one `/`, as `FcParseInclude` and
/// `FcConfigXdgDataHome` join theirs, with the separator written whatever
/// `a` is.
fn join(a: &OsStr, b: &OsStr) -> OsString {
    let mut s = a.to_os_string();
    s.push("/");
    s.push(b);
    s
}

/// `a` and `b` as `FcStrBuildFilename` joins them: a `/` between them only
/// once something is written, so an empty `a` leaves `b` as it is.
fn build_filename(a: &OsStr, b: &OsStr) -> OsString {
    if a.is_empty() {
        b.to_os_string()
    } else {
        join(a, b)
    }
}

/// `name` without its first byte, a `~`.
fn after_tilde(name: &OsStr) -> &OsStr {
    use std::os::unix::ffi::OsStrExt;
    OsStr::from_bytes(&name.as_bytes()[1..])
}

fn starts_with_tilde(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    name.as_bytes().first() == Some(&b'~')
}

impl Load<'_> {
    /// `FcConfigGetFilename`: the file `name` names, if it can be opened
    /// for reading. A file that cannot is passed over for the next, as
    /// `FcConfigFileExists` passes over one that `access(R_OK)` refuses;
    /// baud opens the file to ask, which waits on a named FIFO that has
    /// no writer yet, where `access` does not.
    fn filename(&self, name: &OsStr) -> Option<PathBuf> {
        let exists = |p: PathBuf| std::fs::File::open(&p).is_ok().then_some(p);
        if Path::new(name).is_absolute() {
            return exists(name.into());
        }
        if starts_with_tilde(name) {
            let mut s = self.env.home.clone()?;
            s.push(after_tilde(name));
            return exists(s.into());
        }
        let mut search: Vec<PathBuf> = self
            .env
            .path
            .iter()
            .flat_map(|p| std::env::split_paths(p).collect::<Vec<_>>())
            .collect();
        search.push(self.env.config_dir.clone());
        search
            .into_iter()
            .find_map(|d| exists(join(d.as_os_str(), name).into()))
    }

    /// The root file: as [`Load::parse`], but a named pipe is read where it
    /// is, under the name it was given -- `FcConfigRealFilename` resolves no
    /// link to one for the root, so `FONTCONFIG_FILE=<(...)` reads the
    /// pipe, whose `/dev/fd` link names no file on Linux.
    fn parse_root(&mut self, path: &Path) {
        use std::os::unix::fs::FileTypeExt;
        let fifo = std::fs::metadata(path).is_ok_and(|m| m.file_type().is_fifo());
        if !fifo {
            self.parse(path);
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        self.read_bytes(&bytes, path);
    }

    /// `_FcConfigParse`: the file or directory at `path`, once.
    fn parse(&mut self, path: &Path) {
        let Ok(real) = path.canonicalize() else {
            return;
        };
        if !self.read.insert(real.clone()) {
            return;
        }
        if real.is_dir() {
            self.parse_dir(&real);
            return;
        }
        let Ok(bytes) = std::fs::read(&real) else {
            return;
        };
        self.read_bytes(&bytes, &real);
    }

    /// Every element of one file's bytes, as each ends.
    fn read_bytes(&mut self, bytes: &[u8], file: &Path) {
        for e in Reader::new(bytes).elements() {
            self.element(&e, file);
        }
    }

    /// `FcConfigParseAndLoadDir`: every `[0-9]*.conf` in `dir`, in byte
    /// order.
    fn parse_dir(&mut self, dir: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut names: Vec<OsString> = rd
            .flatten()
            .map(|e| e.file_name())
            .filter(|n| {
                let b = n.as_bytes();
                b.len() > 5 && b[0].is_ascii_digit() && b.ends_with(b".conf")
            })
            .collect();
        names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        for n in names {
            self.parse(&dir.join(n));
        }
    }

    /// What one ended element does, read in `file`.
    fn element(&mut self, e: &Element, file: &Path) {
        match e.name.as_str() {
            "dir" => self.add_dir(&e.text, e.attr("prefix"), file),
            // A remap needs its `as-path`; without one fontconfig says so and
            // ignores the element.
            "remap-dir" if e.attr("as-path").is_some() => {
                self.add_dir(&e.text, e.attr("prefix"), file);
            }
            "reset-dirs" => self.dirs.clear(),
            "include" => self.include(&e.text, e.attr("prefix")),
            _ if self.alias_part(e) => {}
            _ => self.selector_part(e),
        }
    }

    /// **A part of an `<alias>`, or the alias itself** (`FcParseFamily`,
    /// `FcParseFamilies`, `FcParseAlias`): a `<family>` inside a
    /// `<prefer>`, `<accept>` or `<default>` waits for it, and one directly
    /// inside the alias for the alias; each value inside a `<test>` waits
    /// for the test; each of the three lists, and each `<test>`, waits for
    /// the alias, which takes them as it ends ([`Aliases`]). Whether `e`
    /// was one.
    fn alias_part(&mut self, e: &Element) -> bool {
        let p = &mut self.pending;
        match (e.name.as_str(), e.parent.as_deref()) {
            ("string", Some("test")) => p.test_values.push(Literal::Str(e.text.clone())),
            (_, Some("test")) => p.test_values.push(Literal::Other),
            ("test", Some("alias")) => {
                let values = core::mem::take(&mut p.test_values);
                if let Ok(t) = Test::read(e, values) {
                    p.alias_tests.push(t);
                }
            }
            ("test", _) => p.test_values.clear(),
            ("family", Some("prefer" | "accept" | "default")) => p.families.push(e.text.clone()),
            ("family", Some("alias")) => p.alias_families.push(e.text.clone()),
            (which @ ("prefer" | "accept" | "default"), Some("alias")) => {
                let list = core::mem::take(&mut p.families);
                let slot = match which {
                    "prefer" => &mut p.prefer,
                    "accept" => &mut p.accept,
                    _ => &mut p.default,
                };
                slot.get_or_insert(list);
            }
            ("alias", _) => {
                let families = core::mem::take(&mut p.alias_families);
                let (prefer, accept, default) =
                    (p.prefer.take(), p.accept.take(), p.default.take());
                let read = core::mem::take(&mut p.alias_tests);
                p.families.clear();
                let binding = match e.attr("binding") {
                    None | Some("weak") => Some(Binding::Weak),
                    Some("strong") => Some(Binding::Strong),
                    Some("same") => Some(Binding::Same),
                    Some(_) => None,
                };
                let edits = prefer.is_some() || accept.is_some() || default.is_some();
                let (Some(binding), false, true) = (binding, families.is_empty(), edits) else {
                    return true;
                };
                let mut tests = Vec::new();
                for t in read {
                    match t {
                        TestRead::Evaluated(t) => tests.push(t),
                        TestRead::Unevaluated => {
                            self.aliases.unapplied.push(Unapplied);
                            return true;
                        }
                    }
                }
                self.aliases.rules.push(Alias {
                    tests,
                    families,
                    binding,
                    prefer: prefer.unwrap_or_default(),
                    accept: accept.unwrap_or_default(),
                    default: default.unwrap_or_default(),
                });
            }
            _ => return false,
        }
        true
    }

    /// **A part of a `<selectfont>` rule** (`[[source: fontconfig-fcxml-2-15-0]]`
    /// `FcParseAcceptRejectFont`, `FcParsePattern`, `FcParsePatelt`,
    /// `FcPopValue`): each value inside a `<patelt>`, each `<patelt>` inside
    /// a `<pattern>`, and each `<glob>` and `<pattern>` inside an
    /// `<acceptfont>` or `<rejectfont>` waits for the element it is inside,
    /// which takes it as it ends.
    fn selector_part(&mut self, e: &Element) {
        let parent = e.parent.as_deref();
        match (e.name.as_str(), parent) {
            ("string", Some("patelt")) => self.pending.values.push(Value::Str(e.text.clone())),
            ("int", Some("patelt")) => {
                if let Some(v) = parse_int(&e.text) {
                    self.pending.values.push(Value::Int(v));
                }
            }
            ("double", Some("patelt")) => {
                let t = e.text.trim_start_matches([' ', '\t', '\n', '\r']);
                if let Ok(v) = t.parse::<f64>() {
                    self.pending.values.push(Value::Double(v));
                }
            }
            ("bool", Some("patelt")) => self.pending.values.push(Value::Bool(name_bool(&e.text))),
            ("charset", Some("patelt")) => self.pending.values.push(Value::Other(Other::CharSet)),
            ("langset", Some("patelt")) => self.pending.values.push(Value::Other(Other::LangSet)),
            ("range", Some("patelt")) => self.pending.values.push(Value::Other(Other::Range)),
            ("matrix", Some("patelt")) => self.pending.values.push(Value::Other(Other::Matrix)),
            // A `<const>` baud does not resolve, and the expressions.
            (_, Some("patelt")) => self.pending.values.push(Value::Other(Other::Void)),
            ("patelt", _) => {
                let values = core::mem::take(&mut self.pending.values);
                if let (Some(object), Some("pattern")) = (e.attr("name"), parent) {
                    let values = patelt_values(object, values);
                    if !values.is_empty() {
                        self.pending.elements.push(Patelt {
                            object: object.to_owned(),
                            values,
                        });
                    }
                }
            }
            ("pattern", _) => {
                let elements = core::mem::take(&mut self.pending.elements);
                if matches!(parent, Some("acceptfont" | "rejectfont")) {
                    self.pending
                        .selectors
                        .push(Selector::Pattern(Pattern { elements }));
                }
            }
            ("glob", Some("acceptfont" | "rejectfont")) => {
                // `FcStrCopyFilename`: `~` is `HOME`, and the glob is made
                // absolute and canonical; with no `HOME` it is not added.
                if let Some(g) = self.canonical(OsStr::new(&e.text)) {
                    use std::os::unix::ffi::OsStrExt;
                    let g = g.as_os_str().as_bytes().to_vec();
                    self.pending.selectors.push(Selector::Glob(g));
                }
            }
            (which @ ("acceptfont" | "rejectfont"), _) => {
                let accept = which == "acceptfont";
                for s in core::mem::take(&mut self.pending.selectors) {
                    match (s, accept) {
                        (Selector::Glob(g), true) => self.select.accept_globs.push(g),
                        (Selector::Glob(g), false) => self.select.reject_globs.push(g),
                        (Selector::Pattern(p), true) => self.select.accept.push(p),
                        (Selector::Pattern(p), false) => self.select.reject.push(p),
                    }
                }
            }
            _ => {}
        }
    }

    /// `FcParseInclude`.
    fn include(&mut self, text: &str, prefix: Option<&str>) {
        let name: OsString = match prefix {
            Some("xdg") => join(
                &self.xdg_home(&self.env.xdg_config_home, ".config"),
                text.as_ref(),
            ),
            _ => text.into(),
        };
        if let Some(p) = self.filename(&name) {
            self.parse(&p);
        }
    }

    /// `$XDG_..._HOME` when set, empty or not; else `HOME` (or nothing) and
    /// `/` + `fallback`.
    fn xdg_home(&self, var: &Option<OsString>, fallback: &str) -> OsString {
        var.clone().unwrap_or_else(|| {
            join(
                self.env.home.as_deref().unwrap_or(OsStr::new("")),
                fallback.as_ref(),
            )
        })
    }

    /// `FcParseDir`: the directories `text` names under `prefix`.
    fn add_dir(&mut self, text: &str, prefix: Option<&str>, file: &Path) {
        if text.is_empty() {
            return;
        }
        let path = OsStr::new(text);
        let names: Vec<OsString> = match prefix {
            Some("xdg") => {
                let mut v = vec![build_filename(
                    &self.xdg_home(&self.env.xdg_data_home, ".local/share"),
                    path,
                )];
                v.extend(self.xdg_data_dirs().iter().map(|d| build_filename(d, path)));
                v
            }
            Some("relative") => match file.parent() {
                Some(dir) => vec![build_filename(dir.as_os_str(), path)],
                None => Vec::new(),
            },
            _ => vec![path.to_os_string()],
        };
        for n in names {
            if n.is_empty() || (starts_with_tilde(&n) && self.env.home.is_none()) {
                continue;
            }
            let Some(p) = self.canonical(&n) else {
                continue;
            };
            self.dirs.retain(|d| *d != p);
            self.dirs.push(p);
        }
    }

    /// `FcConfigXdgDataDirs`: `$XDG_DATA_DIRS`'s entries, trailing slashes
    /// trimmed, or `/usr/local/share` and `/usr/share` when it is unset.
    fn xdg_data_dirs(&self) -> Vec<OsString> {
        use std::os::unix::ffi::OsStrExt;
        let Some(v) = &self.env.xdg_data_dirs else {
            return vec!["/usr/local/share".into(), "/usr/share".into()];
        };
        v.as_bytes()
            .split(|&b| b == b':')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut s = s;
                while s.len() > 1 && s.ends_with(b"/") {
                    s = &s[..s.len() - 1];
                }
                OsStr::from_bytes(s).to_os_string()
            })
            .collect()
    }

    /// `FcStrCopyFilename`: a leading `~` made `HOME`, the name made
    /// absolute against the working directory, then `//`, `/.` and `/..`
    /// folded and a final `/` trimmed.
    fn canonical(&self, name: &OsStr) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let full: OsString = if starts_with_tilde(name) {
            let mut s = self.env.home.clone()?;
            s.push(after_tilde(name));
            s
        } else {
            name.to_os_string()
        };
        let full = if full.as_bytes().first() == Some(&b'/') {
            full
        } else {
            join(self.env.cwd.as_deref()?.as_os_str(), &full)
        };
        let mut parts: Vec<&[u8]> = Vec::new();
        for seg in full.as_bytes().split(|&b| b == b'/') {
            match seg {
                b"" | b"." => {}
                b".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        let mut out = Vec::new();
        for p in parts {
            out.push(b'/');
            out.extend_from_slice(p);
        }
        if out.is_empty() {
            out.push(b'/');
        }
        Some(PathBuf::from(OsStr::from_bytes(&out)))
    }
}

/// One element the reader has seen end: its name, its attributes, the
/// character data directly inside it, and the element it is inside.
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    parent: Option<String>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A small XML reader over one document.
struct Reader<'b> {
    /// What is left to read.
    rest: &'b [u8],
}

impl<'b> Reader<'b> {
    fn new(src: &'b [u8]) -> Reader<'b> {
        Reader { rest: src }
    }

    fn rest(&self) -> &'b [u8] {
        self.rest
    }

    /// Past the next `end`, or `None` when the document has none.
    fn skip_past(&mut self, end: &[u8]) -> Option<()> {
        let i = self.rest().windows(end.len()).position(|w| w == end)?;
        self.rest = &self.rest[i + end.len()..];
        Some(())
    }

    /// Every element, in the order each ends; the elements read before
    /// anything malformed.
    fn elements(mut self) -> Vec<Element> {
        let mut done = Vec::new();
        let mut open: Vec<Element> = Vec::new();
        while !self.rest.is_empty() {
            let rest = self.rest();
            if rest.starts_with(b"<!--") {
                if self.skip_past(b"-->").is_none() {
                    break;
                }
            } else if rest.starts_with(b"<![CDATA[") {
                let body = &rest[9..];
                self.rest = body;
                if self.skip_past(b"]]>").is_none() {
                    break;
                }
                let inside = &body[..body.len() - self.rest.len() - 3];
                if let Some(e) = open.last_mut() {
                    e.text.push_str(&String::from_utf8_lossy(inside));
                }
            } else if rest.starts_with(b"<?") {
                if self.skip_past(b"?>").is_none() {
                    break;
                }
            } else if rest.starts_with(b"<!") {
                if self.doctype().is_none() {
                    break;
                }
            } else if rest.starts_with(b"</") {
                self.rest = &self.rest[2..];
                let Some(name) = self.name() else { break };
                self.space();
                if self.rest().first() != Some(&b'>') {
                    break;
                }
                self.rest = &self.rest[1..];
                match open.pop() {
                    Some(mut e) if e.name == name => {
                        e.parent = open.last().map(|p| p.name.clone());
                        done.push(e);
                    }
                    _ => break,
                }
            } else if rest.starts_with(b"<") {
                self.rest = &self.rest[1..];
                let Some((e, empty)) = self.start() else {
                    break;
                };
                if empty {
                    let mut e = e;
                    e.parent = open.last().map(|p| p.name.clone());
                    done.push(e);
                } else {
                    open.push(e);
                }
            } else {
                let len = rest.iter().position(|&b| b == b'<').unwrap_or(rest.len());
                self.rest = &rest[len..];
                let Some(text) = unescape(&rest[..len]) else {
                    break;
                };
                if let Some(e) = open.last_mut() {
                    e.text.push_str(&text);
                }
            }
        }
        done
    }

    /// A `<!DOCTYPE ...>`, with any internal subset in brackets.
    fn doctype(&mut self) -> Option<()> {
        let mut depth = 0usize;
        while let Some(&b) = self.rest().first() {
            self.rest = &self.rest[1..];
            match b {
                b'[' => depth += 1,
                b']' => depth = depth.checked_sub(1)?,
                b'>' if depth == 0 => return Some(()),
                _ => {}
            }
        }
        None
    }

    fn space(&mut self) {
        while self.rest().first().is_some_and(u8::is_ascii_whitespace) {
            self.rest = &self.rest[1..];
        }
    }

    /// An element or attribute name.
    fn name(&mut self) -> Option<String> {
        let rest = self.rest();
        let len = rest
            .iter()
            .position(|&b| b.is_ascii_whitespace() || b"/>=<\"'".contains(&b))
            .unwrap_or(rest.len());
        if len == 0 {
            return None;
        }
        self.rest = &self.rest[len..];
        Some(String::from_utf8_lossy(&rest[..len]).into_owned())
    }

    /// A start tag after its `<`: the element, and whether it is empty.
    fn start(&mut self) -> Option<(Element, bool)> {
        let name = self.name()?;
        let mut attrs = Vec::new();
        loop {
            self.space();
            match self.rest().first()? {
                b'>' => {
                    self.rest = &self.rest[1..];
                    let e = Element {
                        name,
                        attrs,
                        text: String::new(),
                        parent: None,
                    };
                    return Some((e, false));
                }
                b'/' => {
                    if self.rest().get(1) != Some(&b'>') {
                        return None;
                    }
                    self.rest = &self.rest[2..];
                    let e = Element {
                        name,
                        attrs,
                        text: String::new(),
                        parent: None,
                    };
                    return Some((e, true));
                }
                _ => {
                    let key = self.name()?;
                    self.space();
                    if self.rest().first() != Some(&b'=') {
                        return None;
                    }
                    self.rest = &self.rest[1..];
                    self.space();
                    let quote = *self.rest().first()?;
                    if quote != b'"' && quote != b'\'' {
                        return None;
                    }
                    self.rest = &self.rest[1..];
                    let len = self.rest().iter().position(|&b| b == quote)?;
                    let value = unescape(&self.rest[..len])?;
                    self.rest = &self.rest[len + 1..];
                    attrs.push((key, value));
                }
            }
        }
    }
}

/// Character data with its references read: the five predefined entities
/// and `&#N;` / `&#xN;`; `None` for any other reference.
fn unescape(b: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(b);
    let mut out = String::with_capacity(s.len());
    let mut rest: &str = &s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest.find(';')?;
        let r = &rest[..end];
        let c = match r {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = match r.strip_prefix("#x") {
                    Some(h) => u32::from_str_radix(h, 16).ok()?,
                    None => r.strip_prefix('#')?.parse().ok()?,
                };
                char::from_u32(n)?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory of this test's own, canonical (macOS puts the
    /// temporary directory behind a symbolic link).
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("baud-b145-fc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn env(root: &Path) -> Env {
        Env {
            home: Some(root.join("home").into()),
            xdg_data_home: None,
            xdg_data_dirs: None,
            xdg_config_home: None,
            file: None,
            path: None,
            config_dir: root.join("etc/fonts"),
            cwd: Some(root.join("cwd")),
            locale: Locale::default(),
        }
    }

    fn shown(root: &Path, dirs: &[PathBuf]) -> Vec<String> {
        let r = root.display().to_string();
        dirs.iter()
            .map(|d| d.display().to_string().replace(&r, "R"))
            .collect()
    }

    /// The system file as fontconfig 2.15.0's `fonts.conf.in` lays it out,
    /// and its `conf.d/50-user.conf` as that release ships it.
    fn system(root: &Path) {
        write(
            &root.join("etc/fonts/fonts.conf"),
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
<!-- Font directory list <dir>/commented/out</dir> -->
	<dir>/usr/share/fonts</dir>
	<dir prefix="xdg">fonts</dir>
	<!-- the following element will be removed in the future -->
	<dir>~/.fonts</dir>
	<include ignore_missing="yes">conf.d</include>
</fontconfig>
"#,
        );
        write(
            &root.join("etc/fonts/conf.d/50-user.conf"),
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <description>Load per-user customization files</description>
	<include ignore_missing="yes" prefix="xdg">fontconfig/conf.d</include>
	<include ignore_missing="yes" prefix="xdg">fontconfig/fonts.conf</include>
	<include ignore_missing="yes" deprecated="yes">~/.fonts.conf.d</include>
	<include ignore_missing="yes" deprecated="yes">~/.fonts.conf</include>
</fontconfig>
"#,
        );
    }

    /// **The system file, its `conf.d` in order, the user's file through
    /// `50-user.conf`, and every prefix**: `xdg` under `$XDG_DATA_HOME`'s
    /// default and both default `$XDG_DATA_DIRS` entries, `~` under `HOME`,
    /// `relative` beside the including file, none relative to the working
    /// directory; a directory added again moves to the end; a `conf.d` file
    /// whose name does not begin with a digit or end in `.conf` is not
    /// read; a file included twice is read once; entities and CDATA are
    /// character data, and a commented-out element is no element.
    #[test]
    fn the_configuration_names_the_directories() {
        let root = scratch("all");
        system(&root);
        write(
            &root.join("etc/fonts/conf.d/10-relative.conf"),
            r#"<fontconfig><dir prefix="relative">rel</dir><dir>plain/./x/../y/</dir></fontconfig>"#,
        );
        write(
            &root.join("etc/fonts/conf.d/60-again.conf"),
            r#"<fontconfig>
  <dir>/usr/share/fonts</dir>
  <remap-dir as-path="/fonts">/srv/fonts</remap-dir>
  <remap-dir>/no/as-path</remap-dir>
  <include>conf.d</include>
</fontconfig>"#,
        );
        write(
            &root.join("etc/fonts/conf.d/x.conf"),
            "<fontconfig><dir>/not-a-digit</dir></fontconfig>",
        );
        write(
            &root.join("etc/fonts/conf.d/7-skip.conf.bak"),
            "<fontconfig><dir>/bak</dir></fontconfig>",
        );
        write(
            &root.join("home/.config/fontconfig/fonts.conf"),
            "<fontconfig><dir>~/Fonts &amp; more</dir><dir><![CDATA[/cdata<dir>]]></dir></fontconfig>",
        );
        let got = dirs(&env(&root)).expect("a configuration");
        assert_eq!(
            shown(&root, &got),
            [
                "R/home/.local/share/fonts",
                "/usr/local/share/fonts",
                "R/home/.fonts",
                "R/etc/fonts/conf.d/rel",
                "R/cwd/plain/y",
                "R/home/Fonts & more",
                "/cdata<dir>",
                "/usr/share/fonts",
                "/srv/fonts",
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// **`<reset-dirs/>` leaves only what follows it**: a user's file that
    /// resets keeps the system's directories out.
    #[test]
    fn reset_dirs_keeps_only_what_follows() {
        let root = scratch("reset");
        system(&root);
        write(
            &root.join("home/.config/fontconfig/fonts.conf"),
            "<fontconfig><reset-dirs /><dir>/mine</dir></fontconfig>",
        );
        assert_eq!(dirs(&env(&root)).unwrap(), [PathBuf::from("/mine")]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **The variables, as fontconfig reads them**: an `XDG_DATA_HOME` or
    /// `XDG_CONFIG_HOME` that is set counts even when empty, the data home
    /// as no prefix at all and the configuration home as `/`;
    /// `XDG_DATA_DIRS`' entries lose their trailing slashes; with no
    /// `HOME`, a `~` directory is skipped and the data home is
    /// `/.local/share`.
    #[test]
    fn the_variables_as_fontconfig_reads_them() {
        let root = scratch("vars");
        system(&root);
        let mut e = env(&root);
        e.xdg_data_home = Some(root.join("data").into());
        e.xdg_data_dirs = Some(format!("/a/:{}//:", root.join("b").display()).into());
        e.xdg_config_home = Some(root.join("cfg").into());
        write(
            &root.join("cfg/fontconfig/fonts.conf"),
            "<fontconfig><dir>/from-xdg-config</dir></fontconfig>",
        );
        assert_eq!(
            shown(&root, &dirs(&e).unwrap()),
            [
                "/usr/share/fonts",
                "R/data/fonts",
                "/a/fonts",
                "R/b/fonts",
                "R/home/.fonts",
                "/from-xdg-config",
            ]
        );
        e.xdg_data_home = Some("".into());
        assert_eq!(
            shown(&root, &dirs(&e).unwrap())[1],
            "R/cwd/fonts",
            "set and empty"
        );
        write(
            &root.join("etc/fonts/fontconfig/fonts.conf"),
            "<fontconfig><dir>/beside-the-system-file</dir></fontconfig>",
        );
        e.xdg_config_home = Some("".into());
        assert!(
            !shown(&root, &dirs(&e).unwrap()).contains(&"/beside-the-system-file".to_string()),
            "set and empty: the include is /fontconfig/fonts.conf"
        );
        let mut e = env(&root);
        e.home = None;
        assert_eq!(
            shown(&root, &dirs(&e).unwrap()),
            [
                "/.local/share/fonts",
                "/usr/local/share/fonts",
                "/usr/share/fonts"
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// **Which file is read**: `FONTCONFIG_FILE` when it names one, a
    /// name looked for along `FONTCONFIG_PATH` before the configuration
    /// directory, and no configuration at all when none is found.
    #[test]
    fn the_file_fontconfig_reads() {
        let root = scratch("file");
        system(&root);
        write(
            &root.join("other.conf"),
            "<fontconfig><dir>/other</dir></fontconfig>",
        );
        write(
            &root.join("path/fonts.conf"),
            "<fontconfig><dir>/on-the-path</dir></fontconfig>",
        );
        let mut e = env(&root);
        e.file = Some(root.join("other.conf").into());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/other")]);
        let mut e = env(&root);
        e.path = Some(root.join("path").into());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/on-the-path")]);
        let mut e = env(&root);
        e.config_dir = root.join("nowhere");
        assert_eq!(dirs(&e), None);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **What comes before a malformed part is kept**: an end tag that
    /// closes the wrong element stops the reading, and an unknown entity
    /// does too. fontconfig reads an `ignore_missing` include this way; a
    /// root file like this one it drops whole for its fallback
    /// configuration, an open departure (the module's doc).
    #[test]
    fn a_malformed_file_keeps_what_came_before() {
        let root = scratch("bad");
        let mut e = env(&root);
        e.file = Some(root.join("bad.conf").into());
        write(
            &root.join("bad.conf"),
            "<fontconfig><dir>/a</dir><dir>/b</fontconfig><dir>/c</dir>",
        );
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/a")]);
        write(
            &root.join("bad.conf"),
            "<fontconfig><dir>/a</dir><dir>/b&nbsp;</dir><dir>/c</dir></fontconfig>",
        );
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/a")]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// The `<selectfont>` rules of a configuration of `body` alone.
    fn select_of(root: &Path, body: &str) -> Select {
        let mut e = env(root);
        e.file = Some(root.join("sel.conf").into());
        write(
            &root.join("sel.conf"),
            &format!("<fontconfig>{body}</fontconfig>"),
        );
        load(&e).unwrap().select
    }

    /// **Globs as fontconfig 2.15.0's code reads them** (`FcStrGlobMatch`,
    /// `FcConfigGlobAdd`, `FcConfigAcceptFilename`): `*` crosses `/` and `?`
    /// is one byte; an `<acceptfont>` glob wins over a `<rejectfont>` one
    /// whatever their order, and `<reset-dirs/>` leaves both; `~` is `HOME`,
    /// a relative glob is made absolute against the working directory
    /// (so the documentation's `*.pcf.gz` matches only there), and a glob
    /// inside anything but `<acceptfont>` or `<rejectfont>` is no rule.
    #[test]
    fn selectfont_globs_read_as_fontconfig_reads_them() {
        let root = scratch("globs");
        let sel = select_of(
            &root,
            "<selectfont><acceptfont><glob>/x/fonts/ugly-but-kept/*</glob></acceptfont></selectfont>\
             <reset-dirs/>\
             <selectfont><rejectfont>\
               <glob>/x/fonts/ugly*</glob><glob>*.pcf.gz</glob><glob>~/hidden/*</glob>\
               <glob>/x/one?.ttf</glob><glob>/y/*/deep.ttf</glob>\
             </rejectfont></selectfont>\
             <match><glob>/x/fonts/*</glob></match>",
        );
        let kept = |p: &str| sel.path(Path::new(p));
        assert_eq!(kept("/x/fonts/uglyfont.ttf"), Kept::No);
        assert_eq!(kept("/x/fonts/uglyfont/sub/deep.ttf"), Kept::No);
        assert_eq!(kept("/y/a/b/deep.ttf"), Kept::No, "* crosses /");
        assert_eq!(kept("/y/a/b/deep.otf"), Kept::Yes);
        assert_eq!(
            kept("/x/fonts/ugly-but-kept/a.ttf"),
            Kept::Yes,
            "accept wins"
        );
        assert_eq!(
            kept("/x/fonts/fine.ttf"),
            Kept::Yes,
            "a glob in <match> is no rule"
        );
        assert_eq!(kept("/x/one1.ttf"), Kept::No);
        assert_eq!(kept("/x/one12.ttf"), Kept::Yes, "? is one byte");
        let cwd = root.join("cwd");
        assert_eq!(sel.path(&cwd.join("a.pcf.gz")), Kept::No);
        assert_eq!(kept("/usr/share/fonts/a.pcf.gz"), Kept::Yes);
        assert_eq!(sel.path(&root.join("home/hidden/x.ttf")), Kept::No);
        assert!(!sel.tests_faces());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A face view with every property at a plain TrueType face's.
    fn view() -> FaceView {
        FaceView {
            families: vec!["Noto Color Emoji".into(), "Noto Emoji Family".into()],
            full_names: vec!["Noto Color Emoji Regular".into()],
            postscript: Some("NotoColorEmoji".into()),
            file: "/f/NotoColorEmoji.ttf".into(),
            index: FcIndex(0),
            outline: Has::Yes,
            color: Has::No,
            variable: Has::No,
            format: Some("TrueType"),
        }
    }

    /// **Patterns as `FcListPatternMatchAny` reads them**: every object
    /// named must be on the face and every value of it one of the face's;
    /// strings ignore spaces and case, a boolean reads `no` and `off` as
    /// false and an integer `0x` as hexadecimal; `scalable` is outline or
    /// colour; `namelang` is skipped; an object baud does not read fails
    /// the pattern; and a value the object does not take ends the
    /// `<patelt>`'s reading, so a pattern left with no elements matches
    /// every face, as fontconfig's does.
    #[test]
    fn selectfont_patterns_read_as_fontconfig_reads_them() {
        let root = scratch("patterns");
        let rejects = |body: &str, v: &FaceView| {
            let sel = select_of(
                &root,
                &format!("<selectfont><rejectfont>{body}</rejectfont></selectfont>"),
            );
            assert!(sel.tests_faces(), "{body}");
            sel.face(v) == Kept::No
        };
        let pat = |elts: &str| format!("<pattern>{elts}</pattern>");
        let mut bitmap = view();
        bitmap.outline = Has::No;
        let no_bitmaps = pat(r#"<patelt name="outline"><bool>false</bool></patelt>"#);
        assert!(rejects(&no_bitmaps, &bitmap));
        assert!(!rejects(&no_bitmaps, &view()));
        let off = pat(r#"<patelt name="outline"><bool>off</bool></patelt>"#);
        assert!(rejects(&off, &bitmap));
        let not_scalable = pat(r#"<patelt name="scalable"><bool>no</bool></patelt>"#);
        let mut colour_bitmap = bitmap.clone();
        colour_bitmap.color = Has::Yes;
        assert!(rejects(&not_scalable, &bitmap));
        assert!(
            !rejects(&not_scalable, &colour_bitmap),
            "colour makes it scalable"
        );
        let family = pat(r#"<patelt name="family"><string>notocolor EMOJI</string></patelt>"#);
        assert!(rejects(&family, &view()));
        let both = pat(
            r#"<patelt name="family"><string>Noto Color Emoji</string><string>Noto Emoji Family</string></patelt>"#,
        );
        assert!(rejects(&both, &view()), "each value is one of the face's");
        let one_missing = pat(
            r#"<patelt name="family"><string>Noto Color Emoji</string><string>Other</string></patelt>"#,
        );
        assert!(!rejects(&one_missing, &view()));
        let index = pat(r#"<patelt name="index"><int>0x0</int></patelt>"#);
        assert!(rejects(&index, &view()));
        let format = pat(r#"<patelt name="fontformat"><string>Type 1</string></patelt>"#);
        assert!(!rejects(&format, &view()));
        let lang = pat(r#"<patelt name="lang"><string>he</string></patelt>"#);
        assert!(
            !rejects(&lang, &view()),
            "an object baud does not read fails"
        );
        let skipped = pat(
            r#"<patelt name="namelang"><string>en</string></patelt><patelt name="outline"><bool>false</bool></patelt>"#,
        );
        assert!(rejects(&skipped, &bitmap));
        let wrong_type = pat(r#"<patelt name="outline"><string>false</string></patelt>"#);
        assert!(
            rejects(&wrong_type, &view()),
            "an empty pattern matches every face"
        );
        // An `<acceptfont>` pattern keeps what a `<rejectfont>` one drops.
        let sel = select_of(
            &root,
            &format!(
                "<selectfont><rejectfont>{no_bitmaps}</rejectfont><acceptfont>{}</acceptfont></selectfont>",
                pat(r#"<patelt name="color"><bool>true</bool></patelt>"#)
            ),
        );
        assert_eq!(sel.face(&bitmap), Kept::No);
        assert_eq!(sel.face(&colour_bitmap), Kept::Yes);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **A face's view is FreeType's and fontconfig's**: a CBDT colour-emoji
    /// face has no outline (`CBDT` overrides `glyf`) but is colour and so
    /// scalable; a TrueType face has outlines and no colour; a CFF one is
    /// "CFF"; a face with `fvar` is variable; the names are its records.
    #[test]
    fn a_face_is_viewed_as_fontconfig_views_it() {
        let read = |name: &str| {
            std::fs::read(format!(
                "{}/../baud-font/tests/fonts/{name}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap()
        };
        let at = |data: &[u8]| {
            let d = baud_font::Described::of(data, baud_font::FaceIndex::FIRST).unwrap();
            FaceView::of(&d, "/f", FcIndex(0))
        };
        let emoji = read("NotoColorEmoji-subset.ttf");
        let v = at(&emoji);
        assert_eq!(
            (v.outline, v.color, v.format),
            (Has::No, Has::Yes, Some("TrueType"))
        );
        let jbm = read("JetBrainsMono-Regular.ttf");
        let v = at(&jbm);
        assert_eq!(
            (v.outline, v.color, v.variable),
            (Has::Yes, Has::No, Has::No)
        );
        assert!(
            v.families.iter().any(|f| f == "JetBrains Mono"),
            "{:?}",
            v.families
        );
        assert_eq!(v.postscript.as_deref(), Some("JetBrainsMono-Regular"));
        let cff = read("SourceCodeVF-Upright.otf");
        let v = at(&cff);
        assert_eq!(
            (v.format, v.variable, v.outline),
            (Some("CFF"), Has::Yes, Has::Yes)
        );
    }

    /// An sfnt of `version` holding `tables`, each its tag and bytes: a
    /// directory for [`FaceView::of`], which reads nothing else.
    fn sfnt(version: &[u8; 4], tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let n = tables.len();
        let mut out = version.to_vec();
        out.extend_from_slice(&u16::try_from(n).unwrap().to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut at = 12 + 16 * n;
        for (tag, body) in tables {
            out.extend_from_slice(&tag[..]);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
            at += body.len();
        }
        for (_, body) in tables {
            out.extend_from_slice(body);
        }
        out
    }

    /// **FreeType's face flags, rule by rule** (`sfnt_load_face()`,
    /// `tt_face_load_sbit()`): `CBDT` takes the outline from a face with
    /// `glyf`; `sbix` beside outlines makes it SBIX, not scalable; a face
    /// with neither outlines nor a strike is scalable; an `EBLC` strike
    /// with its data and no outlines is a bitmap face, without its data
    /// it has no strike; `sbix` is colour only when no `CBLC`, `EBLC` or
    /// `bloc` comes first; `COLR` and `SVG ` are colour; `'OTTO'` is CFF
    /// and `'typ1'` no format. FreeType clips a strike count to the
    /// table's length, (length - 8) / 48 for `CBLC`, `EBLC` and `bloc` and
    /// (length - 8) / 4 for `sbix`, and drops a strike whose metrics fail
    /// to load or whose ppem is zero. baud reads `numSizes` and
    /// `numStrikes` unclipped, so a truncated table is read as having
    /// strikes FreeType would not see. The fixtures therefore hold whole
    /// strikes: a 56-byte `CBLC`/`EBLC` with one 48-byte `BitmapSize`
    /// record at 20 ppem, and a 16-byte `sbix` whose one strike offset
    /// points at a 20 ppem strike header, so the strike count FreeType
    /// reads from each table is the one baud reads.
    #[test]
    fn freetype_flags_rule_by_rule() {
        let mut strikes1 = [0u8; 56];
        strikes1[..8].copy_from_slice(&[0, 2, 0, 0, 0, 0, 0, 1]);
        // BitmapSize at 8: ppemX at +44, ppemY at +45.
        strikes1[8 + 44] = 20;
        strikes1[8 + 45] = 20;
        // version 1, flags 1, one strike at offset 12: ppem 20, 72 dpi.
        let sbix1 = [0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 12, 0, 20, 0, 72];
        let view = |v: &[u8; 4], t: &[(&[u8; 4], &[u8])]| {
            let data = sfnt(v, t);
            let d = baud_font::Described::of(&data, baud_font::FaceIndex::FIRST).unwrap();
            let f = FaceView::of(&d, "/f", FcIndex(0));
            (f.outline, f.color, f.format)
        };
        let tt = b"\0\x01\0\0";
        let (y, n) = (Has::Yes, Has::No);
        let ttf = Some("TrueType");
        assert_eq!(view(tt, &[(b"glyf", &[0; 4])]), (y, n, ttf));
        assert_eq!(
            view(tt, &[(b"CBDT", &[0; 4]), (b"glyf", &[0; 4])]),
            (y, n, ttf),
            "no strike"
        );
        assert_eq!(
            view(
                tt,
                &[(b"CBDT", &[0; 4]), (b"CBLC", &strikes1), (b"glyf", &[0; 4])]
            ),
            (n, y, ttf),
            "CBDT takes the outline"
        );
        assert_eq!(
            view(tt, &[(b"glyf", &[0; 4]), (b"sbix", &sbix1)]),
            (n, y, ttf)
        );
        assert_eq!(
            view(tt, &[(b"head", &[0; 4])]),
            (y, n, ttf),
            "no outline, no strike"
        );
        assert_eq!(
            view(tt, &[(b"EBDT", &[0; 4]), (b"EBLC", &strikes1)]),
            (n, n, ttf)
        );
        assert_eq!(
            view(tt, &[(b"EBLC", &strikes1)]),
            (y, n, ttf),
            "no data, no strike"
        );
        assert_eq!(
            view(
                tt,
                &[
                    (b"EBDT", &[0; 4]),
                    (b"EBLC", &strikes1),
                    (b"glyf", &[0; 4]),
                    (b"sbix", &sbix1)
                ]
            ),
            (n, n, ttf),
            "EBLC comes before sbix"
        );
        assert_eq!(
            view(tt, &[(b"COLR", &[0; 4]), (b"glyf", &[0; 4])]),
            (y, y, ttf)
        );
        assert_eq!(
            view(tt, &[(b"SVG ", &[0; 4]), (b"glyf", &[0; 4])]),
            (y, y, ttf)
        );
        assert_eq!(view(b"OTTO", &[(b"CFF ", &[0; 4])]), (y, n, Some("CFF")));
        assert_eq!(view(b"typ1", &[(b"head", &[0; 4])]).2, None);
    }

    /// **`FcConfigFileExists` asks for a file it can read**: an unreadable
    /// `fonts.conf` on `FONTCONFIG_PATH` is passed over for the next.
    #[test]
    fn an_unreadable_file_is_passed_over() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("unreadable");
        write(
            &root.join("a/fonts.conf"),
            "<fontconfig><dir>/from-a</dir></fontconfig>",
        );
        write(
            &root.join("b/fonts.conf"),
            "<fontconfig><dir>/from-b</dir></fontconfig>",
        );
        let locked = root.join("a/fonts.conf");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Positive control: the file is unreadable to this process.
        assert!(std::fs::read(&locked).is_err());
        let mut e = env(&root);
        e.path = Some(std::env::join_paths([root.join("a"), root.join("b")]).unwrap());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/from-b")]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    /// **`FONTCONFIG_FILE` naming a pipe is read** (`FcConfigRealFilename`
    /// keeps a named pipe's name for the root): `/dev/fd/N` of a pipe, as a
    /// shell's `<(...)` passes it. On Linux that link names no file a path
    /// can be made canonical through; on macOS it does, so this passes
    /// there either way.
    #[test]
    fn a_pipe_as_the_file_is_read() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        let root = scratch("pipe");
        let (reader, mut writer) = std::io::pipe().unwrap();
        let body = b"<fontconfig><dir>/through-a-pipe</dir></fontconfig>";
        writer.write_all(body).unwrap();
        drop(writer);
        let mut e = env(&root);
        e.file = Some(format!("/dev/fd/{}", reader.as_raw_fd()).into());
        assert_eq!(dirs(&e).unwrap(), [PathBuf::from("/through-a-pipe")]);
        drop(reader);
        let _ = std::fs::remove_dir_all(root);
    }

    /// **`<alias>` edits the family list as `FcConfigSubstitute` does**:
    /// each alias once, in the order read, its `<prefer>` before the first
    /// family it equals (blanks and case aside), its `<accept>` after it,
    /// its `<default>` at the end. An alias that tests a family only a later
    /// one adds is not applied; one that adds the name asked for ends with
    /// the list. Not rules: an unknown `binding`, no edits. Not applied: a
    /// `<test>` of `ja` under no locale, which is `en`, and two `<family>`s
    /// whose last is not in the list. What is tried in place of the name is
    /// the rest, each once, the strong-bound first: `C2`, which an alias
    /// bound `strong` adds.
    #[test]
    fn aliases_edit_the_family_list_as_fontconfig_does() {
        let a = Aliases::of_document(
            r#"<fontconfig>
  <alias><family>D Mono</family><prefer><family>Never</family></prefer></alias>
  <alias>
    <family>monospace</family>
    <prefer><family>A Mono</family><family>B Mono</family></prefer>
    <accept><family>C Mono</family></accept>
    <default><family>D Mono</family></default>
  </alias>
  <alias><family>A Mono</family><prefer><family>A2</family></prefer></alias>
  <alias><family>MONO SPACE</family><prefer><family>E</family></prefer></alias>
  <alias binding="bogus"><family>monospace</family><prefer><family>Dropped1</family></prefer></alias>
  <alias>
    <test name="lang"><string>ja</string></test>
    <family>monospace</family><prefer><family>Dropped2</family></prefer>
  </alias>
  <alias><family>monospace</family><family>other</family><prefer><family>Dropped3</family></prefer></alias>
  <alias><family>monospace</family></alias>
  <alias binding="strong"><family>C Mono</family><accept><family>C2</family></accept></alias>
  <alias binding="same"><family>B Mono</family><prefer><family>monospace</family></prefer></alias>
</fontconfig>"#,
        );
        assert_eq!(
            a.expand("monospace"),
            [
                "A2",
                "A Mono",
                "monospace",
                "B Mono",
                "E",
                "monospace",
                "C Mono",
                "C2",
                "D Mono"
            ]
        );
        assert_eq!(
            a.instead_of("Monospace"),
            ["C2", "A2", "A Mono", "B Mono", "E", "C Mono", "D Mono"]
        );
        assert_eq!(a.expand("serif"), ["serif"], "no alias for it");
        assert!(a.instead_of("serif").is_empty());
    }

    /// **An expansion holds at most [`ALIAS_LIST_MAX`] families**: an alias
    /// that prefers 300 holds the first 256 of them, and the name drops off.
    #[test]
    fn an_expansion_is_bounded() {
        let prefer: String = (0..300).map(|i| format!("<family>F{i}</family>")).collect();
        let a = Aliases::of_document(&format!(
            "<fontconfig><alias><family>mono</family><prefer>{prefer}</prefer></alias></fontconfig>"
        ));
        let e = a.expand("mono");
        assert_eq!(e.len(), ALIAS_LIST_MAX);
        assert_eq!((e[0].as_str(), e[255].as_str()), ("F0", "F255"));
    }

    /// **The aliases of every file read, in the order read**: the system's
    /// `conf.d` before the user's own file, which `50-user.conf` includes,
    /// so a user's alias for a family the system's adds is applied after.
    #[test]
    fn the_aliases_of_every_file_in_order() {
        let root = scratch("aliases");
        system(&root);
        write(
            &root.join("etc/fonts/conf.d/45-mono.conf"),
            "<fontconfig><alias><family>monospace</family>\
             <prefer><family>System Mono</family></prefer></alias></fontconfig>",
        );
        write(
            &root.join("home/.config/fontconfig/fonts.conf"),
            "<fontconfig><alias><family>System Mono</family>\
             <prefer><family>My Mono</family></prefer></alias></fontconfig>",
        );
        let c = load(&env(&root)).unwrap();
        assert_eq!(
            c.aliases.expand("monospace"),
            ["My Mono", "System Mono", "monospace"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A locale of `LANG` alone, or of the four variables.
    fn locale(
        fc_lang: Option<&str>,
        lc_all: Option<&str>,
        lc_ctype: Option<&str>,
        lang: Option<&str>,
    ) -> Locale {
        let var = |v: Option<&str>| v.map(OsString::from);
        Locale {
            fc_lang: var(fc_lang),
            lc_all: var(lc_all),
            lc_ctype: var(lc_ctype),
            lang: var(lang),
        }
    }

    /// **A pattern's languages are the locale's, as `FcGetDefaultLangs`
    /// and `FcLangNormalize` make them**: the codeset dropped, a territory
    /// kept only where fontconfig's table has the tag with it (`zh-TW`, not
    /// `ja-JP`), `C` as `en`, a language the table lacks kept whole,
    /// lowercased; `FC_LANG` first, an empty variable and an `LC_CTYPE` of
    /// just `UTF-8` passed over; `en` where nothing makes a tag; the list
    /// ending at a language already in it, caseless. Breaks: keep every
    /// territory, and `ja_JP.UTF-8` reads `ja-JP`.
    #[test]
    fn a_pattern_s_languages_are_the_locale_s_as_fontconfig_reads_it() {
        let lang = |l: &str| locale(None, None, None, Some(l)).langs();
        assert_eq!(lang("ja_JP.UTF-8"), ["ja"]);
        assert_eq!(lang("zh_TW.UTF-8"), ["zh-TW"]);
        assert_eq!(lang("C.UTF-8"), ["en"]);
        assert_eq!(lang("xx_YY"), ["xx-yy"], "not in the table: kept whole");
        assert_eq!(lang("x"), ["en"], "no tag");
        assert_eq!(locale(None, None, None, None).langs(), ["en"]);
        assert_eq!(
            locale(Some("ja:ko"), None, None, Some("de_DE")).langs(),
            ["ja", "ko"]
        );
        assert_eq!(
            locale(None, Some(""), Some("UTF-8"), Some("de_DE.UTF-8")).langs(),
            ["de"]
        );
        assert_eq!(
            locale(None, Some("fr_CA"), None, Some("de")).langs(),
            ["fr"]
        );
        assert_eq!(
            locale(Some("en:fr:EN:de"), None, None, None).langs(),
            ["en", "fr"],
            "ends at a language already held"
        );
    }

    /// **An alias's `<test>`s are evaluated against the pattern**: a
    /// `lang` that contains `ja` applies under `ja_JP.UTF-8` and not under
    /// `en_US`; a test of an object the pattern does not hold applies under
    /// `qual="all"` and not under `any`; `prgname` is `baud`; `less` never
    /// compares a string; a test fontconfig drops (an unknown `qual`) leaves
    /// its alias untested. A test of other than one `<string>`, or with
    /// `target="font"`, is not evaluated, its alias not applied, and the
    /// warning counts them. Breaks: read an absent object as matching under
    /// every qual, and `Absent Any` is applied.
    #[test]
    fn an_alias_s_tests_are_evaluated_against_the_pattern() {
        let doc = r#"<fontconfig>
  <alias><test name="lang" compare="contains"><string>ja</string></test>
    <family>monospace</family><prefer><family>JP Mono</family></prefer></alias>
  <alias><test name="spacing" qual="all"><string>x</string></test>
    <family>monospace</family><prefer><family>Absent All</family></prefer></alias>
  <alias><test name="spacing"><string>x</string></test>
    <family>monospace</family><prefer><family>Absent Any</family></prefer></alias>
  <alias><test name="prgname"><string>baud</string></test>
    <family>monospace</family><prefer><family>Baud Mono</family></prefer></alias>
  <alias><test name="prgname" compare="not_eq"><string>baud</string></test>
    <family>monospace</family><prefer><family>Not Baud</family></prefer></alias>
  <alias><test name="lang" compare="less"><string>zz</string></test>
    <family>monospace</family><prefer><family>Less</family></prefer></alias>
  <alias><test name="lang" qual="bogus"><string>zz</string></test>
    <family>monospace</family><prefer><family>Untested</family></prefer></alias>
  <alias><test name="size"><double>12</double></test>
    <family>monospace</family><prefer><family>Double</family></prefer></alias>
  <alias><test name="lang" target="font"><string>ja</string></test>
    <family>monospace</family><prefer><family>Font</family></prefer></alias>
</fontconfig>"#;
        let ja = Aliases::of_document_in(doc, &locale(None, None, None, Some("ja_JP.UTF-8")));
        assert_eq!(
            ja.expand("monospace"),
            [
                "JP Mono",
                "Absent All",
                "Baud Mono",
                "Untested",
                "monospace"
            ]
        );
        let en = Aliases::of_document_in(doc, &locale(None, None, None, Some("en_US.UTF-8")));
        assert_eq!(
            en.expand("monospace"),
            ["Absent All", "Baud Mono", "Untested", "monospace"]
        );
        assert_eq!(
            en.unapplied().as_deref(),
            Some(
                "fontconfig: 2 <alias> rules not applied: a <test> with an expression other \
                 than one <string>, or with target font or scan, is not evaluated"
            )
        );
        assert_eq!(Aliases::of_document("<fontconfig/>").unapplied(), None);
    }

    /// **Aliases run in order over one pattern that keeps each family's
    /// binding**: alias A, bound `strong`, puts `P1` before `monospace`, and
    /// alias B, naming `P1` and `monospace`, applies because `monospace`,
    /// the last, is in the list, placing `Q` at `P1`, the first of the run
    /// of matches ending with it. An alias whose earlier family is missing
    /// applies at the last; one whose last is missing does not. `same`
    /// takes the binding of the family it is placed by -- strong by `P1`,
    /// weak by `Q` -- and weak at the end. Breaks: take the first family's
    /// match, and `Never` is applied; bind `same` strong, and `T` ranks
    /// with the strong.
    #[test]
    fn aliases_run_in_order_over_one_pattern_with_its_bindings() {
        let a = Aliases::of_document(
            r#"<fontconfig>
  <alias binding="strong"><family>monospace</family><prefer><family>P1</family></prefer></alias>
  <alias><family>P1</family><family>monospace</family><prefer><family>Q</family></prefer></alias>
  <alias><family>nope</family><family>monospace</family><prefer><family>R</family></prefer></alias>
  <alias><family>monospace</family><family>nope</family><prefer><family>Never</family></prefer></alias>
  <alias binding="same"><family>P1</family><prefer><family>S</family></prefer></alias>
  <alias binding="same"><family>Q</family><prefer><family>T</family></prefer>
    <default><family>U</family></default></alias>
</fontconfig>"#,
        );
        assert_eq!(
            a.expand("monospace"),
            ["T", "Q", "S", "P1", "R", "monospace", "U"]
        );
        assert_eq!(
            a.order("monospace"),
            ["S", "P1", "monospace", "T", "Q", "R", "U"]
        );
    }

    /// **An installed family gives way only to a `<prefer>` bound strong**:
    /// against a fake set of installed families, `Foo` is drawn as itself
    /// under a weak alias that prefers `Bar`, and as `Bar` under the same
    /// alias bound `strong` or `same`; a name nothing installed answers to
    /// takes `Bar` either way. Breaks: rank by the list alone, and the weak
    /// alias draws `Bar`.
    #[test]
    fn an_installed_family_gives_way_only_to_a_strong_prefer() {
        let installed = ["Foo", "Bar"];
        let drawn = |binding: &str, name: &str| {
            let a = Aliases::of_document(&format!(
                "<fontconfig><alias{binding}><family>{name}</family>\
                 <prefer><family>Bar</family></prefer></alias></fontconfig>"
            ));
            a.order(name)
                .into_iter()
                .find(|f| installed.iter().any(|i| blank_caseless(i, f)))
        };
        assert_eq!(drawn("", "Foo").as_deref(), Some("Foo"));
        assert_eq!(drawn(r#" binding="weak""#, "Foo").as_deref(), Some("Foo"));
        assert_eq!(drawn(r#" binding="strong""#, "Foo").as_deref(), Some("Bar"));
        assert_eq!(drawn(r#" binding="same""#, "Foo").as_deref(), Some("Bar"));
        assert_eq!(drawn("", "Missing").as_deref(), Some("Bar"));
    }
}
