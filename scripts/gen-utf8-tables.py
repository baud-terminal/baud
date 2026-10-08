#!/usr/bin/env python3
"""Generate the three 16-entry lookup tables of `[[source: utf8-lookup]]`.

The paper's Table 8 lists seven 2-byte error patterns and the algorithm ANDs
three nibble-indexed lookups: high nibble of byte 1, low nibble of byte 1, high
nibble of byte 2. A bit set in all three means that pair is invalid.

**Generated, not transcribed**, because a guessed table is unrecoverable and
looks like it worked. Here it is worse than cosmetic -- a wrong entry makes
the validator *accept* invalid UTF-8, and `E0 80 AF` decoding to `/` is a bug
this project has already had once.

So the patterns are written as predicates over a byte pair, the tables are
derived from them by construction, and then **every one of the 65,536 pairs is
checked** against an independent reference decoder. Transcription cannot
survive that; a typo in a predicate cannot either, unless it is a typo the
reference makes too, which is why the reference is written from the UTF-8
definition rather than from the patterns.
"""

from __future__ import annotations

import sys

# --- the reference, written from the definition of UTF-8 --------------------


def reference_bad_pair(b1: int, b2: int) -> bool:
    """Is `b2` an impossible successor of `b1`, judging the pair alone?

    Deliberately independent of the patterns below: it encodes the rules the
    way baud's scalar decoder does -- lead ranges, continuation ranges, the
    overlong floors and the surrogate hole -- so agreement between the two is
    evidence rather than tautology.
    """
    cont = 0x80 <= b2 <= 0xBF
    if b1 <= 0x7F:
        # ASCII must not be followed by a continuation: that continuation
        # belongs to no sequence. ("Too Long")
        return cont
    if 0x80 <= b1 <= 0xBF:
        # A continuation as the first byte is an error found elsewhere; the
        # pair itself is not what proves it. Bit 7 of the paper's table marks
        # this case "Not An Error".
        return False
    if b1 in (0xC0, 0xC1):
        return True  # overlong two-byte lead, invalid whatever follows
    if 0xC2 <= b1 <= 0xDF:
        return not cont
    if b1 == 0xE0:
        return not cont or b2 < 0xA0  # overlong three-byte
    if b1 == 0xED:
        return not cont or b2 > 0x9F  # surrogate
    if 0xE1 <= b1 <= 0xEF:
        return not cont
    if b1 == 0xF0:
        return not cont or b2 < 0x90  # overlong four-byte
    if b1 == 0xF4:
        return not cont or b2 > 0x8F  # above U+10FFFF
    if 0xF1 <= b1 <= 0xF3:
        return not cont
    return True  # F5..FF: no lead byte exists here


# --- the paper's seven patterns, as predicates on nibbles -------------------
#
# Each must be a conjunction of a constraint on hi(b1), one on lo(b1) and one
# on hi(b2); that is what makes three independent lookups AND together
# correctly, and it is the property the exhaustive check would catch a
# violation of.

PATTERNS = [
    # bit, name, hi(b1), lo(b1), hi(b2)
    (0, "too long (ASCII + continuation)",
     lambda h: h <= 0x7, lambda l: True, lambda h: h in (0x8, 0x9, 0xA, 0xB)),
    (1, "too short (lead not followed by a continuation)",
     lambda h: h in (0xC, 0xD, 0xE, 0xF), lambda l: True,
     lambda h: not (h in (0x8, 0x9, 0xA, 0xB))),
    (2, "overlong, two-byte",
     lambda h: h == 0xC, lambda l: l <= 0x1, lambda h: h in (0x8, 0x9, 0xA, 0xB)),
    (3, "surrogate",
     lambda h: h == 0xE, lambda l: l == 0xD, lambda h: h in (0xA, 0xB)),
    (4, "overlong, three-byte",
     lambda h: h == 0xE, lambda l: l == 0x0, lambda h: h in (0x8, 0x9)),
    # Table 8 puts two errors on bit 5, and the pairing is not arbitrary: both
    # are the cases where hi(b2) is 0x8. `F0 8x` is the overlong four-byte
    # form; `F5..FF 8x` is past U+10FFFF. So lo(b1) is `0` *or* `5..F`, which
    # is a set rather than a range -- expressible because the table has an
    # entry per nibble.
    (5, "overlong four-byte, and too large with hi(b2) = 8",
     lambda h: h == 0xF, lambda l: l == 0x0 or l >= 0x5, lambda h: h == 0x8),
    # **`hi(b2)` excludes 0x8 here**, and the exhaustive check is what found
    # that: `F4 80`..`F4 8F` is U+100000..U+10FFFF and perfectly valid. Row 6
    # reads `10|01...11`, so the continuation's high nibble is 9, A or B.
    (6, "too large, above U+10FFFF",
     lambda h: h == 0xF, lambda l: l >= 0x4, lambda h: h in (0x9, 0xA, 0xB)),
    # **Bit 7 is not an error.** It marks a pair that is two continuations, and
    # § 6.2 needs exactly that to count the long forms. Table 8's last row says
    # so in its title, and a validator that treated bit 7 as an error would
    # reject every three- and four-byte character.
    (7, "two continuations (not an error)",
     lambda h: h in (0x8, 0x9, 0xA, 0xB), lambda l: True,
     lambda h: h in (0x8, 0x9, 0xA, 0xB)),
]


def build() -> tuple[list[int], list[int], list[int]]:
    hi1 = [0] * 16
    lo1 = [0] * 16
    hi2 = [0] * 16
    for bit, _name, p_hi1, p_lo1, p_hi2 in PATTERNS:
        for v in range(16):
            if p_hi1(v):
                hi1[v] |= 1 << bit
            if p_lo1(v):
                lo1[v] |= 1 << bit
            if p_hi2(v):
                hi2[v] |= 1 << bit
    return hi1, lo1, hi2


def lookup(hi1, lo1, hi2, b1: int, b2: int) -> int:
    """The algorithm itself: three lookups, ANDed."""
    return hi1[b1 >> 4] & lo1[b1 & 0xF] & hi2[b2 >> 4]


def classify(hi1, lo1, hi2, b1: int, b2: int) -> bool:
    """An error in bits 0-6. Bit 7 is masked off: it is a fact, not a fault."""
    return (lookup(hi1, lo1, hi2, b1, b2) & 0x7F) != 0


# --- the whole algorithm, including § 6.2's long-form rule -----------------


def validate(hi1, lo1, hi2, data: bytes) -> bool:
    """Is `data` well-formed UTF-8, by the paper's algorithm?

    Two parts. The nibble tables above reject anything visible in a byte
    *pair*. § 6.2 then catches the 3- and 4-byte forms with too many or too few
    continuations, which no pair can show: "we expect two continuations if the
    previous byte matches 111|, or if the byte before that matches 1111|", and
    that expectation is compared against where two continuations actually
    occur -- bit 7 of the same classification. "If these two lists differ in
    any respect, the UTF-8 is invalid."
    """
    n = len(data)
    for i in range(n):
        b1 = data[i - 1] if i > 0 else 0x00
        b2 = data[i]
        if classify(hi1, lo1, hi2, b1, b2):
            return False
        have_two = bool(lookup(hi1, lo1, hi2, b1, b2) & 0x80)
        # **"The previous byte" is previous to the *pair*, not to `i`.** The
        # pair judged at `i` is `(data[i-1], data[i])`, so the byte before it is
        # `data[i-2]` and the one before that `data[i-3]`. Reading "previous"
        # as `data[i-1]` rejects `E0 A0 80` -- U+0800, perfectly valid -- and
        # the exhaustive check against CPython is what caught it.
        prev = data[i - 2] if i >= 2 else 0x00
        prev2 = data[i - 3] if i >= 3 else 0x00
        want_two = prev >= 0xE0 or prev2 >= 0xF0
        if have_two != want_two:
            return False
    return True


def python_verdict(data: bytes) -> str:
    """`valid`, `truncated`, or `invalid`, from CPython's own decoder.

    An independent oracle, and a far better one than a reference written
    alongside the thing it checks. `truncated` is kept separate because a
    sequence cut by a read boundary is not malformed -- it is what
    `State::Utf8` exists to carry, and the block validator is not asked about
    it.
    """
    try:
        data.decode("utf-8")
        return "valid"
    except UnicodeDecodeError as e:
        return "truncated" if "unexpected end of data" in e.reason else "invalid"


def ends_open(data: bytes) -> bool:
    """Does `data` stop in the middle of a sequence?

    **The block validator is not responsible for these and must not be judged
    on them.** Its tables classify byte *pairs*, so a lead byte at the very end
    has no successor to be judged against: a trailing `C0` is not yet known to
    be an overlong, it is a sequence still open. In baud that is not a gap but
    a division of labour -- `read(2)` splits wherever it likes, `State::Utf8`
    carries the open sequence to the next chunk, and `fast_utf8` declines
    exactly this case. CPython has no such notion and calls a lone `C0` an
    invalid start byte, which is true of the whole string and not of the pair.
    """
    n = len(data)
    return (
        (n >= 1 and data[-1] >= 0xC0)
        or (n >= 2 and data[-2] >= 0xE0)
        or (n >= 3 and data[-3] >= 0xF0)
    )


def check_strings(hi1, lo1, hi2, cases) -> tuple[list, int]:
    bad, skipped = [], 0
    for data in cases:
        if ends_open(data):
            skipped += 1
            continue
        verdict = python_verdict(data)
        got = validate(hi1, lo1, hi2, data)
        if (verdict == "invalid" and got) or (verdict == "valid" and not got):
            bad.append((data, verdict, got))
    return bad, skipped


BOUNDARIES = [
    0x00, 0x1F, 0x20, 0x41, 0x7E, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF,
    0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xEC, 0xED, 0xEE, 0xEF, 0xF0, 0xF1,
    0xF3, 0xF4, 0xF5, 0xFE, 0xFF,
]


def main() -> int:
    hi1, lo1, hi2 = build()
    bad = []
    for b1 in range(256):
        for b2 in range(256):
            want = reference_bad_pair(b1, b2)
            got = classify(hi1, lo1, hi2, b1, b2)
            if want != got:
                bad.append((b1, b2, want, got))
    if bad:
        print(f"  FAIL {len(bad)} of 65536 pairs disagree with the reference")
        for b1, b2, want, got in bad[:20]:
            print(f"    {b1:#04x} {b2:#04x}: reference says "
                  f"{'invalid' if want else 'valid'}, tables say "
                  f"{'invalid' if got else 'valid'}")
        return 1
    print("  ok   all 65536 byte pairs agree with the reference decoder")

    # Whole strings, against CPython's decoder. One and two bytes exhaustively;
    # three and four over every boundary value, which is where the lead ranges,
    # the overlong floors and the surrogate hole meet.
    exhaustive = "--exhaustive" in sys.argv
    ones = [bytes([a]) for a in range(256)]
    twos = [bytes([a, b]) for a in range(256) for b in range(256)]
    # Boundary values by default because all 16.7M three-byte strings take
    # minutes; `--exhaustive` runs every one of them and is what the committed
    # tables were actually checked against.
    r3 = range(256) if exhaustive else BOUNDARIES
    threes = [bytes([a, b, c]) for a in r3 for b in r3 for c in r3]
    fours = [
        bytes([a, b, c, d])
        for a in BOUNDARIES for b in BOUNDARIES
        for c in BOUNDARIES for d in BOUNDARIES
    ]
    for name, cases in (("1-byte", ones), ("2-byte", twos),
                        ("3-byte", threes), ("4-byte", fours)):
        bad, skipped = check_strings(hi1, lo1, hi2, cases)
        if bad:
            print(f"  FAIL {len(bad)} of {len(cases)} {name} strings disagree "
                  f"with CPython")
            for data, verdict, got in bad[:10]:
                print(f"    {data.hex(' ')}: python says {verdict}, "
                      f"algorithm says {'valid' if got else 'invalid'}")
            return 1
        print(f"  ok   {len(cases) - skipped} {name} strings agree with "
              f"CPython ({skipped} end mid-sequence, State::Utf8's job)")

    for name, t in (("HI1", hi1), ("LO1", lo1), ("HI2", hi2)):
        body = ", ".join(f"0x{v:02x}" for v in t)
        print(f"  const {name}: [u8; 16] = [{body}];")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
