//! Each source file transcribed in part from another work carries that
//! work's copyright lines, its licence and, where the work's text is in
//! `LICENSES/`, the path to it, in the comment block it opens with. Each
//! copyright line read here is the one that text gives.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The words of `s`, one space between each.
fn words(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The comment block a source opens with: its lines up to the first that
/// is neither blank nor a comment, each without its comment marker.
fn top(src: &str) -> String {
    let block: Vec<&str> = src
        .lines()
        .map(str::trim_start)
        .take_while(|l| l.is_empty() || l.starts_with("//"))
        .map(|l| l.trim_start_matches('/').trim_start_matches('!'))
        .collect();
    words(&block.join("\n"))
}

/// A licence text's words, each line without a leading `*`.
fn text(path: &str) -> String {
    let t = fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    let lines: Vec<&str> = t
        .lines()
        .map(|l| l.trim_start().trim_start_matches('*'))
        .collect();
    words(&lines.join("\n"))
}

/// `path`'s opening comment block holds every one of `copyright` and
/// `licence`, and each of `copyright` is in the licence text `from`.
fn carries(path: &str, copyright: &[&str], licence: &[&str], from: Option<&str>) {
    let src = fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    let top = top(&src);
    let missing: Vec<&&str> = copyright
        .iter()
        .chain(licence)
        .filter(|w| !top.contains(&words(w)))
        .collect();
    assert!(missing.is_empty(), "{path} does not open with {missing:?}");
    if let Some(from) = from {
        let text = text(from);
        for c in copyright {
            assert!(text.contains(&words(c)), "{from} has no {c:?}");
        }
    }
}

#[test]
fn the_rasteriser_carries_its_notice() {
    carries(
        "baud-font/src/raster.rs",
        &[
            "Copyright 2015 Google Inc. All rights reserved.",
            "Modifications copyright (C) 2020 Alex Butler",
        ],
        &[
            "Licensed under the Apache License, Version 2.0",
            "LICENSES/Apache-2.0.txt",
        ],
        None,
    );
}

#[test]
fn the_xcms_port_carries_its_notices() {
    carries(
        "baud-domain/src/vt/xcms.rs",
        &[
            "Code and supporting documentation (c) Copyright 1990 1991 Tektronix, Inc.",
            "Copyright 1990, 1998 The Open Group",
        ],
        &[
            "Permission is hereby granted to use, copy, modify, sell, and otherwise distribute this software",
            "Permission to use, copy, modify, distribute, and sell this software",
            "LICENSES/libx11-xcms.txt",
        ],
        Some("LICENSES/libx11-xcms.txt"),
    );
}

#[test]
fn the_deflate_port_carries_its_notice() {
    carries(
        "baud-bench/src/capture/deflate.rs",
        &["Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler"],
        &["This notice may not be removed or altered from any source distribution."],
        None,
    );
}

#[test]
fn the_fdlibm_kernels_carry_their_notice() {
    carries(
        "tools/baud-pictures/src/det.rs",
        &["Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved."],
        &[
            "Permission to use, copy, modify, and distribute this software is freely granted, \
             provided that this notice is preserved.",
            "Licence: SunPro.",
            "LICENSES/fdlibm-SunPro.txt",
        ],
        Some("LICENSES/fdlibm-SunPro.txt"),
    );
}

#[test]
fn the_cpython_float_arithmetic_carries_its_notice() {
    carries(
        "tools/baud-pictures/src/py.rs",
        &["Copyright (c) 2001 Python Software Foundation; All Rights Reserved"],
        &[
            "Python Software Foundation License Version 2",
            "LICENSES/cpython-Python-2.0.txt",
        ],
        Some("LICENSES/cpython-Python-2.0.txt"),
    );
}

#[test]
fn the_libwebp_interface_carries_its_notice() {
    carries(
        "tools/baud-pictures/src/webp.rs",
        &["Copyright (c) 2010, Google Inc. All rights reserved."],
        &[
            "Licence: BSD-3-Clause.",
            "LICENSES/libwebp-BSD-3-Clause.txt",
        ],
        Some("LICENSES/libwebp-BSD-3-Clause.txt"),
    );
}

#[test]
fn the_numpy_arithmetic_carries_its_notice() {
    carries(
        "tools/baud-pictures/src/numpy.rs",
        &["Copyright (c) 2005-2025, NumPy Developers. All rights reserved."],
        &["Licence: BSD-3-Clause.", "LICENSES/numpy-BSD-3-Clause.txt"],
        Some("LICENSES/numpy-BSD-3-Clause.txt"),
    );
}

/// Pillow's copyright lines, as `LICENSES/pillow-MIT-CMU.txt` gives them.
const PILLOW: [&str; 3] = [
    "Copyright © 1997-2011 by Secret Labs AB",
    "Copyright © 1995-2011 by Fredrik Lundh and contributors",
    "Copyright © 2010 by Jeffrey 'Alex' Clark and contributors",
];

/// `path` carries Pillow's notice.
fn carries_pillow(path: &str) {
    carries(
        path,
        &PILLOW,
        &["Licence: MIT-CMU.", "LICENSES/pillow-MIT-CMU.txt"],
        Some("LICENSES/pillow-MIT-CMU.txt"),
    );
}

#[test]
fn the_pillow_png_writer_carries_its_notice() {
    carries_pillow("tools/baud-pictures/src/png.rs");
}

#[test]
fn the_pillow_lanczos_resize_carries_its_notice() {
    carries_pillow("tools/baud-pictures/src/resample.rs");
}

#[test]
fn the_pillow_webp_writer_and_reader_carry_its_notice() {
    carries_pillow("tools/baud-pictures/src/webp.rs");
}

#[test]
fn the_pillow_gif_loop_test_carries_its_notice() {
    carries_pillow("tools/baud-pictures/src/gif.rs");
}
