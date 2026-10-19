//! `rgbi:` and the six device-independent colour specifications of the
//! Xlib manual's "Color Strings" (`[[source: xlib-ch06-b8d5a70]]` 637-695):
//! `CIEXYZ:`, `CIEuvY:`, `CIExyY:`, `CIELab:`, `CIELuv:` and `TekHVC:`, each
//! converted to the 8-bit sRGB value baud stores.
//!
//! **The device is baud's surface: sRGB.** The manual converts through "the
//! device profile" of the screen; libX11, finding none on the root window,
//! falls back to a 1990 Tektronix CRT whose white is far bluer than D65
//! (`[[source: libx11-lrgb-b8d5a70]]` 214-440). baud's surface is sRGB, so
//! the profile is sRGB's: the matrices and D65 white of
//! `[[source: css-color-4-20261009]]` 10.2, and for `rgbi:`, whose values are
//! "linear intensity values ... not gamma corrected", the sRGB transfer
//! curve. Jev decided it at 0.94 and 0.92 (decisions 810/811).
//!
//! **CIELab and CIELuv are CIE's** (`[[source: cie-015-2018]]` 8.2.1, 8.2.2
//! and Annex C), the definitions the manual names. libX11 reads a*, b*, u*
//! and v* at one hundredth of CIE's scale and gives L* below 8 a Y a hundred
//! times too large, so its `CIELab:7/0/0` is near white; that is its
//! departure. Jev decided it at 0.92 and 0.92 (decisions 810/811).
//!
//! **TekHVC, the conversions between the CIE forms and the gamut clip are
//! Xcms's**, ported from libX11 b8d5a70's `src/xcms`:
//! `[[source: libx11-xcms-uvy-b8d5a70]]`, `[[source: libx11-xcms-xyy-b8d5a70]]`,
//! `[[source: libx11-xcms-xyz-b8d5a70]]`, `[[source: libx11-xcms-hvc-b8d5a70]]`,
//! the clip `XcmsTekHVCClipC` (`[[source: libx11-xcms-hvcgcc-b8d5a70]]`),
//! the default compression (`[[source: libx11-xcms-cmsint-b8d5a70]]`), the
//! searches it calls (`[[source: libx11-xcms-hvcmxc-b8d5a70]]`,
//! `[[source: libx11-xcms-hvcmxvc-b8d5a70]]`), the order of conversion
//! (`[[source: libx11-xcms-cvcols-b8d5a70]]`), and Xcms's own roots and
//! trigonometry (`[[source: libx11-xcms-cmsmath-b8d5a70]]`,
//! `[[source: libx11-xcms-cmstrig-b8d5a70]]`), whose numbers TekHVC is
//! defined by. A colour out of gamut keeps its TekHVC hue and value and
//! loses chroma, as Xcms's default compression does; one still out of gamut
//! after it is refused. One step departs from libX11's arithmetic, in
//! `uvy_to_hvc`: a hue a rounding error from the u' axis.
//! Under libX11's own CRT profile the port draws what libX11 draws on a
//! 24-bit Xvfb for 726 of 768 specs, the rest a channel at the gamut's edge
//! or `rgbi:` past 1 (`clip_matches_libx11_under_its_own_profile`).
//!
//! The ported lines keep their notices: Tektronix's TekHVC notice for
//! the TekHVC conversions, the clip and its searches, Tektronix's Xcms
//! notice for the other conversions, the trigonometry and the CRT data,
//! and The Open Group's for `cube_root` and `square_root`. They follow as
//! the files carry them, and `LICENSES/libx11-xcms.txt` holds them with
//! the list of what falls under each
//! (`[[source: libx11-xcms-lab-b8d5a70]]` and
//! `[[source: libx11-xcms-luv-b8d5a70]]` were read and not ported).

// Code and supporting documentation (c) Copyright 1990 1991 Tektronix, Inc.
// 	All Rights Reserved
//
// This file is a component of an X Window System-specific implementation
// of Xcms based on the TekColor Color Management System.  TekColor is a
// trademark of Tektronix, Inc.  The term "TekHVC" designates a particular
// color space that is the subject of U.S. Patent No. 4,985,853 (equivalent
// foreign patents pending).  Permission is hereby granted to use, copy,
// modify, sell, and otherwise distribute this software and its
// documentation for any purpose and without fee, provided that:
//
// 1. This copyright, permission, and disclaimer notice is reproduced in
//    all copies of this software and any modification thereof and in
//    supporting documentation;
// 2. Any color-handling application which displays TekHVC color
//    cooordinates identifies these as TekHVC color coordinates in any
//    interface that displays these coordinates and in any associated
//    documentation;
// 3. The term "TekHVC" is always used, and is only used, in association
//    with the mathematical derivations of the TekHVC Color Space,
//    including those provided in this file and any equivalent pathways and
//    mathematical derivations, regardless of digital (e.g., floating point
//    or integer) representation.
//
// Tektronix makes no representation about the suitability of this software
// for any purpose.  It is provided "as is" and with all faults.
//
// TEKTRONIX DISCLAIMS ALL WARRANTIES APPLICABLE TO THIS SOFTWARE,
// INCLUDING THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
// PARTICULAR PURPOSE.  IN NO EVENT SHALL TEKTRONIX BE LIABLE FOR ANY
// SPECIAL, INDIRECT OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER
// RESULTING FROM LOSS OF USE, DATA, OR PROFITS, WHETHER IN AN ACTION OF
// CONTRACT, NEGLIGENCE, OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR THE PERFORMANCE OF THIS SOFTWARE.
//
// ----------------------------------------------------------------------
//
// Code and supporting documentation (c) Copyright 1990 1991 Tektronix, Inc.
// 	All Rights Reserved
//
// This file is a component of an X Window System-specific implementation
// of Xcms based on the TekColor Color Management System.  Permission is
// hereby granted to use, copy, modify, sell, and otherwise distribute this
// software and its documentation for any purpose and without fee, provided
// that this copyright, permission, and disclaimer notice is reproduced in
// all copies of this software and in supporting documentation.  TekColor
// is a trademark of Tektronix, Inc.
//
// Tektronix makes no representation about the suitability of this software
// for any purpose.  It is provided "as is" and with all faults.
//
// TEKTRONIX DISCLAIMS ALL WARRANTIES APPLICABLE TO THIS SOFTWARE,
// INCLUDING THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
// PARTICULAR PURPOSE.  IN NO EVENT SHALL TEKTRONIX BE LIABLE FOR ANY
// SPECIAL, INDIRECT OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER
// RESULTING FROM LOSS OF USE, DATA, OR PROFITS, WHETHER IN AN ACTION OF
// CONTRACT, NEGLIGENCE, OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR THE PERFORMANCE OF THIS SOFTWARE.
//
// ----------------------------------------------------------------------
//
// Copyright 1990, 1998  The Open Group
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE
// OPEN GROUP BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
// AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Except as contained in this notice, the name of The Open Group shall not be
// used in advertising or otherwise to promote the sale, use or other dealings
// in this Software without prior written authorization from The Open Group.

use core::cmp::Ordering;

/// Xcms's tolerance on a spec's range (`XMY_DBL_EPSILON`, which these files
/// define as 0.00001 when `<float.h>` is not included, as here: libX11
/// takes `CIELab:100.000005/0/0` and refuses `CIELab:100.00002/0/0`).
const XMY_EPS: f64 = 0.00001;
/// The gamut and search tolerance of `LRGB.c` and `HVCMxC.c`.
const EPS: f64 = 0.001;

/// A colour-space prefix, read in any case: "Color strings are
/// case-insensitive".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Space {
    RgbI,
    CieXyz,
    CieUvY,
    CieXyY,
    CieLab,
    CieLuv,
    TekHvc,
}

impl Space {
    /// The space a prefix names, by its length and its lower-cased bytes.
    pub(crate) fn of(prefix: &[u8]) -> Option<Space> {
        let n = prefix.len();
        if !(4..=6).contains(&n) {
            return None;
        }
        let mut low = [0u8; 6];
        for (d, s) in low.iter_mut().zip(prefix) {
            *d = s.to_ascii_lowercase();
        }
        match &low[..n] {
            b"rgbi" => Some(Space::RgbI),
            b"ciexyz" => Some(Space::CieXyz),
            b"cieuvy" => Some(Space::CieUvY),
            b"ciexyy" => Some(Space::CieXyY),
            b"cielab" => Some(Space::CieLab),
            b"cieluv" => Some(Space::CieLuv),
            b"tekhvc" => Some(Space::TekHvc),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Xyz {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct UvY {
    u: f64,
    v: f64,
    y: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Hvc {
    h: f64,
    v: f64,
    c: f64,
}

type Mat = [[f64; 3]; 3];

/// A screen colour characterisation: linear RGB to XYZ and back, and the
/// encoding of the three linear intensities as the 16-bit device values
/// whose high bytes are drawn.
struct Profile {
    to_xyz: Mat,
    from_xyz: Mat,
    encode: fn([f64; 3]) -> [u16; 3],
}

/// sRGB, `[[source: css-color-4-20261009]]` 10.2: `lin_sRGB_to_XYZ` and
/// `XYZ_to_lin_sRGB` as the specification prints them, D65 its white.
const SRGB: Profile = Profile {
    to_xyz: [
        [506752.0 / 1228815.0, 87881.0 / 245763.0, 12673.0 / 70218.0],
        [87098.0 / 409605.0, 175762.0 / 245763.0, 12673.0 / 175545.0],
        [7918.0 / 409605.0, 87881.0 / 737289.0, 1001167.0 / 1053270.0],
    ],
    from_xyz: [
        [12831.0 / 3959.0, -329.0 / 214.0, -1974.0 / 3959.0],
        [
            -851781.0 / 878810.0,
            1648619.0 / 878810.0,
            36519.0 / 878810.0,
        ],
        [705.0 / 12673.0, -2585.0 / 12673.0, 705.0 / 667.0],
    ],
    encode: |i| i.map(srgb_encode),
};

/// `gam_sRGB` for an intensity in [0, 1], at the nearest of the 256 levels
/// a 24-bit visual has, which is where Xcms's `_XcmsIntensityInterpolation`
/// puts its answer too. `i^(1/2.4)` is `i^(5/12)`, a cube root and two square
/// roots.
fn srgb_encode(i: f64) -> u16 {
    debug_assert!(!i.is_nan(), "a NaN intensity reached the device");
    let e = if i > 0.0031308 {
        let c = cube_root(i);
        1.055 * c * square_root(square_root(c)) - 0.055
    } else {
        12.92 * i
    };
    let level = core::f64::math::round(e.clamp(0.0, 1.0) * 255.0);
    // In [0, 255] by the clamp: exact as an integer.
    let level = u16::try_from(level as u32).unwrap_or(255);
    level << 8
}

fn mat_vec(m: &Mat, v: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (o, row) in out.iter_mut().zip(m) {
        for (a, b) in row.iter().zip(v) {
            *o += a * b;
        }
    }
    out
}

/// The profile with the white points and TekHVC's hue offset it implies.
struct Ctx {
    p: &'static Profile,
    white: Xyz,
    white_uvy: UvY,
    theta_offset: f64,
}

impl Ctx {
    /// The screen white is the sum of the RGB-to-XYZ rows, its Y set to 1
    /// (`LRGB.c`'s `LINEAR_RGB_InitSCCData`); TekHVC's hue is measured from
    /// the line through it and "best red", u' 0.7127, v' 0.4931 (`HVC.c`'s
    /// `ThetaOffset`).
    fn new(p: &'static Profile) -> Option<Ctx> {
        let m = &p.to_xyz;
        let white = Xyz {
            x: m[0][0] + m[0][1] + m[0][2],
            y: 1.0,
            z: m[2][0] + m[2][1] + m[2][2],
        };
        let white_uvy = UvY {
            y: 1.0,
            ..xyz_to_uvy(white, None)?
        };
        let div = U_BR - white_uvy.u;
        if div == 0.0 {
            return None;
        }
        let theta_offset = degrees(arc_tangent((V_BR - white_uvy.v) / div));
        Some(Ctx {
            p,
            white,
            white_uvy,
            theta_offset,
        })
    }
}

// Xcms's own arithmetic. TekHVC is defined by what these compute, and the
// clip searches on it, so they are ported rather than replaced.

/// `_XcmsCubeRoot`: Newton's method to a relative step of one epsilon.
fn cube_root(a: f64) -> f64 {
    if a == 0.0 {
        return 0.0;
    }
    let abs_a = a.abs();
    let mut g = if abs_a > 1.0 {
        abs_a / 8.0
    } else {
        abs_a * 8.0
    };
    loop {
        let delta = (g - abs_a / (g * g)) / 3.0;
        g -= delta;
        // Xcms's `while (delta >= cur_guess*DBL_EPSILON)`: a NaN, which
        // compares as nothing, ends the loop as it does there.
        if !matches!(
            delta.abs().partial_cmp(&(g * f64::EPSILON)),
            Some(Ordering::Greater | Ordering::Equal)
        ) {
            break;
        }
    }
    if a < 0.0 { -g } else { g }
}

/// `_XcmsSquareRoot`; zero for a negative argument.
fn square_root(a: f64) -> f64 {
    if a <= 0.0 {
        return 0.0;
    }
    let mut g = if a > 1.0 { a / 4.0 } else { a * 4.0 };
    loop {
        let delta = (g - a / g) / 2.0;
        g -= delta;
        if !matches!(
            delta.abs().partial_cmp(&(g * f64::EPSILON)),
            Some(Ordering::Greater | Ordering::Equal)
        ) {
            break;
        }
    }
    g
}

// Xcms writes these as decimal literals (`XCMS_PI` and the rest); each
// parses to the same double as `core`'s constant.
const PI: f64 = core::f64::consts::PI;
const TWOPI: f64 = core::f64::consts::TAU;
const HALFPI: f64 = core::f64::consts::FRAC_PI_2;
const FOURTHPI: f64 = core::f64::consts::FRAC_PI_4;
const X6_UNDERFLOWS: f64 = 4.209340e-52;
const MAXERROR: f64 = 0.000001;
const MAXITER: usize = 10000;
/// `XCMS_DMAXPOWTWO`: `0x7FFFFFFF * 2^(64 - 11 - 32 + 1)`, past which a
/// double has no fraction.
const DMAXPOWTWO: f64 = 2147483647.0 * 4194304.0;

#[allow(clippy::excessive_precision)] // as cmsTrig.c prints them
const COS_P: [f64; 4] = [
    0.12905394659037374438e7,
    -0.37456703915723204710e6,
    0.13432300986539084285e5,
    -0.11231450823340933092e3,
];
#[allow(clippy::excessive_precision)] // as cmsTrig.c prints them
const COS_Q: [f64; 4] = [
    0.12905394659037373590e7,
    0.23467773107245835052e5,
    0.20969518196726306286e3,
    1.0,
];
#[allow(clippy::excessive_precision)] // as cmsTrig.c prints them
const SIN_P: [f64; 4] = [
    0.20664343336995858240e7,
    -0.18160398797407332550e6,
    0.35999306949636188317e4,
    -0.20107483294588615719e2,
];
#[allow(clippy::excessive_precision)] // as cmsTrig.c prints them
const SIN_Q: [f64; 4] = [
    0.26310659102647698963e7,
    0.39270242774649000308e5,
    0.27811919481083844087e3,
    1.0,
];

fn polynomial(c: &[f64; 4], x: f64) -> f64 {
    c.iter().rev().skip(1).fold(c[3], |r, &k| k + x * r)
}

/// `_XcmsModulo(value, base)`: the fraction of `value / base`, times `base`.
fn modulo(value: f64, base: f64) -> f64 {
    let val = value / base;
    let abs = val.abs();
    let ip = if abs >= DMAXPOWTWO {
        val
    } else {
        let mut ip = abs + DMAXPOWTWO;
        ip -= DMAXPOWTWO;
        if ip > abs {
            ip -= 1.0;
        }
        ip.abs()
    };
    (val - ip) * base
}

fn reduce(mut x: f64) -> f64 {
    if !(-PI..=PI).contains(&x) {
        x = modulo(x, TWOPI);
        if x > PI {
            x -= TWOPI;
        } else if x < -PI {
            x += TWOPI;
        }
    }
    x
}

/// `_XcmsCosine`.
fn cosine(x: f64) -> f64 {
    let x = reduce(x);
    if x > HALFPI {
        -cosine(x - PI)
    } else if x < -HALFPI {
        -cosine(x + PI)
    } else if x > FOURTHPI {
        sine(HALFPI - x)
    } else if x < -FOURTHPI {
        sine(HALFPI + x)
    } else if x < X6_UNDERFLOWS && x > -X6_UNDERFLOWS {
        square_root(1.0 - x * x)
    } else {
        let y = x / FOURTHPI;
        let yt2 = y * y;
        polynomial(&COS_P, yt2) / polynomial(&COS_Q, yt2)
    }
}

/// `_XcmsSine`.
fn sine(x: f64) -> f64 {
    let x = reduce(x);
    if x > HALFPI {
        -sine(x - PI)
    } else if x < -HALFPI {
        -sine(x + PI)
    } else if x > FOURTHPI {
        cosine(HALFPI - x)
    } else if x < -FOURTHPI {
        -cosine(HALFPI + x)
    } else if x < X6_UNDERFLOWS && x > -X6_UNDERFLOWS {
        x
    } else {
        let y = x / FOURTHPI;
        let yt2 = y * y;
        y * (polynomial(&SIN_P, yt2) / polynomial(&SIN_Q, yt2))
    }
}

/// `_XcmsArcTangent`: the arithmetic-geometric mean, to a relative error of
/// 1e-6 (an absolute one for a negative argument, as Xcms has it).
fn arc_tangent(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    let maxerror = if x < 1.0 { x * MAXERROR } else { MAXERROR };
    let mut ai = square_root(1.0 / (1.0 + x * x));
    let mut bi = 1.0;
    let (mut a1, mut b1) = (0.0, 0.0);
    for _ in 0..MAXITER {
        a1 = (ai + bi) / 2.0;
        b1 = square_root(a1 * bi);
        if a1 == b1 || (a1 - b1).abs() < maxerror {
            break;
        }
        ai = a1;
        bi = b1;
    }
    let l = if a1 > b1 { b1 } else { a1 };
    x / (square_root(1.0 + x * x) * l)
}

fn degrees(r: f64) -> f64 {
    r * 180.0 / PI
}

fn radians(d: f64) -> f64 {
    d * PI / 180.0
}

/// A value as the manual spells it: "an optional plus or minus sign, a
/// string of digits possibly containing a decimal point, and an optional
/// exponent field consisting of an ``E'' or ``e'' followed by an optional
/// plus or minus followed by a string of digits". libX11 reads it with
/// `sscanf("%lf")`, which also takes leading blanks, `inf`, `nan` and hex
/// floats; baud takes the grammar and nothing else.
fn number(s: &[u8]) -> Option<f64> {
    let digits = |i: &mut usize| {
        let at = *i;
        while s.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - at
    };
    let mut i = usize::from(matches!(s.first(), Some(b'+' | b'-')));
    let mut n = digits(&mut i);
    if s.get(i) == Some(&b'.') {
        i += 1;
        n += digits(&mut i);
    }
    if n == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        i += 1;
        i += usize::from(matches!(s.get(i), Some(b'+' | b'-')));
        if digits(&mut i) == 0 {
            return None;
        }
    }
    if i != s.len() {
        return None;
    }
    let v: f64 = core::str::from_utf8(s).ok()?.parse().ok()?;
    v.is_finite().then_some(v)
}

/// `<a>/<b>/<c>`: three values and nothing after them. libX11's
/// `sscanf("%lf/%lf/%lf")` ignores what follows the third.
fn triple(fields: &[u8]) -> Option<[f64; 3]> {
    let mut it = fields.split(|&b| b == b'/');
    let out = [
        number(it.next()?)?,
        number(it.next()?)?,
        number(it.next()?)?,
    ];
    it.next().is_none().then_some(out)
}

fn in_range(v: f64, lo: f64, hi: f64) -> bool {
    v >= lo - XMY_EPS && v <= hi + XMY_EPS
}

const U_BR: f64 = 0.7127;
const V_BR: f64 = 0.4931;
const CHROMA_SCALE_FACTOR: f64 = 7.50725;

/// `_XcmsCIEXYZ_ValidSpec`: Y from 0 to 1.
fn xyz_valid(c: Xyz) -> bool {
    in_range(c.y, 0.0, 1.0)
}

/// `_XcmsCIEuvY_ValidSpec`: Y from 0 to 1.
fn uvy_valid(c: UvY) -> bool {
    in_range(c.y, 0.0, 1.0)
}

/// `XcmsCIEuvYToCIEXYZ`.
fn uvy_to_xyz(c: UvY, white: UvY) -> Option<Xyz> {
    if !uvy_valid(c) {
        return None;
    }
    let mut div = 6.0 * c.u - 16.0 * c.v + 12.0;
    let (x, y) = if div == 0.0 {
        div = 6.0 * white.u - 16.0 * white.v + 12.0;
        if div == 0.0 {
            return None;
        }
        (9.0 * white.u / div, 4.0 * white.v / div)
    } else {
        (9.0 * c.u / div, 4.0 * c.v / div)
    };
    let z = 1.0 - x - y;
    Some(if y != 0.0 {
        Xyz {
            x: x * c.y / y,
            y: c.y,
            z: z * c.y / y,
        }
    } else {
        Xyz { x, y: c.y, z }
    })
}

/// `XcmsCIEXYZToCIEuvY`; black takes the white's chromaticity.
fn xyz_to_uvy(c: Xyz, white: Option<UvY>) -> Option<UvY> {
    if !xyz_valid(c) {
        return None;
    }
    let div = c.x + 15.0 * c.y + 3.0 * c.z;
    if div == 0.0 {
        let w = white?;
        return Some(UvY {
            u: w.u,
            v: w.v,
            y: c.y,
        });
    }
    Some(UvY {
        u: 4.0 * c.x / div,
        v: 9.0 * c.y / div,
        y: c.y,
    })
}

/// `XcmsCIExyYToCIEXYZ`, by way of u'v' as Xcms goes. Its second divisor,
/// `6u' - 16v' + 12`, is `36 / (-2x + 12y + 3)` and never zero, so the
/// branch Xcms keeps for it is not.
fn xyy_to_xyz(x: f64, y: f64, yy: f64) -> Option<Xyz> {
    if !(in_range(x, 0.0, 1.0) && in_range(y, 0.0, 1.0) && in_range(yy, 0.0, 1.0)) {
        return None;
    }
    let div = -2.0 * x + 12.0 * y + 3.0;
    if div == 0.0 {
        return Some(Xyz {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
    }
    let u = 4.0 * x / div;
    let v = 9.0 * y / div;
    let div = 6.0 * u - 16.0 * v + 12.0;
    let px = 9.0 * u / div;
    let mut py = 4.0 * v / div;
    let pz = 1.0 - px - py;
    if py == 0.0 {
        py = 0.00001;
    }
    Some(Xyz {
        x: px * yy / py,
        y: yy,
        z: pz * yy / py,
    })
}

/// CIE 015:2018 C.1-C.9's Y: the cube above L* = 8, the line below.
fn cie_y(l: f64) -> f64 {
    let fy = (l + 16.0) / 116.0;
    if l > 8.0 {
        fy * fy * fy
    } else {
        (fy - 16.0 / 116.0) * (108.0 / 841.0)
    }
}

/// CIELAB to XYZ, CIE 015:2018 Annex C, against the screen white.
fn lab_to_xyz(ctx: &Ctx, l: f64, a: f64, b: f64) -> Option<Xyz> {
    if !in_range(l, 0.0, 100.0) {
        return None;
    }
    let inv = |f: f64| {
        if f > 24.0 / 116.0 {
            f * f * f
        } else {
            (f - 16.0 / 116.0) * (108.0 / 841.0)
        }
    };
    let fy = (l + 16.0) / 116.0;
    let w = ctx.white;
    Some(Xyz {
        x: w.x * inv(a / 500.0 + fy),
        y: cie_y(l),
        z: w.z * inv(fy - b / 200.0),
    })
}

/// CIELUV to u'v'Y, CIE 015:2018 8.2.2 reversed: u' = u* / 13L* + u'n.
fn luv_to_uvy(ctx: &Ctx, l: f64, us: f64, vs: f64) -> Option<UvY> {
    if !in_range(l, 0.0, 100.0) {
        return None;
    }
    let w = ctx.white_uvy;
    let y = cie_y(l);
    if l == 0.0 {
        return Some(UvY { u: w.u, v: w.v, y });
    }
    let t = 13.0 * l;
    Some(UvY {
        u: us / t + w.u,
        v: vs / t + w.v,
        y,
    })
}

/// `XcmsTekHVC_ValidSpec`: V from 0 to 100, C not negative, H taken into
/// [0, 360).
fn hvc_valid(mut c: Hvc) -> Option<Hvc> {
    if c.v < -XMY_EPS || c.v > 100.0 + XMY_EPS || c.c < -XMY_EPS {
        return None;
    }
    if c.v < 0.0 {
        c.v = XMY_EPS;
    } else if c.v > 100.0 {
        c.v = 100.0 - XMY_EPS;
    }
    if c.c < 0.0 {
        c.c = -XMY_EPS;
    }
    // Xcms adds or takes 360 until H is in range, one turn a step; the
    // remainder is the same angle without a loop as long as H is large.
    c.h = core::f64::math::rem_euclid(c.h, 360.0);
    if c.h >= 360.0 {
        c.h -= 360.0;
    }
    Some(c)
}

/// `_XcmsTekHVC_CheckModify`.
fn hvc_check_modify(mut c: Hvc) -> Hvc {
    if c.v < 0.0 {
        c.v = XMY_EPS;
    } else if c.v > 100.0 {
        c.v = 100.0 - XMY_EPS;
    }
    if c.c < 0.0 {
        c.c = -XMY_EPS;
    }
    if c.h < 0.0 {
        let n = core::f64::math::trunc(-c.h / 360.0);
        c.h += (n + 1.0) * 360.0;
        if c.h >= 360.0 {
            c.h -= 360.0;
        }
    } else if c.h >= 360.0 {
        let n = core::f64::math::trunc(c.h / 360.0);
        c.h -= n * 360.0;
    }
    c
}

/// `XcmsTekHVCToCIEuvY`.
fn hvc_to_uvy(ctx: &Ctx, c: Hvc) -> Option<UvY> {
    let c = hvc_valid(c)?;
    let w = ctx.white_uvy;
    if c.v == 0.0 || c.v == 100.0 {
        let y = if c.v == 100.0 { 1.0 } else { 0.0 };
        return Some(UvY { u: w.u, v: w.v, y });
    }
    let hue = radians(core::f64::math::rem_euclid(c.h + ctx.theta_offset, 360.0));
    let u = (cosine(hue) * c.c) / (c.v * CHROMA_SCALE_FACTOR);
    let v = (sine(hue) * c.c) / (c.v * CHROMA_SCALE_FACTOR);
    let y = if c.v < 7.99953624 {
        c.v / 903.29
    } else {
        let t = (c.v + 16.0) / 116.0;
        t * t * t
    };
    Some(UvY {
        u: u + w.u,
        v: v + w.v,
        y,
    })
}

/// `XcmsCIEuvYToTekHVC`.
fn uvy_to_hvc(ctx: &Ctx, c: UvY) -> Option<Hvc> {
    if !uvy_valid(c) {
        return None;
    }
    let w = ctx.white_uvy;
    let u = c.u - w.u;
    let v = c.v - w.v;
    // Xcms brings the arctangent into the point's quadrant by adding or
    // taking 90 degrees until it lies there, which is a half or a whole
    // turn for any angle the arithmetic can hold. For an angle a rounding
    // error from the u' axis it is not: -1e-17 plus 90 is 90, and three
    // steps land on 270, a quarter turn from the hue, so a colour on that
    // axis was clipped toward blue. The half or whole turn is added here
    // in one step, because the manual's clip keeps the hue: `XcmsTekHVCClipC`
    // reduces "the Chroma dimension" only, and `XcmsTekHVCClipV` clips
    // "while maintaining the same Hue" (`[[source: xlib-ch06-b8d5a70]]`
    // 4616, 4629-4631); libX11's 90-degree loop departs from that. A point
    // exactly on the negative u' axis (v == 0) keeps the angle 0 that Xcms
    // gives it.
    let mut theta = if u == 0.0 {
        0.0
    } else {
        degrees(arc_tangent(v / u))
    };
    if u < 0.0 && v != 0.0 {
        theta += 180.0;
    } else if u > 0.0 && v < 0.0 {
        theta += 360.0;
    }
    let l2 = if c.y < 0.008856 {
        c.y * 903.29
    } else {
        cube_root(c.y) * 116.0 - 16.0
    };
    let chroma = l2 * CHROMA_SCALE_FACTOR * square_root(u * u + v * v);
    if chroma < 0.0 {
        theta = 0.0;
    }
    let mut h = theta - ctx.theta_offset;
    while h < -EPS {
        h += 360.0;
    }
    while h >= 360.0 + EPS {
        h -= 360.0;
    }
    Some(Hvc {
        h,
        v: l2,
        c: chroma,
    })
}

fn hvc_to_xyz(ctx: &Ctx, c: Hvc) -> Option<Xyz> {
    uvy_to_xyz(hvc_to_uvy(ctx, c)?, ctx.white_uvy)
}

fn xyz_to_hvc(ctx: &Ctx, c: Xyz) -> Option<Hvc> {
    uvy_to_hvc(ctx, xyz_to_uvy(c, Some(ctx.white_uvy))?)
}

fn rgbi_to_xyz(ctx: &Ctx, rgb: [f64; 3]) -> Xyz {
    let [x, y, z] = mat_vec(&ctx.p.to_xyz, rgb);
    Xyz { x, y, z }
}

/// XYZ to RGB intensity with no compression, `XcmsCIEXYZToRGBi` with no
/// compression procedure: `Err` carries the out-of-gamut intensities, which
/// Xcms leaves in the colour for its searches to read.
fn xyz_to_rgbi_raw(ctx: &Ctx, c: Xyz) -> Result<[f64; 3], [f64; 3]> {
    let t = mat_vec(&ctx.p.from_xyz, [c.x, c.y, c.z]);
    // Written with `contains` rather than `min` and `max`, which pass a NaN
    // by: a NaN intensity is out of gamut.
    if t.iter().all(|v| (-EPS..=1.0 + EPS).contains(v)) {
        Ok(t.map(|v| v.clamp(0.0, 1.0)))
    } else {
        Err(t)
    }
}

/// `_XcmsTekHVCQueryMaxVCRGB`: the most chromatic colour of a hue, from
/// TekHVC 40/120 brought to the gamut's surface in RGB.
fn query_max_vc_rgb(ctx: &Ctx, hue: f64) -> Option<(Hvc, [f64; 3])> {
    let xyz = hvc_to_xyz(
        ctx,
        Hvc {
            h: hue,
            v: 40.0,
            c: 120.0,
        },
    )?;
    let (Ok(t) | Err(t)) = xyz_to_rgbi_raw(ctx, xyz);
    if !t.iter().all(|v| v.is_finite()) {
        return None;
    }
    let small = t[0].min(t[1]).min(t[2]);
    let t = t.map(|v| v - small);
    let large = t[0].max(t[1]).max(t[2]);
    let t = t.map(|v| v / large);
    let mut hvc = xyz_to_hvc(ctx, rgbi_to_xyz(ctx, t))?;
    hvc.h = hue;
    Some((hvc, t))
}

/// `XcmsTekHVCQueryMaxC`: the largest chroma a hue and value have in
/// gamut. Below the hue's most chromatic value the answer is on the line to
/// black; above it, a search along the line from that colour to white, a
/// hundred steps at most.
fn query_max_c(ctx: &Ctx, hue: f64, value: f64) -> Option<Hvc> {
    let mut tmp = hvc_check_modify(Hvc {
        h: hue,
        v: value,
        c: 100.0,
    });
    let (max_vc, saved) = query_max_vc_rgb(ctx, hue)?;
    if value <= max_vc.v {
        tmp.c = value * max_vc.c / max_vc.v;
        return Some(hvc_check_modify(tmp));
    }
    let saved_value = value;
    let mut n_value = value;
    let mut last_value = -1.0;
    let mut last_chroma = -1.0;
    let max_dist = 100.0 - max_vc.v;
    let mut r_factor = 1.0;
    let nearer = |tmp: &mut Hvc, last_value: f64, last_chroma: f64| {
        if (last_value - saved_value).abs() < (tmp.v - saved_value).abs() {
            tmp.v = last_value;
            tmp.c = last_chroma;
        }
    };
    for _ in 0..100 {
        let prev_value = last_value;
        last_value = tmp.v;
        last_chroma = tmp.c;
        let nt = (n_value - max_vc.v) / max_dist * r_factor;
        tmp = xyz_to_hvc(ctx, rgbi_to_xyz(ctx, saved.map(|s| s * (1.0 - nt) + nt)))?;
        if tmp.v <= saved_value + EPS && tmp.v >= saved_value - EPS {
            tmp.h = hue;
            return Some(hvc_check_modify(tmp));
        }
        n_value += saved_value - tmp.v;
        if n_value < max_vc.v {
            n_value = max_vc.v;
            r_factor *= 0.5;
        } else if n_value > 100.0 {
            tmp.h = hue;
            nearer(&mut tmp, last_value, last_chroma);
            return Some(hvc_check_modify(tmp));
        } else if tmp.v <= prev_value + EPS && tmp.v >= prev_value - EPS {
            r_factor *= 0.5;
        }
    }
    nearer(&mut tmp, last_value, last_chroma);
    tmp.h = hue;
    Some(tmp)
}

/// XYZ to RGB intensity; out of gamut, `XcmsTekHVCClipC` replaces it with
/// the most chromatic colour of its TekHVC hue and value, and a colour still
/// out of gamut is refused, as `XcmsCIEXYZToRGBi` does.
fn xyz_to_rgbi(ctx: &Ctx, c: Xyz) -> Option<[f64; 3]> {
    // A value the grammar takes can still overflow on the way here
    // (`CIEXYZ:1e308/0.5/0`, `TekHVC:0/0.00001/1e308`): libX11 goes on with
    // the infinity, and baud refuses it.
    if !(c.x.is_finite() && c.y.is_finite() && c.z.is_finite()) {
        return None;
    }
    match xyz_to_rgbi_raw(ctx, c) {
        Ok(t) => Some(t),
        Err(_) => {
            let hvc = xyz_to_hvc(ctx, c)?;
            let max = query_max_c(ctx, hvc.h, hvc.v)?;
            xyz_to_rgbi_raw(ctx, hvc_to_xyz(ctx, max)?).ok()
        }
    }
}

/// A numeric spec, its space and the bytes after the colon, as the 16-bit
/// device value of each channel.
pub(crate) fn resolve(space: Space, fields: &[u8]) -> Option<[u16; 3]> {
    resolve_on(&SRGB, space, fields)
}

fn resolve_on(p: &'static Profile, space: Space, fields: &[u8]) -> Option<[u16; 3]> {
    let ctx = Ctx::new(p)?;
    let [a, b, c] = triple(fields)?;
    let rgbi = match space {
        // "floating-point values between 0.0 and 1.0, inclusive". libX11
        // checks no range, and its table search extrapolates past 1:
        // `rgbi:1.1/0/0` draws 0b0000 there.
        Space::RgbI => {
            if !(in_range(a, 0.0, 1.0) && in_range(b, 0.0, 1.0) && in_range(c, 0.0, 1.0)) {
                return None;
            }
            [a, b, c].map(|v| v.clamp(0.0, 1.0))
        }
        Space::CieXyz => {
            let x = Xyz { x: a, y: b, z: c };
            if !xyz_valid(x) {
                return None;
            }
            xyz_to_rgbi(&ctx, x)?
        }
        Space::CieUvY => xyz_to_rgbi(&ctx, uvy_to_xyz(UvY { u: a, v: b, y: c }, ctx.white_uvy)?)?,
        Space::CieXyY => xyz_to_rgbi(&ctx, xyy_to_xyz(a, b, c)?)?,
        Space::CieLab => xyz_to_rgbi(&ctx, lab_to_xyz(&ctx, a, b, c)?)?,
        Space::CieLuv => xyz_to_rgbi(&ctx, uvy_to_xyz(luv_to_uvy(&ctx, a, b, c)?, ctx.white_uvy)?)?,
        Space::TekHvc => xyz_to_rgbi(&ctx, hvc_to_xyz(&ctx, Hvc { h: a, v: b, c })?)?,
    };
    Some((p.encode)(rgbi))
}

#[cfg(test)]
mod tests;
