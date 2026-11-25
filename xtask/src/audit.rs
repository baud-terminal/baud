//! `cargo xtask audit`: raw integers outside the definition crate's newtype
//! internals fail it.
//!
//! The definition crate legitimately contains them: a newtype's whole job is
//! to wrap one. Everywhere else they are a bug waiting for two quantities to
//! be confused. Every budget below is set at its count and is a floor: it
//! may go down and must never go up, and a budget that is not equal to the
//! count measures nothing. A crate or tree this table does not name is
//! audited by nothing, which looks exactly like passing.
//!
//! The order of the report is the order of this file, and every line of it
//! is the line the shell form of this gate printed.

use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::impl_audit::{Counts, Sites, Sum};
use crate::scan_audit;
use crate::text;
use crate::walk::{self, Depth, Files};

/// Which files of an aggregate get a line of their own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    /// A file with a raw-integer site.
    Total,
    /// A file with a raw-integer site or a signature primitive.
    Either,
}

/// What an aggregate does when its roots hold no sources.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Empty {
    /// Name what is empty, fail, and still report the (zero) count.
    Report(&'static str),
    /// Fail under the aggregate's own name and report nothing more.
    Stop,
}

/// A raw-integer budget and a signature budget over every `.rs` file under
/// some roots.
pub struct Crate {
    /// The name the report gives it.
    pub name: &'static str,
    /// The directories it walks.
    pub roots: &'static [&'static str],
    /// The raw-integer sites it may hold.
    pub total: Sites,
    /// The signature primitives it may hold.
    pub signature: Sites,
    /// Which files get a line.
    pub show: Show,
    /// What an empty walk does.
    pub empty: Empty,
    /// Said after the limits on a pass.
    pub note: &'static str,
}

/// **The ISA crates, on their own budget.** One raw-integer site and zero
/// signature primitives: the cast feeding `LaneMask::from_movemask` in
/// `scan_avx2`, which is the one point where an intrinsic's return value
/// enters the type system. They were inside `baud-core+isa`'s 14 and
/// invisible there -- moving the scanners into their own crates moved the
/// sites with them, and the gate must follow the code in the same commit.
/// It did not, for as long as one limit covered both.
///
/// **It was three, the same cast in three AVX2 scans, until 48b4934 deleted
/// `find_final` and `find_st` with theirs -- and the limit stayed at 3
/// after**, slack that would have let two new casts land green.
pub const ISA: Crate = Crate {
    name: "baud-isa*",
    roots: &[
        "baud-isa/src",
        "baud-isa-x86-64/src",
        "baud-isa-aarch64/src",
    ],
    total: Sites(1),
    signature: Sites(0),
    show: Show::Total,
    empty: Empty::Report("baud-isa*/src"),
    note: ", the movemask cast",
};

/// The core, on its own budget. **Two, and every one enumerated**:
///
///   1  `GRID`, the product of `MAX_ROWS` and `MAX_COLS`, an array dimension. Rust requires an
///      array length to be a `usize` const expression and no newtype can be
///      one -- forced by the language, not chosen.
///   1  the cast in `Nodes::at`, where a graph node becomes an array index.
///      A `repr(u8)` enum indexing its own table is the one cast the
///      language forces, and it is the single point a node enters index
///      space.
///
/// **Signatures are at zero and the limit follows the count.** It got there
/// when the one-line loop was typed -- `at`/`len`/`left`/`k` came in as bare
/// `usize` and left as `BytePos`, `ReadLen`, `ByteLen` and `Count`, and
/// `build(latin1: bool)` became `build(enc: InputEncoding)`, which is the
/// two-state enum a `bool` parameter always is.
///
/// It went 14 to 8 to 4 to 3 to 2 by moving byte-to-bits into `StreamByte`,
/// the stride maths into `RowBlock` and the offsets onto `ByteLen` -- none
/// of which was re-scoping.
pub const CORE: Crate = Crate {
    name: "baud-core",
    roots: &["baud-core/src"],
    total: Sites(2),
    signature: Sites(0),
    show: Show::Either,
    empty: Empty::Report("baud-core/src"),
    note: ", every site enumerated",
};

/// One crate's budget, reported under its own name.
const fn krate(name: &'static str, root: &'static [&'static str], t: Sites, s: Sites) -> Crate {
    Crate {
        name,
        roots: root,
        total: t,
        signature: s,
        show: Show::Either,
        empty: Empty::Stop,
        note: "",
    }
}

/// **The renderer crates and the rest, each on its own budget from the day
/// it is created**, gated in the same commit that creates the crate, and
/// each change that adds code sets the limit to the count it ends at, never
/// above it.
pub const CRATES: [Crate; 11] = [
    // **baud-font: three raw-integer sites and forty signature primitives,
    // every one enumerated, and the limit follows the count downward.** The
    // signature sites are the newtype surface of a parser over a byte
    // format -- the same exemption `baud-domain` has for the same reason:
    // `Len::of(u32)`, `FaceIndex::of(u32)`, `Palette::of(u16)`,
    // `PaletteEntry::of(u16)`, the `value()` accessors (`Ppem`,
    // `PaletteEntry` among them), `Offset::index -> usize` (the one bridge
    // into slice indexing), `is_sfnt(u32)` on the directory's magic,
    // `glyph(u16)` where a `cmap` value becomes a `GlyphId`, the
    // `parse(u16)` constructors that turn a `CBLC` index or image format
    // number into its enum, and in the outline readers the bytes a
    // charstring and a DICT are made of: `OffSize::parse(u8)`,
    // `dict_number`'s first byte, `sid_to_gid(u16)` on a string id,
    // `usize_of -> u32` on an offset operand, `subr_index`'s bias as `i32`,
    // `f2dot14(i16) -> f32`, and `Delta::of(bool, bool)` built from two
    // flag bits. A DICT operator is a `DictOp`, a call depth a `Depth`, and
    // the path operators CFF and CFF2 share take their operands as a slice,
    // so no width slot is passed as a `usize`. The PNG decoder was the third interpreter, with
    // twenty signature sites and six raw ones; it moved to `baud-domain` for
    // V19 step 2 and took them with it, and the limits fell from 12 and 60.
    // The rasteriser carries eight more, all pixel arithmetic: `Px::of(u32)`
    // and `Px::value`, `Scale::of` taking pixels per em as `f32`,
    // `Scale::placing`'s two pixel offsets, `Point::new`'s two coordinates,
    // `Point::lerp`'s parameter, `distance_to`'s length, `row(f32) -> usize`
    // -- the one place a pixel coordinate becomes a row index, spelled once
    // -- and `splat`'s row start as `usize`, which is also the third raw
    // site: the reference's own index arithmetic, kept in its shape so the
    // byte-exact differential has one algorithm to compare rather than two.
    // The shaper adds three signature sites and no raw ones: `same(GlyphId,
    // u16)` comparing a glyph to the value a layout table stores,
    // `LookupFlags::has(u16)` testing one flag bit, and `Count::slot ->
    // usize`, the second bridge into slice indexing beside `Offset::index`.
    // Buffer positions are `Index`, lengths are `Count`, table glyph ids are
    // `GlyphId`, classes are `Class`, lookup types are `Kind` and flags are
    // `LookupFlags`, so a position and a count cannot be added and a class
    // and a glyph cannot be confused. The other two raw sites are the
    // interpreters' state: the charstring stack length and the stem count
    // that sizes `hintmask` (CFF's; CFF2 keeps its stems as a `Count` and
    // shares the stack); `glyf` carries its recursion depth and contour
    // count as `Count`. Every `bool` field became a two-state enum
    // before this count was taken (`Path`, `Moveto`, `Width`, `End`, `Axis`,
    // `Delta`, `Sign`, `IndexFormat`, `ImageFormat`, and CFF2's `Variation`
    // and `Regions`).
    krate("baud-font", &["baud-font/src"], Sites(3), Sites(40)),
    // baud-render: 0 raw sites and 11 signature primitives, every one a unit
    // type's own edge -- `Tally::of(usize)` and `Tally::value -> u64`,
    // `Digest::fold(u64)` and `Digest::value -> u64`, `slots -> usize` (the
    // one place a pixel count becomes a slice index), `screen(i32)` (the one
    // place pixel arithmetic becomes an instance coordinate), `Side::value
    // -> u32`, `Key::fold(u32)`, `FaceIx::index -> usize`, and the two
    // colour helpers `triple(u8, u8, u8)` and `grey(u8)` that build an
    // `Argb` from wire bytes.
    krate("baud-render", &["baud-render/src"], Sites(0), Sites(11)),
    // baud-present-macos: one signature primitive, `slots(Px) -> usize`,
    // where a pixel count becomes a slice index. baud-app shares that edge,
    // and its own line below enumerates the rest of its count.
    krate(
        "baud-present-macos",
        &["baud-present-macos/src"],
        Sites(0),
        Sites(1),
    ),
    // baud-wayland: seven raw sites and seven signature primitives, and the
    // floor is the kernel's socket ABI -- `sys.rs` declares
    // `sendmsg`/`recvmsg` and their `msghdr`/`cmsghdr` as the platform lays
    // them out (`i32` flags, and a length whose width is the platform's,
    // `Len`, crossed by `ControlBytes` in one place), the way the ISA crates
    // declare `sysctlbyname` -- plus the wire's own widths: a header's
    // `usize` size, the byte cursor `put`/`word` carry, and the 24.8
    // fixed-point's saturating cast. The protocol values (`ObjectId(u32)`,
    // `Opcode(u16)`, `Fixed(i32)`) are newtypes. It was eleven and eight
    // until the lengths took a type; set at the count, and it does not move
    // to fit a change.
    krate("baud-wayland", &["baud-wayland/src"], Sites(7), Sites(7)),
    // baud-xkb: 10 raw sites and 5 signature primitives, and they are the
    // keymap's own numbers -- keycode ranges, level and group indices, the
    // 32-bit modifier mask the Wayland protocol serialises (a type alias,
    // since it is the wire's encoding and not an index) -- plus the byte
    // cursor the lexer carries. The keysym and the keycode are newtypes.
    krate("baud-xkb", &["baud-xkb/src"], Sites(10), Sites(5)),
    // baud-present-wayland: 5 raw sites and 10 signature primitives, the
    // kernel's ABI in `sys.rs` (`mmap`, `munmap`, `ftruncate`,
    // `memfd_create` and the `fcntl` a clipboard pipe is made non-blocking
    // with, as the externs they are) and the pool's byte arithmetic -- a
    // stride in words on the pool and on `present`, an offset into the file
    // -- which the wire carries as `int`; plus the keyboard's `group` field
    // and `bind`'s wire `name` and `version`, which sit one to a line and so
    // read to the audit as fields. It was 14 until `premultiply` moved to
    // `baud-domain`, which both presenters call, and 6 and 12 until the
    // mapping's length became `sys::FileLen`, which took three signatures
    // and a field as `fcntl` added one.
    krate(
        "baud-present-wayland",
        &["baud-present-wayland/src"],
        Sites(5),
        Sites(10),
    ),
    // baud-present-vulkan: 0 raw sites and 1 signature primitive, the
    // kernel's ABI in `sys.rs`: `ioctl` declared as the extern it is, its
    // fd, request and result the platform's widths. Vulkan's own integers
    // stay inside `ash`'s types; a node, a modifier, a byte count and a
    // memory type are newtypes.
    krate(
        "baud-present-vulkan",
        &["baud-present-vulkan/src"],
        Sites(0),
        Sites(1),
    ),
    // baud-app: 1 raw site and 4 signature primitives. One is `slots(Px) ->
    // usize` in the presenter edge, as above; the other four are the kernel
    // ABI in `sys.rs`, which declares the pty, the ioctl, the poll, the
    // mapping and Darwin's raw clock as the externs they are rather than
    // through a `libc` crate. The three parameters are `size_t` byte counts
    // on `read`, `write` and `mmap`, and the field is the window size's own
    // `u16`: the platform's widths, which a newtype would misrepresent
    // rather than document.
    krate("baud", &["baud-app/src"], Sites(1), Sites(4)),
    // baud-tablegen: 4 raw sites and 25 signature primitives, and they are
    // the text it reads: a code point as the `u32` the UCD's hex is
    // (`CpRange::lo`/`hi`, `get`, `code_point`), a line and a field index as
    // `usize` (`Row::field`, `get`, `error`, `expect`, `Source::line`, the
    // value sets' `check`), the FNV-1a hash's `u32` seed and value and the
    // `u16` seeds it finds, a trie's `shift` and the counts `Errors::count`
    // compares, plus `From<bool> for Check`,
    // the one bridge into the two-state enum, and the cast the generated
    // const pin carries as text. It is a build-dependency with no
    // dependencies, so `baud-domain`'s types are not there to take. The
    // perfect hash's bucket and slot counts are `Buckets` and `Slots`,
    // which `place` takes too, so the two cannot be swapped, and the values
    // a line of `Emit::array` holds are a `PerLine`.
    krate("baud-tablegen", &["baud-tablegen/src"], Sites(4), Sites(25)),
    // baud-pipeline: 5 raw sites and no signature primitives, every one a
    // struct field: the handoff's two slot indices, `IngestEnd::back` and
    // `FrameEnd::front`, each the `u8` the shared `AtomicU8` swaps; the
    // stress test's `Seq::first` and `Seq::last`, the sequence number a torn
    // read would show as two; and `Lender::out`, the leases out, compared
    // with the const generic capacity `N`, which the language requires to
    // be a `usize`.
    krate("baud-pipeline", &["baud-pipeline/src"], Sites(5), Sites(0)),
    // xtask: 5 raw sites and no signature primitives, and every one is the
    // audit's own text matching itself -- the category names it prints,
    // which the table must spell as it always has (the cast category's
    // name spells three casts, the index category's name and pattern one
    // index product each). Its numbers are `Sites`, `Depth` and `Line`.
    krate("xtask", &["xtask/src"], Sites(5), Sites(0)),
];

/// A raw-integer budget over a tree, with its signature primitives not
/// counted, and each file with a site named.
pub struct Tree {
    /// The name the report gives it.
    pub label: &'static str,
    /// The directory it walks.
    pub dir: &'static str,
    /// The raw-integer sites it may hold.
    pub limit: Sites,
}

const fn tree(dir: &'static str, limit: Sites) -> Tree {
    Tree {
        label: dir,
        dir,
        limit,
    }
}

/// **baud-bench is an aggregate for the same reason baud-core is**: its
/// permitted sites live across several files, and per-file limits would
/// have to be re-guessed every time a function moves. **Two trees since the
/// measurement programs became `[[bench]]` targets**: they were
/// `baud-bench/src/bin/*.rs` and are `baud-bench/benches/*.rs`, so one walk
/// over `src` went from auditing eleven raw sites to auditing none while
/// still reporting "ok (limit 14)". The field pattern is anchored to a line
/// and so cannot tell a struct field from a parameter rustfmt has put on a
/// line of its own; the benches' count carries such parameters.
///
/// **Every other crate's `tests/` was audited by nothing** until these rows:
/// the eleven test trees carried 100 raw-integer sites between them, the
/// same scoped-to-what-it-was-written-beside failure as `docs.rs`'s `ROOTS`
/// and `feature-arms`'s crate list. The rows that read zero are listed
/// rather than omitted -- a tree absent from this list is a tree nothing
/// checks -- and a named directory that holds no sources fails, so a rename
/// cannot silently empty one.
pub const TREES: [Tree; 16] = [
    tree("baud-bench/src", Sites(0)),
    tree("baud-bench/benches", Sites(11)),
    tree("baud-domain/tests", Sites(35)),
    tree("baud-font/tests", Sites(48)),
    tree("baud-render/tests", Sites(6)),
    tree("baud-present-macos/tests", Sites(1)),
    tree("baud-xkb/tests", Sites(1)),
    tree("baud-bench/tests", Sites(0)),
    tree("baud-isa/tests", Sites(0)),
    tree("baud-isa-aarch64/tests", Sites(0)),
    tree("baud-isa-x86-64/tests", Sites(0)),
    tree("baud-wayland/tests", Sites(0)),
    tree("baud-present-wayland/tests", Sites(0)),
    tree("baud-present-vulkan/tests", Sites(0)),
    tree("baud-tablegen/tests", Sites(0)),
    tree("xtask/tests", Sites(0)),
];

/// The suites under a test directory: every `.rs` file at least one
/// directory down, counted together and listed by nobody.
pub struct Suites {
    /// The name the report gives it.
    pub name: &'static str,
    /// The test directory.
    pub root: &'static str,
    /// The raw-integer sites they may hold.
    pub limit: Sites,
}

/// **The flat file list reaches none of baud-core's suites**: the loose
/// files match `baud-core/tests/*.rs` and the directories `conformance/`,
/// `vt100/`, `xterm/` and the rest do not, so they were audited by nothing
/// for as long as those directories existed. Gated at its count rather than
/// at zero, because a limit set to a number nobody has driven to fails on
/// the next commit for a reason unrelated to it. **It is a floor**: the
/// loose files reached 0 and this should. 35, up one for `spec_inventory`'s
/// `images_refused` counter field, which it checks so a `DCS q` registers as
/// handled-not-silent, the same shape as the `unsupported`/`bells` counter
/// fields beside it.
pub const SUITES: Suites = Suites {
    name: "baud-core/tests suites",
    root: "baud-core/tests",
    limit: Sites(35),
};

/// The loose test files, expanded as a glob, in front of [`FILES`].
pub const GLOB: &str = "baud-core/tests";

/// The files named one by one after the glob, in this order.
pub const FILES: [&str; 5] = [
    "baud-difftest/src/lib.rs",
    "baud-difftest/tests/alacritty.rs",
    "baud-sfte-difftest/tests/sfte.rs",
    "baud-sfte-difftest/src/lib.rs",
    "baud-sfte-difftest/src/ffi.rs",
];

/// **Per file, and three of baud-core's are not zero**: written for this
/// core rather than ported, and never audited while the glob named the
/// other one. Each is pinned at what it is so it cannot grow, which is the
/// same contract every file at zero carries.
pub const FILE_LIMITS: [(&str, Sites); 3] = [
    ("baud-core/tests/text.rs", Sites(5)),
    ("baud-core/tests/properties.rs", Sites(4)),
    ("baud-core/tests/fixed.rs", Sites(1)),
];

/// The scan limit: the count may go down and must never go up.
pub const SCAN_LIMIT: scan_audit::Limit = scan_audit::Limit(5);

/// **baud-domain is exempt from the count -- a newtype's whole job is to
/// wrap a primitive -- but not from this.** The exemption also hid
/// primitives on the crate's *API surface*, and five `bool` fields
/// accumulated inside protocol enums -- `Csi::Mode { set: bool }`,
/// `Csi::SelectProtection { protect: bool }`, `Report::MouseSgr { release:
/// bool }` and two more. A `bool` *field* is never a newtype internal -- a
/// newtype over bool is `struct X(bool)`, which this pattern does not match
/// -- so zero is the right limit and there is no judgement call in it. The
/// whole directory is read, every file in it, line by line.
pub const BOOL_DIR: &str = "baud-domain/src/";

/// A `bool` field, one to a line.
pub const BOOL_FIELD: &str = r"^[[:space:]]+[a-z_]+: bool,$";

/// **The four crates the debug profile optimises have no address backstop
/// but this one.** `Cargo.toml` builds baud-domain, baud-core, baud-font and
/// baud-render at opt-level 1 in `dev`, where opt-level 0 kept a separate
/// copy of a const's bytes in each codegen unit: a test comparing a const's
/// address from two modules was red in a debug run and green in release,
/// which is how the debug gate once caught one in baud-app. Optimised, those
/// crates merge the copies in debug too, so their address operations are
/// held here to the count, each read when the budget was set: heap or
/// caller-supplied buffers, a read buffer handed to a syscall, a font file's
/// bytes, and statics, which have one address in every profile. A new one
/// fails until it is read the same way and the budget moves with it. The
/// count is of lines, over every `.rs` file in the crate's directory.
pub const ADDRESS: &str = r"\.as_ptr\(\)|\.as_ptr_range\(\)|\.as_mut_ptr\(\)|\.addr\(\)|ptr::eq\(|ptr::addr_eq\(|as \*const|as \*mut|\{:#?p\}";

/// **The pattern is checked on a sample first**: one that stopped matching
/// would count zero and pass.
pub const ADDRESS_SAMPLE: [&str; 5] = [
    "a.as_ptr()",
    "b.addr()",
    "core::ptr::eq(",
    "x as *const u8",
    "\"{:p}\"",
];

/// The address budgets.
pub const ADDRESSES: [(&str, Sites); 4] = [
    ("baud-domain", Sites(11)),
    ("baud-core", Sites(13)),
    ("baud-font", Sites(0)),
    ("baud-render", Sites(2)),
];

/// The target the other architecture's ISA crate is built for. Not
/// `x86_64-unknown-none`: that target is soft-float, and rustc refuses
/// `#[target_feature(enable = "avx2")]` there (`x86_softfloat_sse`,
/// rust-lang/rust#117938). `nix/rust.nix` fetches this one's standard
/// library, and the crate is `no_std` without its `std` feature either way.
pub const CROSS_TARGET: &str = "x86_64-unknown-linux-gnu";

/// **A positive control decides whether the target is usable**, because
/// asking `rustup` does not: this toolchain is nix-provided and `rustup
/// target list --installed` prints nothing while the cross-build works. A
/// crate known to be fine for the target is built first; if it fails the
/// toolchain is the problem and the gate says so.
pub const CROSS_CONTROL: &str = "baud-isa";

/// **The other architecture's crate must still compile, and nothing else
/// checks it.** `baud-isa-x86-64` is `#![cfg(target_arch = "x86_64")]` and
/// `baud-isa-aarch64` is the mirror, so on any one machine exactly one of
/// them is an empty crate that type-checks vacuously. Adding `stride_three`
/// and `stride_four` to `Scan` left the x86-64 impl missing two methods and
/// **nothing went red**: not the tests, not clippy, not this gate.
pub const CROSS_CRATE: &str = "baud-isa-x86-64";

/// Builds a package for [`CROSS_TARGET`].
pub trait Cross {
    /// Whether `package` builds.
    fn builds(&self, package: &str) -> bool;
}

/// The cargo that ran this, in release, quietly.
pub struct Cargo {
    /// The workspace it builds in.
    pub root: PathBuf,
}

impl Cross for Cargo {
    fn builds(&self, package: &str) -> bool {
        let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
        Command::new(cargo)
            .args(["build", "--quiet", "--release", "-p", package])
            .args(["--target", CROSS_TARGET])
            .current_dir(&self.root)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// One run of the audit over a tree.
pub struct Audit<'a> {
    /// The tree's root; every path is named relative to it.
    pub root: &'a Path,
    /// Where the report goes.
    pub out: &'a mut dyn Write,
    /// Whether anything has failed.
    pub failed: Failed,
}

/// Whether a gate has failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failed {
    /// Nothing has.
    No,
    /// Something has.
    Yes,
}

impl<'a> Audit<'a> {
    /// A run over `root` writing to `out`.
    pub fn new(root: &'a Path, out: &'a mut dyn Write) -> Self {
        Self {
            root,
            out,
            failed: Failed::No,
        }
    }

    fn fail(&mut self) {
        self.failed = Failed::Yes;
    }

    fn counts(&self, file: &str) -> io::Result<Counts> {
        let src = text::read_universal(&self.root.join(file))
            .map_err(|e| io::Error::new(e.kind(), format!("{file}: {e}")))?;
        Ok(Counts::of(&src))
    }

    /// Every gate, in the report's order.
    pub fn run(&mut self, cross: &dyn Cross) -> io::Result<Failed> {
        self.scans(SCAN_LIMIT)?;
        self.cross(cross)?;
        self.krate(&ISA)?;
        self.krate(&CORE)?;
        for c in &CRATES {
            self.krate(c)?;
        }
        for t in &TREES {
            self.tree(t)?;
        }
        self.suites(&SUITES)?;
        self.files()?;
        self.bools()?;
        self.addresses(ADDRESS)?;
        Ok(self.failed)
    }

    /// The ingest path's scan loops, against `limit`.
    /// A file it cannot read fails the gate and the audit goes on.
    pub fn scans(&mut self, limit: scan_audit::Limit) -> io::Result<()> {
        let src = match text::read_universal(&self.root.join(scan_audit::PATH)) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("scan-audit: {}: {e}", scan_audit::PATH);
                self.fail();
                return Ok(());
            }
        };
        let (report, ok) = scan_audit::report(&scan_audit::scans(&src), limit);
        self.out.write_all(report.as_bytes())?;
        if !ok {
            self.fail();
        }
        Ok(())
    }

    /// The control crate, then the other architecture's ISA crate.
    pub fn cross(&mut self, cross: &dyn Cross) -> io::Result<()> {
        self.out.flush()?;
        if !cross.builds(CROSS_CONTROL) {
            writeln!(
                self.out,
                "  FAIL {CROSS_TARGET}: the control crate does not build, so this gate checked nothing"
            )?;
            self.fail();
        } else if cross.builds(CROSS_CRATE) {
            writeln!(self.out, "  ok   {CROSS_CRATE}: cross-compiles")?;
        } else {
            writeln!(
                self.out,
                "  FAIL {CROSS_CRATE}: does not compile for x86_64 -- the trait moved and this impl did not"
            )?;
            self.fail();
        }
        Ok(())
    }

    /// One aggregate budget.
    pub fn krate(&mut self, c: &Crate) -> io::Result<()> {
        let mut files: Vec<String> = c
            .roots
            .iter()
            .flat_map(|r| walk::find_rs(self.root, r, Depth(1)))
            .collect();
        files.sort();
        if files.is_empty() {
            match c.empty {
                Empty::Report(what) => {
                    writeln!(
                        self.out,
                        "  FAIL {what}: no sources found -- this gate audited nothing"
                    )?;
                    self.fail();
                }
                Empty::Stop => {
                    writeln!(
                        self.out,
                        "  FAIL {}: no sources found -- this gate audited nothing",
                        c.name
                    )?;
                    self.fail();
                    return Ok(());
                }
            }
        }
        let (mut total, mut sig) = (Sites(0), Sites(0));
        for f in &files {
            let n = self.counts(f)?;
            let (t, s) = (n.sum(Sum::Total), n.sum(Sum::Signature));
            total = total + t;
            sig = sig + s;
            let shown = match c.show {
                Show::Total => t > Sites(0),
                Show::Either => t + s > Sites(0),
            };
            if shown {
                writeln!(self.out, "  ..   {f}: {t} + {s} sig")?;
            }
        }
        if total > c.total || sig > c.signature {
            writeln!(
                self.out,
                "  FAIL {}: {total} raw-integer sites, {sig} signature (limit {} and {})",
                c.name, c.total, c.signature
            )?;
            self.fail();
        } else {
            writeln!(
                self.out,
                "  ok   {}: {total} and {sig} (limit {} and {}{})",
                c.name, c.total, c.signature, c.note
            )?;
        }
        Ok(())
    }

    /// One tree's raw-integer budget.
    pub fn tree(&mut self, t: &Tree) -> io::Result<()> {
        let files = walk::find_rs(self.root, t.dir, Depth(1));
        if files.is_empty() {
            writeln!(
                self.out,
                "  FAIL {}: no sources found -- this gate audited nothing",
                t.label
            )?;
            self.fail();
            return Ok(());
        }
        let mut total = Sites(0);
        for f in &files {
            let n = self.counts(f)?.sum(Sum::Total);
            total = total + n;
            if n > Sites(0) {
                writeln!(self.out, "  ..   {f}: {n}")?;
            }
        }
        if total > t.limit {
            writeln!(
                self.out,
                "  FAIL {}: {total} raw-integer sites (limit {})",
                t.label, t.limit
            )?;
            self.fail();
        } else {
            writeln!(self.out, "  ok   {}: {total} (limit {})", t.label, t.limit)?;
        }
        Ok(())
    }

    /// The suites' budget.
    pub fn suites(&mut self, s: &Suites) -> io::Result<()> {
        let files = walk::find_rs(self.root, s.root, Depth(2));
        if files.is_empty() {
            writeln!(
                self.out,
                "  FAIL {}: no sources found -- this gate audited nothing",
                s.name
            )?;
            self.fail();
            return Ok(());
        }
        let mut total = Sites(0);
        for f in &files {
            total = total + self.counts(f)?.sum(Sum::Total);
        }
        if total > s.limit {
            writeln!(
                self.out,
                "  FAIL {}: {total} raw-integer sites (limit {})",
                s.name, s.limit
            )?;
            self.fail();
        } else {
            writeln!(
                self.out,
                "  ok   {}: {total} (limit {}, a floor to drive down)",
                s.name, s.limit
            )?;
        }
        Ok(())
    }

    /// The loose files, each on its own limit. **A gate that cannot find its
    /// input must fail, not pass**: a renamed or removed file, or a glob that
    /// matches nothing and so names only itself, is "no such file"; a file
    /// that cannot be read as text is "no count".
    pub fn files(&mut self) -> io::Result<()> {
        let mut all = walk::glob_rs(self.root, GLOB);
        all.extend(FILES.iter().map(|f| f.to_string()));
        for f in &all {
            if !self.root.join(f).is_file() {
                writeln!(
                    self.out,
                    "  FAIL {f}: no such file -- this gate audited nothing"
                )?;
                self.fail();
                continue;
            }
            let limit = FILE_LIMITS
                .iter()
                .find(|(p, _)| p == f)
                .map_or(Sites(0), |(_, l)| *l);
            match self.counts(f) {
                Err(_) => {
                    writeln!(self.out, "  FAIL {f}: the audit produced no count")?;
                    self.fail();
                }
                Ok(n) if n.sum(Sum::Total) > limit => {
                    writeln!(
                        self.out,
                        "  FAIL {f}: {} raw-integer sites (limit {limit})",
                        n.sum(Sum::Total)
                    )?;
                    self.fail();
                }
                Ok(n) => {
                    writeln!(
                        self.out,
                        "  ok   {f}: {} (limit {limit})",
                        n.sum(Sum::Total)
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Zero `bool` fields in baud-domain.
    pub fn bools(&mut self) -> io::Result<()> {
        let re = Regex::new(BOOL_FIELD).expect("the bool-field pattern");
        let hits = walk::grep(self.root, BOOL_DIR, Files::All, &re);
        if hits.is_empty() {
            writeln!(self.out, "  ok   baud-domain/src: 0 bool fields (limit 0)")?;
            return Ok(());
        }
        writeln!(
            self.out,
            "  FAIL baud-domain/src: {} bool fields (limit 0) -- use a named two-state enum",
            hits.len()
        )?;
        for h in &hits {
            writeln!(self.out, "{}:{}:{}", h.path, h.line, h.text)?;
        }
        self.fail();
        Ok(())
    }

    /// Each optimised crate's address operations, with `pattern` checked on
    /// its sample first.
    pub fn addresses(&mut self, pattern: &str) -> io::Result<()> {
        let re = Regex::new(pattern).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        if ADDRESS_SAMPLE.iter().filter(|l| re.is_match(l)).count() != ADDRESS_SAMPLE.len() {
            writeln!(
                self.out,
                "  FAIL address operations: the pattern misses its own sample -- this gate counts nothing"
            )?;
            self.fail();
        }
        for (krate, limit) in ADDRESSES {
            if walk::find_rs(self.root, krate, Depth(1)).is_empty() {
                writeln!(
                    self.out,
                    "  FAIL {krate}: no sources found -- this gate audited nothing"
                )?;
                self.fail();
                continue;
            }
            let hits = walk::grep(self.root, krate, Files::Rust, &re);
            let n = Sites(hits.len());
            if n > limit {
                writeln!(
                    self.out,
                    "  FAIL {krate}: {n} lines with an address operation (limit {limit})"
                )?;
                for h in &hits {
                    writeln!(self.out, "{}:{}:{}", h.path, h.line, h.text)?;
                }
                self.fail();
            } else {
                writeln!(
                    self.out,
                    "  ok   {krate}: {n} lines with an address operation (limit {limit})"
                )?;
            }
        }
        Ok(())
    }
}
