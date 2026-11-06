//! What the font-build tests read that is not in the repository: Noto
//! Sans Devanagari 2.007's variable font, from the cache
//! `scripts/build-fonts.sh` fetches it into, and fontTools 4.63.0's
//! instances of it, from the directory `BAUD_FONTTOOLS_NOTO` names. Each is
//! refused unless it hashes as pinned; a test that needs one that is
//! absent says so and passes on what it has.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub const EPOCH: i64 = 315_532_800;
const VF_SHA256: &str = "9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(path: &str) -> Vec<u8> {
    std::fs::read(root().join(path)).unwrap()
}

pub fn sha256(data: &[u8]) -> String {
    // The pinned digests are read here without a dependency: FIPS 180-4.
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The variable font, from the cache, or `None` with a note.
pub fn noto_vf() -> Option<Vec<u8>> {
    let dir = std::env::var_os("NOTO_CACHE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME").map(|d| PathBuf::from(d).join("baud-readme"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/baud-readme"))
        })?;
    let Ok(vf) = std::fs::read(dir.join("NotoSansDevanagari[wdth,wght].ttf")) else {
        eprintln!("the variable Noto Sans Devanagari is not in the cache: skipped");
        return None;
    };
    assert_eq!(
        sha256(&vf),
        VF_SHA256,
        "the cache's file is not the pinned one"
    );
    Some(vf)
}

/// fontTools' instance at `w`, from `BAUD_FONTTOOLS_NOTO`, or `None` with
/// a note.
pub fn fonttools(w: i32) -> Option<Vec<u8>> {
    let Some(dir) = std::env::var_os("BAUD_FONTTOOLS_NOTO") else {
        eprintln!("BAUD_FONTTOOLS_NOTO unset: the comparison with fontTools did not run");
        return None;
    };
    let want = match w {
        400 => "92cde87d462bf2a8a9e4cccb51c02c9aeaf6ef4e99db06af0ebc0e7c5e82f64b",
        700 => "f3e9be2b943be49cc1683f30456b9dba068422a917a4918677e8cb0dc652cdc4",
        _ => unreachable!("no fontTools instance at {w}"),
    };
    let data = std::fs::read(PathBuf::from(dir).join(format!("noto{w}.ttf")))
        .unwrap_or_else(|e| panic!("BAUD_FONTTOOLS_NOTO: noto{w}.ttf: {e}"));
    assert_eq!(
        sha256(&data),
        want,
        "noto{w}.ttf is not fontTools' pinned instance"
    );
    Some(data)
}
