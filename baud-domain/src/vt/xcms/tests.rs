//! The port held to libX11 under libX11's own screen characterisation.
//!
//! libX11 with no XDCCC properties on the root window converts through a
//! Tektronix CRT, `Default_RGB_SCCData` (`[[source: libx11-lrgb-b8d5a70]]`
//! 214-440): a matrix and three intensity tables, read by `_XcmsTableSearch`
//! and `_XcmsIntensityInterpolation` at eight bits per RGB. With that
//! profile in place of sRGB, the conversions, the TekHVC clip and the
//! searches it calls draw what libX11 1.8.13 draws on a 24-bit Xvfb
//! (`[[source: xparsecolor-obs-xcms-out]]`), for every spec whose space
//! baud reads as libX11 does: `rgbi:` in range, `CIEXYZ:`, `CIEuvY:`,
//! `CIExyY:` and `TekHVC:`. CIELab and CIELuv are CIE's here and not
//! libX11's, and are held in `baud-domain/tests/x_parse_color.rs`.
//!
//! The CRT data and the table search are `LRGB.c`'s, under Tektronix's
//! Xcms notice, which the parent module reproduces and
//! `LICENSES/libx11-xcms.txt` holds.
use super::*;

#[allow(clippy::excessive_precision)] // as LRGB.c prints them
const RED: [(u16, f64); 52] = [
    (0x0000, 0.000000),
    (0x0909, 0.000000),
    (0x0a0a, 0.000936),
    (0x0f0f, 0.001481),
    (0x1414, 0.002329),
    (0x1919, 0.003529),
    (0x1e1e, 0.005127),
    (0x2323, 0.007169),
    (0x2828, 0.009699),
    (0x2d2d, 0.012759),
    (0x3232, 0.016392),
    (0x3737, 0.020637),
    (0x3c3c, 0.025533),
    (0x4141, 0.031119),
    (0x4646, 0.037431),
    (0x4b4b, 0.044504),
    (0x5050, 0.052373),
    (0x5555, 0.061069),
    (0x5a5a, 0.070624),
    (0x5f5f, 0.081070),
    (0x6464, 0.092433),
    (0x6969, 0.104744),
    (0x6e6e, 0.118026),
    (0x7373, 0.132307),
    (0x7878, 0.147610),
    (0x7d7d, 0.163958),
    (0x8282, 0.181371),
    (0x8787, 0.199871),
    (0x8c8c, 0.219475),
    (0x9191, 0.240202),
    (0x9696, 0.262069),
    (0x9b9b, 0.285089),
    (0xa0a0, 0.309278),
    (0xa5a5, 0.334647),
    (0xaaaa, 0.361208),
    (0xafaf, 0.388971),
    (0xb4b4, 0.417945),
    (0xb9b9, 0.448138),
    (0xbebe, 0.479555),
    (0xc3c3, 0.512202),
    (0xc8c8, 0.546082),
    (0xcdcd, 0.581199),
    (0xd2d2, 0.617552),
    (0xd7d7, 0.655144),
    (0xdcdc, 0.693971),
    (0xe1e1, 0.734031),
    (0xe6e6, 0.775322),
    (0xebeb, 0.817837),
    (0xf0f0, 0.861571),
    (0xf5f5, 0.906515),
    (0xfafa, 0.952662),
    (0xffff, 1.000000),
];
#[allow(clippy::excessive_precision)] // as LRGB.c prints them
const GREEN: [(u16, f64); 50] = [
    (0x0000, 0.000000),
    (0x1313, 0.000000),
    (0x1414, 0.000832),
    (0x1919, 0.001998),
    (0x1e1e, 0.003612),
    (0x2323, 0.005736),
    (0x2828, 0.008428),
    (0x2d2d, 0.011745),
    (0x3232, 0.015740),
    (0x3737, 0.020463),
    (0x3c3c, 0.025960),
    (0x4141, 0.032275),
    (0x4646, 0.039449),
    (0x4b4b, 0.047519),
    (0x5050, 0.056520),
    (0x5555, 0.066484),
    (0x5a5a, 0.077439),
    (0x5f5f, 0.089409),
    (0x6464, 0.102418),
    (0x6969, 0.116485),
    (0x6e6e, 0.131625),
    (0x7373, 0.147853),
    (0x7878, 0.165176),
    (0x7d7d, 0.183604),
    (0x8282, 0.203140),
    (0x8787, 0.223783),
    (0x8c8c, 0.245533),
    (0x9191, 0.268384),
    (0x9696, 0.292327),
    (0x9b9b, 0.317351),
    (0xa0a0, 0.343441),
    (0xa5a5, 0.370580),
    (0xaaaa, 0.398747),
    (0xafaf, 0.427919),
    (0xb4b4, 0.458068),
    (0xb9b9, 0.489165),
    (0xbebe, 0.521176),
    (0xc3c3, 0.554067),
    (0xc8c8, 0.587797),
    (0xcdcd, 0.622324),
    (0xd2d2, 0.657604),
    (0xd7d7, 0.693588),
    (0xdcdc, 0.730225),
    (0xe1e1, 0.767459),
    (0xe6e6, 0.805235),
    (0xebeb, 0.843491),
    (0xf0f0, 0.882164),
    (0xf5f5, 0.921187),
    (0xfafa, 0.960490),
    (0xffff, 1.000000),
];
#[allow(clippy::excessive_precision)] // as LRGB.c prints them
const BLUE: [(u16, f64); 51] = [
    (0x0000, 0.000000),
    (0x0e0e, 0.000000),
    (0x0f0f, 0.001341),
    (0x1414, 0.002080),
    (0x1919, 0.003188),
    (0x1e1e, 0.004729),
    (0x2323, 0.006766),
    (0x2828, 0.009357),
    (0x2d2d, 0.012559),
    (0x3232, 0.016424),
    (0x3737, 0.021004),
    (0x3c3c, 0.026344),
    (0x4141, 0.032489),
    (0x4646, 0.039481),
    (0x4b4b, 0.047357),
    (0x5050, 0.056154),
    (0x5555, 0.065903),
    (0x5a5a, 0.076634),
    (0x5f5f, 0.088373),
    (0x6464, 0.101145),
    (0x6969, 0.114968),
    (0x6e6e, 0.129862),
    (0x7373, 0.145841),
    (0x7878, 0.162915),
    (0x7d7d, 0.181095),
    (0x8282, 0.200386),
    (0x8787, 0.220791),
    (0x8c8c, 0.242309),
    (0x9191, 0.264937),
    (0x9696, 0.288670),
    (0x9b9b, 0.313499),
    (0xa0a0, 0.339410),
    (0xa5a5, 0.366390),
    (0xaaaa, 0.394421),
    (0xafaf, 0.423481),
    (0xb4b4, 0.453547),
    (0xb9b9, 0.484592),
    (0xbebe, 0.516587),
    (0xc3c3, 0.549498),
    (0xc8c8, 0.583291),
    (0xcdcd, 0.617925),
    (0xd2d2, 0.653361),
    (0xd7d7, 0.689553),
    (0xdcdc, 0.726454),
    (0xe1e1, 0.764013),
    (0xe6e6, 0.802178),
    (0xebeb, 0.840891),
    (0xf0f0, 0.880093),
    (0xf5f5, 0.919723),
    (0xfafa, 0.959715),
    (0xffff, 1.00000),
];

/// `_XcmsTableSearch` with `_XcmsIntensityCmp` and
/// `_XcmsIntensityInterpolation`, at eight bits per RGB.
fn tek_value(table: &[(u16, f64)], key: f64) -> u16 {
    const MASK: u16 = 0xff00;
    const SHIFT: i64 = 8;
    const MAX_COLOR: i64 = 255;
    let cmp = |e: f64| {
        if key < e {
            -1
        } else if key > e {
            1
        } else {
            0
        }
    };
    let (mut lo, mut hi) = (0usize, table.len() - 1);
    if cmp(table[lo].1) <= 0 {
        return table[lo].0 & MASK;
    }
    let (mut mid, mut last) = (lo, hi);
    while mid != last {
        last = mid;
        mid = lo + (hi - lo) / 2;
        match cmp(table[mid].1) {
            0 => return table[mid].0 & MASK,
            r if r < 0 => hi = mid,
            _ => lo = mid,
        }
    }
    let (l, h) = (table[lo], table[hi]);
    let ratio = (key - l.1) / (h.1 - l.1);
    // `target *= ratio` on a long: the product truncated.
    let target = ((i64::from(h.0) - i64::from(l.0)) as f64 * ratio) as i64 + i64::from(l.0);
    let mut up = ((target >> SHIFT) * 0xFFFF) / MAX_COLOR;
    let down;
    if up < target {
        down = up;
        up = (((down >> SHIFT) + 1).min(MAX_COLOR) * 0xFFFF) / MAX_COLOR;
    } else {
        down = (((up >> SHIFT) - 1).max(0) * 0xFFFF) / MAX_COLOR;
    }
    let v = if (up - target) < (target - down) {
        up
    } else {
        down
    };
    u16::try_from(v).unwrap() & MASK
}

#[allow(clippy::excessive_precision)] // as LRGB.c prints them
const TEK: Profile = Profile {
    to_xyz: [
        [
            0.38106149108714790,
            0.32025712365352110,
            0.24834578525933100,
        ],
        [
            0.20729745115140850,
            0.68054638776373240,
            0.11215616108485920,
        ],
        [
            0.02133944350088028,
            0.14297193020246480,
            1.24172892629665500,
        ],
    ],
    from_xyz: [
        [
            3.48340481253539000,
            -1.52176374927285200,
            -0.55923133354049780,
        ],
        [
            -1.07152751306193600,
            1.96593795204372400,
            0.03673691339553462,
        ],
        [
            0.06351179790497788,
            -0.20020501000496480,
            0.81070942031648220,
        ],
    ],
    encode: |i| {
        [
            tek_value(&RED, i[0]),
            tek_value(&GREEN, i[1]),
            tek_value(&BLUE, i[2]),
        ]
    },
};

/// The first step of each CRT table above zero intensity: an intensity a
/// rounding error above zero draws it, and one at zero draws 00.
const STEP: [u8; 3] = [0x09, 0x13, 0x0e];

/// **Every spec libX11 drew or refused, drawn or refused the same,** but
/// for three kinds of line, each counted:
///
/// - `rgbi:` past [0, 1], which libX11 extrapolates its tables for and the
///   manual's "between 0.0 and 1.0, inclusive" refuses;
/// - a colour compressed onto the gamut's edge with one channel within a
///   rounding error of zero, where the CRT table's first step decides
///   between 00 and [`STEP`]. Which side a channel lands on is libX11's
///   arithmetic on its build, here arm64 macOS: fusing the matrix
///   product's multiply-adds in baud moves 16 of these lines to agree.
///   baud does not fuse.
///
/// No line is off any other way.
#[test]
fn clip_matches_libx11_under_its_own_profile() {
    let src = include_str!("../../../../docs/research/sources/xparsecolor-observed/xcms.out");
    let (mut same, mut range, mut edge) = (0, 0, 0);
    for line in src.lines() {
        let mut f = line.split('\t');
        let spec = f.next().unwrap().as_bytes();
        let answer = f.next().unwrap();
        let Some(colon) = spec.iter().position(|&b| b == b':') else {
            continue;
        };
        let Some(space) = Space::of(&spec[..colon]) else {
            continue;
        };
        if matches!(space, Space::CieLab | Space::CieLuv) {
            continue;
        }
        let ours =
            resolve_on(&TEK, space, &spec[colon + 1..]).map(|c| c.map(|v| v.to_be_bytes()[0]));
        let theirs = (answer != "FAIL").then(|| {
            let px = u32::from_str_radix(f.next().unwrap(), 16)
                .unwrap()
                .to_be_bytes();
            [px[1], px[2], px[3]]
        });
        let text = core::str::from_utf8(spec).unwrap();
        if ours == theirs {
            same += 1;
        } else if space == Space::RgbI && ours.is_none() {
            range += 1;
        } else {
            let (o, t) = (ours.unwrap(), theirs.unwrap());
            let off: [bool; 3] = core::array::from_fn(|k| o[k] != t[k]);
            let k = off.iter().position(|&x| x).unwrap();
            assert_eq!(
                off.iter().filter(|&&x| x).count(),
                1,
                "{text}: {o:02x?} {t:02x?}"
            );
            let pair = [o[k].min(t[k]), o[k].max(t[k])];
            assert_eq!(pair, [0, STEP[k]], "{text}: {o:02x?} {t:02x?}");
            edge += 1;
        }
    }
    assert_eq!((same, range, edge), (726, 2, 40));
}
