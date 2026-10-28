//! CFF Standard Encoding, code to SID, generated from Appendix B of
//! `[[source: adobe-5176-cff]]` by `scripts/gen-cff-standard-encoding.py`.
//! Do not edit; regenerate.
//!
//! The table is part of Appendix B, "Predefined Encodings", of The Compact
//! Font Format Specification, Technical Note #5176, version 1.0, by Adobe
//! Systems Incorporated:
//! https://adobe-type-tools.github.io/font-tech-notes/pdfs/5176.CFF.pdf
//!
//! Copyright © 1996–1998, 2000, 2003 by Adobe Systems Incorporated.
//! All rights reserved.
//!
//! Adobe licenses its font technical notes under the Creative Commons
//! Attribution-NoDerivatives 4.0 International licence (CC BY-ND 4.0),
//! https://creativecommons.org/licenses/by-nd/4.0/, in the LICENSE.md of
//! https://github.com/adobe-type-tools/font-tech-notes. The material is
//! offered as-is and as-available, with no warranties of any kind: see the
//! licence's section 5, Disclaimer of Warranties and Limitation of
//! Liability.
//!
//! Not modified. The appendix's code and SID columns are reproduced in
//! part, as the licence's section 2(a)(1)(A) allows: its name column is
//! left out, and each SID is one byte at its code. Writing the same values
//! in another format is a technical modification under section 2(a)(4),
//! which never produces Adapted Material.
//!
//! Indexed by the code `seac` supplies; the value is the standard string id
//! the face's charset then maps to a glyph. Every SID in Standard Encoding is
//! below 150, so a byte holds it.

/// `STANDARD_ENCODING[code]` is the SID; 0 is `.notdef`.
pub const STANDARD_ENCODING: &[u8; 256] = b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\x3e\x3f\x40\x41\x42\x43\x44\x45\x46\x47\x48\x49\x4a\x4b\x4c\x4d\x4e\x4f\x50\x51\x52\x53\x54\x55\x56\x57\x58\x59\x5a\x5b\x5c\x5d\x5e\x5f\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x60\x61\x62\x63\x64\x65\x66\x67\x68\x69\x6a\x6b\x6c\x6d\x6e\x00\x6f\x70\x71\x72\x00\x73\x74\x75\x76\x77\x78\x79\x7a\x00\x7b\x00\x7c\x7d\x7e\x7f\x80\x81\x82\x83\x00\x84\x85\x00\x86\x87\x88\x89\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x8a\x00\x8b\x00\x00\x00\x00\x8c\x8d\x8e\x8f\x00\x00\x00\x00\x00\x90\x00\x00\x00\x91\x00\x00\x92\x93\x94\x95\x00\x00\x00\x00";
