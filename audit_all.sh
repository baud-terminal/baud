#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0

# **The ingest path answers by indexing, not by searching.** `scripts/scan-audit.py`
# carries the reasoning and the exemptions; the number may go down and must
# never go up.
if ! python3 scripts/scan-audit.py 5; then
  fail=1
fi

# **The ISA crates, on their own budget.** One raw-integer site and zero
# signature primitives: the `as u32` feeding `LaneMask::from_movemask` in
# `scan_avx2`, which is the one point where an intrinsic's return value enters
# the type system. They were inside `baud-core+isa`'s 14 and invisible there --
# moving the scanners into their own crates moved the sites with them, and this
# repo already records that "the gate must follow the code in the same commit".
# It did not, for as long as one limit covered both.
#
# **It was three, the same cast in three AVX2 scans, until 4089bc3
# (2026-09-19) deleted `find_final` and `find_st` with theirs -- and the limit
# stayed at 3 after**, slack that would have let two new casts land green. A
# budget that is not equal to the count measures nothing; set at the count.
isa=0
isasig=0
# **The other architecture's crate must still compile, and nothing checked it.**
#
# `baud-isa-x86-64` is `#![cfg(target_arch = "x86_64")]` and `baud-isa-aarch64`
# is the mirror, so on any one machine exactly one of them is an empty crate that
# type-checks vacuously. Adding `stride_three` and `stride_four` to `Scan` left
# the x86-64 impl missing two methods and **nothing went red**: not the 615
# tests, not clippy, not this script -- the same absent-rather-than-red failure
# this repo records for the renamed CI target and for `diag`.
#
# One cross-compile closes it. `x86_64-unknown-none` is already installed for the
# `no_std` check, so this costs a build and no new toolchain.
# **A positive control decides whether the target is usable**, because asking
# `rustup` does not: this toolchain is nix-provided and `rustup target list
# --installed` prints nothing while the cross-build works perfectly. The first
# version of this gate believed it and reported "not installed" on a machine
# that had just cross-compiled -- a probe that cannot fire, caught by perturbing
# it and getting the same message either way.
#
# So: build a crate known to be fine for that target. If the control fails the
# toolchain is the problem and this gate says so; if the control passes, a
# failure on `baud-isa-x86-64` is the crate.
if cargo build --quiet --release -p baud-isa --target x86_64-unknown-none 2>/dev/null; then
  if cargo build --quiet --release -p baud-isa-x86-64 --target x86_64-unknown-none 2>/dev/null; then
    echo "  ok   baud-isa-x86-64: cross-compiles"
  else
    echo "  FAIL baud-isa-x86-64: does not compile for x86_64 -- the trait moved and this impl did not"
    fail=1
  fi
else
  echo "  FAIL x86_64-unknown-none: the control crate does not build, so this gate checked nothing"
  fail=1
fi

files=$(find baud-isa/src baud-isa-x86-64/src baud-isa-aarch64/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-isa*/src: no sources found -- this gate audited nothing"
  fail=1
fi
for f in $files; do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  isa=$((isa + n))
  isasig=$((isasig + m))
  [ "$n" -gt 0 ] && echo "  ..   $f: $n + $m sig"
done
if [ "$isa" -gt 1 ] || [ "$isasig" -gt 0 ]; then
  echo "  FAIL baud-isa*: $isa raw-integer sites, $isasig signature (limit 1 and 0)"
  fail=1
else
  echo "  ok   baud-isa*: $isa and $isasig (limit 1 and 0, the movemask cast)"
fi

# The new core, on its own budget. **Two, and every one enumerated**:
#
#   1  `GRID = MAX_ROWS * MAX_COLS`, an array dimension. Rust requires an
#      array length to be a `usize` const expression and no newtype can be one
#      -- forced by the language, not chosen.
#   1  `n as usize` in `Nodes::at`, where a graph node becomes an array index.
#      A `repr(u8)` enum indexing its own table is the one cast the language
#      forces, and it is the single point a node enters index space.
#
# **Signatures are at zero and the limit follows the count**, which is this
# repo's own rule: a budget that is not equal to the count measures nothing.
# It got there when the one-line loop was typed -- `at`/`len`/`left`/`k` came
# in as bare `usize` and left as `BytePos`, `ReadLen`, `ByteLen` and `Count`,
# and `build(latin1: bool)` became `build(enc: InputEncoding)`, which is the
# two-state enum a `bool` parameter always is.
#
# The budget is a floor to drive down, not an allowance to spend. It went 14
# to 8 to 4 to 3 to 2 by moving byte-to-bits into `StreamByte`, the stride
# maths into `RowBlock` and the offsets onto `ByteLen` -- none of which was
# re-scoping.
new=0
newsig=0
files=$(find baud-core/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-core/src: no sources found -- this gate audited nothing"
  fail=1
fi
for f in $(find baud-core/src -name '*.rs' 2>/dev/null | sort); do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  new=$((new + n))
  newsig=$((newsig + m))
  [ $((n + m)) -gt 0 ] && echo "  ..   $f: $n + ${m} sig"
done
if [ "$new" -gt 2 ] || [ "$newsig" -gt 0 ]; then
  echo "  FAIL baud-core: $new raw-integer sites, $newsig signature (limit 2 and 0)"
  fail=1
else
  echo "  ok   baud-core: $new and $newsig (limit 2 and 0, every site enumerated)"
fi

# **The renderer crates, each on its own budget from the day it is created.**
# A crate the audit does not name is audited by nothing, which looks exactly
# like passing -- the shape this file records for the flat test glob and
# `docs.rs`'s `ROOTS`. So the four crates the rendering plan adds are gated
# here in the same commit that creates them, at zero, and each phase that adds
# code sets the limit to the count it ends at, never above it.
crate_audit() {
  ca_name="$1"
  ca_limit="$2"
  ca_siglimit="$3"
  ca_root="$4"
  ca_total=0
  ca_sig=0
  ca_files=$(find "$ca_root" -name '*.rs' 2>/dev/null | sort)
  if [ -z "$ca_files" ]; then
    echo "  FAIL $ca_name: no sources found -- this gate audited nothing"
    fail=1
    return
  fi
  for ca_f in $ca_files; do
    ca_out=$(python3 impl_audit.py "$ca_f")
    ca_n=$(echo "$ca_out" | grep '^  total' | awk '{print $2}')
    ca_m=$(echo "$ca_out" | grep '^  signature' | awk '{print $2}')
    ca_total=$((ca_total + ca_n))
    ca_sig=$((ca_sig + ca_m))
    [ $((ca_n + ca_m)) -gt 0 ] && echo "  ..   $ca_f: $ca_n + $ca_m sig"
  done
  if [ "$ca_total" -gt "$ca_limit" ] || [ "$ca_sig" -gt "$ca_siglimit" ]; then
    echo "  FAIL $ca_name: $ca_total raw-integer sites, $ca_sig signature (limit $ca_limit and $ca_siglimit)"
    fail=1
  else
    echo "  ok   $ca_name: $ca_total and $ca_sig (limit $ca_limit and $ca_siglimit)"
  fi
}
# **baud-font: twelve raw-integer sites and sixty signature primitives,
# every one enumerated, and the limit follows the count downward.** The
# signature sites are the newtype surface of a parser over a byte format --
# the same exemption `baud-domain` has for the same reason: `Len::of(u32)`,
# `FaceIndex::of(u32)`, `Palette::of(u16)`, `PaletteEntry::of(u16)`, the
# `value()` accessors (`Ppem`, `PaletteEntry` among them), `Offset::index ->
# usize` (the one bridge into slice indexing), `is_sfnt(u32)` on the
# directory's magic, `glyph(u16)` where a `cmap` value becomes a `GlyphId`,
# the `parse(u16)` constructors that turn a `CBLC` index or image format
# number into its enum, and in the outline readers the bytes a charstring
# and a DICT are made of: `OffSize::parse(u8)`, `dict_number`'s first byte,
# `sid_to_gid(u16)` on a string id, `usize_of -> u32` on an offset operand,
# `subr_index`'s bias as `i32`, `f2dot14(i16) -> f32`, the composite depth
# as `u8` against a cap of 32, and `Delta::of(bool, bool)` built from two
# flag bits. A DICT operator is a `DictOp`, a call depth a `Depth`, and the
# path operators CFF and CFF2 share take their operands as a slice, so no
# width slot is passed as a `usize`. The PNG decoder
# is the third interpreter and carries twenty of the sixty: `crc32`
# and `adler32` over bytes to a `u32`, `paeth(u8, u8, u8) -> u8` on bytes,
# `sample`/`scale` taking a bit depth and a sample, `BitReader::take(u32) ->
# u32` on a bit count, `Out::push(u8)`, `Color::parse(u8)` on the IHDR byte,
# `raw_len`/`rgba_len -> usize` sizing a caller's buffers, and `small(u32)
# -> usize` on a Huffman field. What the decoder does not carry is a swappable
# pair: the back-reference's `Run` and `Back` are two types because they are
# the same width. The rasteriser carries eight more, all pixel arithmetic:
# `Px::of(u32)` and `Px::value`, `Scale::of` taking pixels per em as `f32`,
# `Scale::placing`'s two pixel offsets, `Point::new`'s two coordinates,
# `Point::lerp`'s parameter, `distance_to`'s length, `row(f32) -> usize` --
# the one place a pixel coordinate becomes a row index, spelled once -- and
# `splat`'s row start as `usize`, which is also the twelfth raw site: the
# reference's own index arithmetic, kept in its shape so the byte-exact
# differential has one algorithm to compare rather than two. The shaper adds
# three signature sites and no raw ones: `same(GlyphId, u16)` comparing a
# glyph to the value a layout table stores, `LookupFlags::has(u16)` testing
# one flag bit, and `Count::slot -> usize`, the second bridge into slice
# indexing beside `Offset::index`. Buffer positions are `Index`, lengths
# are `Count`, table glyph ids are `GlyphId`, classes are `Class`, lookup
# types are `Kind` and flags are `LookupFlags`, so a position and a count
# cannot be added and a class and a glyph cannot be confused. The other
# eleven raw sites are the interpreters' state: the charstring stack length
# and the stem count that sizes `hintmask` (CFF's; CFF2 keeps its stems as a
# `Count` and shares the stack), the composite recursion depth (twice) and
# the contour count in `glyf`, and in the decoder
# the bit depth, the IDAT read position, the bit accumulator and its count,
# the output length, and the one `as usize` that indexes the CRC table inside
# a `const fn`, where `try_from` cannot be called. Every `bool` field became a
# two-state enum before this count was taken (`Path`, `Moveto`, `Width`,
# `End`, `Axis`, `Delta`, `Sign`, `IndexFormat`, `ImageFormat`, and CFF2's
# `Variation` and `Regions`).
crate_audit "baud-font" 12 60 baud-font/src
# baud-render: 0 raw sites and 11 signature primitives, every one a unit
# type's own edge -- `Tally::of(usize)` and `Tally::value -> u64`,
# `Digest::fold(u64)` and `Digest::value -> u64`, `slots -> usize` (the one
# place a pixel count becomes a slice index), `screen(i32)` (the one place
# pixel arithmetic becomes an instance coordinate), `Side::value -> u32`,
# `Key::fold(u32)`, `FaceIx::index -> usize`, and the two colour helpers
# `triple(u8, u8, u8)` and `grey(u8)` that build an `Argb` from wire bytes.
crate_audit "baud-render" 0 11 baud-render/src
# baud-present-macos: one signature primitive, `slots(Px) -> usize`, where a
# pixel count becomes a slice index. baud-app shares that edge, and its own
# line below enumerates the rest of its count.
crate_audit "baud-present-macos" 0 1 baud-present-macos/src
# baud-wayland: seven raw sites and seven signature primitives, and the
# floor is the kernel's socket ABI -- `sys.rs` declares `sendmsg`/`recvmsg`
# and their `msghdr`/`cmsghdr` as the platform lays them out (`i32` flags,
# and a length whose width is the platform's, `Len`, crossed by
# `ControlBytes` in one place), the way the ISA crates declare
# `sysctlbyname` -- plus the wire's own widths: a header's `usize` size, the
# byte cursor `put`/`word` carry, and the 24.8 fixed-point's saturating cast.
# The protocol values (`ObjectId(u32)`, `Opcode(u16)`, `Fixed(i32)`) are
# newtypes. It was eleven and eight until the lengths took a type; set at
# the count, and it does not move to fit a change.
crate_audit "baud-wayland" 7 7 baud-wayland/src
# baud-xkb: 10 raw sites and 6 signature primitives, and they are the keymap's
# own numbers -- keycode ranges, level and group indices, the 32-bit modifier
# mask the Wayland protocol serialises (a type alias, since it is the wire's
# encoding and not an index) -- plus the byte cursor the lexer carries. The
# keysym and the keycode are newtypes. Set at the count.
crate_audit "baud-xkb" 10 6 baud-xkb/src
# baud-present-wayland: 6 raw sites and 14 signature primitives, the kernel's
# mapping ABI in `sys.rs` (`mmap`, `munmap`, `ftruncate`, `memfd_create`, as
# the externs they are, and the mapping's `len`) and the pool's byte
# arithmetic -- a stride in words on the pool and on `present`, an offset into
# the file -- which the wire carries as `int`; plus the keyboard's `group`
# field and `bind`'s wire `name` and `version`, which sit one to a line and so
# read to the audit as fields. Set at the count. (This comment said 4 from the
# commit that set the limit at 6, 20ab159.)
crate_audit "baud-present-wayland" 6 14 baud-present-wayland/src
# baud-app: 1 raw site and 4 signature primitives. One is `slots(Px) ->
# usize` in the presenter edge, as above; the other four are the kernel ABI
# in `sys.rs`, which declares the pty, the ioctl, the poll, the mapping and
# Darwin's raw clock as the externs they are rather than through a `libc`
# crate -- the way `baud-present-wayland` declares `mmap` and the ISA crates
# declare `sysctlbyname`. The three parameters are `size_t` byte counts on
# `read`, `write` and `mmap`, and the field is the window size's own `u16`:
# the platform's widths, which a newtype would misrepresent rather than
# document. Set at the count, and it does not move to fit a change.
crate_audit "baud-app" 1 4 baud-app/src

# baud-bench is an aggregate for the same reason baud-core is: after the SRP
# split its 12 permitted sites live across main.rs, harness.rs and drive.rs, and
# per-file limits would have to be re-guessed every time a function moves.
#
# Why 12 and not 4: impl_audit.py anchors `^\s+\w+: (usize|bool|...),` to a
# line, meaning to catch struct fields; rustfmt breaks a long fn signature onto
# one parameter per line, and `total: usize,` as a parameter is then
# indistinguishable from a field. The extra hits are all parameters of
# run_once/run_once_t that were already there, hand-wrapped onto shared lines
# where the regex could not see them. The limit was raised rather than the
# formatter disabled, but note the cost: 12 permits more genuinely untyped
# sites than 4 did. The real fix is to teach impl_audit.py the difference
# between a field and a parameter.
# **Two trees since the measurement programs became `[[bench]]` targets.**
# They were `baud-bench/src/bin/*.rs` and are `baud-bench/benches/*.rs`, so one
# `find` over `src` went from auditing eleven raw sites to auditing none while
# still reporting "ok (limit 14)" -- a budget four times its count, which this
# file's own rule calls not measuring anything. Each tree carries its own, and
# each is set at the count: `src` is the shared lib alone, `benches` every
# bench program (nine as of `reshape`; the count is not written here so it
# cannot go stale again). Both are floors and neither moves to fit a change.
audit_tree() {
  label=$1
  dir=$2
  limit=$3
  tot=0
  files=$(find "$dir" -name '*.rs' 2>/dev/null | sort)
  if [ -z "$files" ]; then
    echo "  FAIL $label: no sources found -- this gate audited nothing"
    fail=1
    return
  fi
  for f in $files; do
    n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
    tot=$((tot + n))
    [ "$n" -gt 0 ] && echo "  ..   $f: $n"
  done
  if [ "$tot" -gt "$limit" ]; then
    echo "  FAIL $label: $tot raw-integer sites (limit $limit)"
    fail=1
  else
    echo "  ok   $label: $tot (limit $limit)"
  fi
}
audit_tree "baud-bench/src" baud-bench/src 0
audit_tree "baud-bench/benches" baud-bench/benches 11

# **Every other crate's `tests/` was audited by nothing.** `tree_audit` below
# covers `baud-core/tests` and the flat glob after it covers that crate's loose
# files; the other eleven test trees were reached by neither, carrying **100**
# raw-integer sites between them -- 43 in `baud-domain/tests`, 49 in
# `baud-font/tests`, 6 in `baud-render/tests` and one each in
# `baud-present-macos` and `baud-xkb`. That is the same scoped-to-what-it-was-
# written-beside failure as `docs.rs`'s `ROOTS` and `check-features.sh`'s crate
# list, a third time: the gate was green and auditing a fraction of the tree.
#
# Each is set at its count and every one is a floor. The six that read zero are
# listed rather than omitted -- a tree absent from this list is a tree nothing
# checks, which is exactly how the eleven got here, and `audit_tree` fails when
# a named directory holds no sources so a rename cannot silently empty one.
audit_tree "baud-domain/tests" baud-domain/tests 43
audit_tree "baud-font/tests" baud-font/tests 48
audit_tree "baud-render/tests" baud-render/tests 6
audit_tree "baud-present-macos/tests" baud-present-macos/tests 1
audit_tree "baud-xkb/tests" baud-xkb/tests 1
audit_tree "baud-bench/tests" baud-bench/tests 0
audit_tree "baud-isa/tests" baud-isa/tests 0
audit_tree "baud-isa-aarch64/tests" baud-isa-aarch64/tests 0
audit_tree "baud-isa-x86-64/tests" baud-isa-x86-64/tests 0
audit_tree "baud-wayland/tests" baud-wayland/tests 0
audit_tree "baud-present-wayland/tests" baud-present-wayland/tests 0

# **The glob is flat, and most of the test tree is behind it.**
# `baud-core/tests/*.rs` matches the loose files and **none** of the suites in
# `conformance/`, `vt100/`, `xterm/` and the rest, which were audited by nothing
# for as long as those directories existed. The limit-0 discipline below is real
# and it covers only the files the glob reaches, so the trees need their own
# gate.
#
# That is the same shape as `docs.rs`'s `ROOTS` list, which named every crate
# but the newer core: a gate scoped to less than it appears to cover, which
# looks exactly like passing.
#
# The tree is gated below at its count rather than at zero, because a limit set
# to a number nobody has driven to is a limit that fails on the next commit for
# a reason unrelated to it. **It is a floor**: the loose files reached 0 and
# this should.
tree_audit() {
  ta_name="$1"
  ta_limit="$2"
  ta_root="$3"
  ta_total=0
  ta_files=$(find "$ta_root" -mindepth 2 -name '*.rs' 2>/dev/null | sort)
  if [ -z "$ta_files" ]; then
    echo "  FAIL $ta_name: no sources found -- this gate audited nothing"
    fail=1
    return
  fi
  for ta_f in $ta_files; do
    ta_total=$((ta_total + $(python3 impl_audit.py "$ta_f" | tail -1 | awk '{print $2}')))
  done
  if [ "$ta_total" -gt "$ta_limit" ]; then
    echo "  FAIL $ta_name: $ta_total raw-integer sites (limit $ta_limit)"
    fail=1
  else
    echo "  ok   $ta_name: $ta_total (limit $ta_limit, a floor to drive down)"
  fi
}
# 35, up one for `spec_inventory`'s `images_refused: usize` -- the Sixel counter
# it checks so a `DCS q` registers as handled-not-silent, the same shape as the
# `unsupported`/`bells` counter fields beside it. A floor to drive down.
tree_audit "baud-core/tests suites" 35 baud-core/tests

for f in baud-core/tests/*.rs baud-difftest/src/lib.rs baud-difftest/tests/alacritty.rs baud-sfte-difftest/tests/sfte.rs baud-sfte-difftest/src/lib.rs baud-sfte-difftest/src/ffi.rs; do
  # **A gate that cannot find its input must fail, not pass.** An unmatched
  # glob passes through literally in bash, so a renamed or removed directory
  # would have this loop run `impl_audit.py` on the pattern itself, `n` would
  # come back empty, `[ "$n" -gt 0 ]` would error, and the `else` branch would
  # print `ok` -- a gate reporting success having audited nothing. Same family
  # as `inf x amplification`, the stale `scan path: AVX2` and `fast_hit`, and
  # it is what caught the commit that removed `baud-core2`.
  if [ ! -f "$f" ]; then
    echo "  FAIL $f: no such file -- this gate audited nothing"
    fail=1
    continue
  fi
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  # **Per file, and `baud-core`'s three are not zero.** `text.rs` 5,
  # `properties.rs` 4 and `fixed.rs` 1 -- written for this core rather than
  # ported, and never audited because the glob named the other one. Each is
  # pinned at what it is so it cannot grow, which is the same contract the
  # fifteen zeroes carry.
  case "$f" in
  baud-core/tests/text.rs) limit=5 ;;
  baud-core/tests/properties.rs) limit=4 ;;
  baud-core/tests/fixed.rs) limit=1 ;;
  *) limit=0 ;;
  esac
  if [ -z "$n" ]; then
    echo "  FAIL $f: the audit produced no count"
    fail=1
  elif [ "$n" -gt "$limit" ]; then
    echo "  FAIL $f: $n raw-integer sites (limit $limit)"
    fail=1
  else
    echo "  ok   $f: $n (limit $limit)"
  fi
done

# baud-domain is exempt from the count above -- a newtype's whole job is to wrap
# a primitive, so counting them there measures nothing. The check below globs
# the whole crate rather than naming lib.rs: after the SRP split lib.rs is ~40
# lines of `mod`/`pub use`, so a path-pinned grep would report zero for ever. But that exemption had a
# hole: it also hid primitives on the crate's *API surface*, and five `bool`
# fields accumulated inside protocol enums -- `Csi::Mode { set: bool }`,
# `Csi::SelectProtection { protect: bool }`, `Report::MouseSgr { release: bool }`
# and two more. Each was precisely the failure `Marker`'s own doc-comment argues
# against: "a `bool` argument at a call site says nothing".
#
# A `bool` *field* is never a newtype internal -- a newtype over bool is
# `struct X(bool)`, which this pattern does not match -- so zero is the right
# limit and there is no judgement call in it.
b=$(grep -rhE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/ | wc -l | tr -d " ")
if [ "$b" -gt 0 ]; then
  echo "  FAIL baud-domain/src: $b bool fields (limit 0) -- use a named two-state enum"
  grep -rnE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/
  fail=1
else
  echo "  ok   baud-domain/src: 0 bool fields (limit 0)"
fi

exit $fail
