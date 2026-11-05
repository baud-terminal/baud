//! **The C probe that holds `hb.rs` and `ft.rs` to the headers**, the way
//! `baud-app/src/sys.rs`'s tests hold its declarations to the platform's:
//! written, compiled with `cc` against the flags `pkg-config` gives for the
//! linked HarfBuzz and FreeType, run, and its `name=value` lines read back.
//! A wrong signature usually fails to link; a wrong field order compiles,
//! links and reads the wrong member, so the struct half cannot be left to
//! reading.

#![cfg(test)]

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const PROBE: &str = r#"
#include <stddef.h>
#include <stdio.h>
#include <hb.h>
#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_MULTIPLE_MASTERS_H

#define SIZE(t) printf(#t ".size=%lld\n", (long long)sizeof(t))
#define AT(t, f) printf(#t "." #f "=%lld\n", (long long)offsetof(t, f))
#define VALUE(v) printf(#v "=%lld\n", (long long)(v))

int main(void) {
    SIZE(hb_variation_t); AT(hb_variation_t, tag); AT(hb_variation_t, value);
    SIZE(hb_feature_t); AT(hb_feature_t, tag); AT(hb_feature_t, value);
    AT(hb_feature_t, start); AT(hb_feature_t, end);
    SIZE(hb_glyph_info_t); AT(hb_glyph_info_t, codepoint); AT(hb_glyph_info_t, cluster);
    SIZE(hb_glyph_position_t); AT(hb_glyph_position_t, x_advance);
    AT(hb_glyph_position_t, x_offset); AT(hb_glyph_position_t, y_offset);
    SIZE(hb_codepoint_t); SIZE(hb_position_t);
    VALUE(HB_MEMORY_MODE_DUPLICATE); VALUE(HB_FEATURE_GLOBAL_END);
    VALUE(HB_VERSION_MAJOR); VALUE(HB_VERSION_MINOR); VALUE(HB_VERSION_MICRO);

    SIZE(FT_Vector); AT(FT_Vector, y);
    SIZE(FT_Outline); AT(FT_Outline, n_contours); AT(FT_Outline, n_points);
    AT(FT_Outline, points); AT(FT_Outline, tags); AT(FT_Outline, contours);
    AT(FT_Outline, flags);
    printf("FT_Outline.n_contours.size=%lld\n", (long long)sizeof(((FT_Outline *)0)->n_contours));
    printf("FT_Outline.contours.elem=%lld\n", (long long)sizeof(*((FT_Outline *)0)->contours));
    AT(FT_FaceRec, glyph); AT(FT_GlyphSlotRec, outline);
    SIZE(FT_Pos); SIZE(FT_Fixed); SIZE(FT_Error);
    VALUE(FT_LOAD_NO_SCALE); VALUE(FT_LOAD_NO_HINTING); VALUE(FT_CURVE_TAG_ON);
    VALUE(FREETYPE_MAJOR); VALUE(FREETYPE_MINOR); VALUE(FREETYPE_PATCH);

    /* The header's own error list, expanded the way fterrors.h documents. */
#undef FTERRORS_H_
#define FT_ERRORDEF(e, v, s) printf("error %d %s\n", (int)(v), (s));
#define FT_ERROR_START_LIST
#define FT_ERROR_END_LIST
#include FT_ERRORS_H
    return 0;
}
"#;

/// The integer lines the probe prints: the floor its output is held to.
const PRINTED: usize = 44;

/// The errors `fterrdef.h` defines, the probe counted on to print them.
const DEFINED: usize = 95;

fn run_probe() -> String {
    static CALL: AtomicUsize = AtomicUsize::new(0);
    let call = CALL.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("baud-oracle-abi-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temp dir");
    let (src, bin) = (dir.join("probe.c"), dir.join("probe"));
    std::fs::write(&src, PROBE).expect("write the probe");
    let flags = Command::new("pkg-config")
        .args(["--cflags", "harfbuzz", "freetype2"])
        .output()
        .expect("pkg-config, which tools/shell.nix provides");
    assert!(
        flags.status.success(),
        "pkg-config found no harfbuzz or freetype2"
    );
    let flags = String::from_utf8(flags.stdout).expect("utf-8 flags");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let out = Command::new(&cc)
        .args(flags.split_whitespace())
        .arg("-o")
        .arg(&bin)
        .arg(&src)
        .output()
        .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
    assert!(
        out.status.success(),
        "the probe did not compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().expect("run it");
    assert!(run.status.success(), "the probe did not run");
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8(run.stdout).expect("utf-8 output")
}

/// Compile and run the probe; every `name=integer` line it printed.
pub fn ask_the_headers() -> HashMap<String, i128> {
    let text = run_probe();
    let map: HashMap<String, i128> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter_map(|(k, v)| v.trim().parse().ok().map(|n| (k.to_string(), n)))
        .collect();
    // **The control.** A probe that printed nothing, or whose output
    // stopped parsing, would let every assertion pass over an empty map.
    assert!(
        map.len() >= PRINTED,
        "the probe printed {} values, fewer than its {PRINTED} lines: {text}",
        map.len()
    );
    map
}

/// `fterrdef.h`'s codes and texts, as its own macros expand them.
pub fn error_texts() -> Vec<(i32, String)> {
    let text = run_probe();
    let errors: Vec<(i32, String)> = text
        .lines()
        .filter_map(|l| l.strip_prefix("error "))
        .filter_map(|l| l.split_once(' '))
        .map(|(c, m)| (c.parse().expect("a code"), m.to_string()))
        .collect();
    assert_eq!(errors.len(), DEFINED, "the header's error list: {text}");
    errors
}
