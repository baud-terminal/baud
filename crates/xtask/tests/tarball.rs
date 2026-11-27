//! The `.crate` reader: a gzip member around a tar archive, each field it
//! reads perturbed once. The gzip streams here are stored deflate blocks
//! (RFC 1951 3.2.4), written by hand so the test needs no compressor; the
//! compressed path is the inflater's own, and `publish`'s packaging tests
//! read real `.crate` files through it.

use xtask::tarball::{self, Crc, Refusal};

/// A 512-byte tar header: `name`, its size, a type flag, and `prefix` in
/// the ustar field when it is not empty.
fn header(name: &str, data: &[u8], kind: char, prefix: &str) -> Vec<u8> {
    let mut h = vec![0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    let octal = format!("{:011o}\0", data.len());
    h[124..136].copy_from_slice(octal.as_bytes());
    h[156] = u8::try_from(kind).expect("an ASCII type flag");
    h[257..263].copy_from_slice(b"ustar\0");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    h
}

fn entry(tar: &mut Vec<u8>, name: &str, data: &[u8], kind: char, prefix: &str) {
    tar.extend(header(name, data, kind, prefix));
    tar.extend(data);
    tar.resize(tar.len().div_ceil(512) * 512, 0);
}

fn archive() -> Vec<u8> {
    let long = format!("pkg-0.1.0/{}/Cargo.toml", "d".repeat(120));
    let mut tar = Vec::new();
    entry(
        &mut tar,
        "pkg-0.1.0/Cargo.toml",
        b"[package]\nname = \"pkg\"\n",
        '0',
        "",
    );
    entry(
        &mut tar,
        "././@LongLink",
        format!("{long}\0").as_bytes(),
        'L',
        "",
    );
    entry(&mut tar, "truncated", b"the long one\n", '0', "");
    entry(&mut tar, "lib.rs", b"pub fn f() {}\n", '0', "pkg-0.1.0/src");
    tar.extend(vec![0u8; 1024]);
    tar
}

/// `data` as a gzip member of stored blocks.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut gz = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 3];
    gz.extend(b"a.tar\0");
    let chunks: Vec<&[u8]> = data.chunks(40_000).collect();
    for (i, c) in chunks.iter().enumerate() {
        gz.push(u8::from(i + 1 == chunks.len()));
        let len = u16::try_from(c.len()).expect("a stored block");
        gz.extend(len.to_le_bytes());
        gz.extend((!len).to_le_bytes());
        gz.extend(*c);
    }
    let Crc(crc) = tarball::crc32(data);
    gz.extend(crc.to_le_bytes());
    gz.extend(u32::try_from(data.len()).expect("small").to_le_bytes());
    gz
}

#[test]
fn the_crc_is_iso_3309s() {
    assert_eq!(tarball::crc32(b"123456789"), Crc(0xcbf4_3926));
    assert_eq!(tarball::crc32(b""), Crc(0));
}

#[test]
fn a_member_by_name_by_long_name_and_by_prefix() {
    let gz = gzip(&archive());
    assert_eq!(gunzip_len(&gz), archive().len());
    assert_eq!(
        tarball::extract(&gz, "pkg-0.1.0/Cargo.toml").expect("the manifest"),
        b"[package]\nname = \"pkg\"\n"
    );
    let long = format!("pkg-0.1.0/{}/Cargo.toml", "d".repeat(120));
    assert_eq!(
        tarball::extract(&gz, &long).expect("the long name"),
        b"the long one\n"
    );
    assert_eq!(
        tarball::extract(&gz, "pkg-0.1.0/src/lib.rs").expect("the prefixed name"),
        b"pub fn f() {}\n"
    );
    assert_eq!(
        tarball::extract(&gz, "pkg-0.1.0/README.md"),
        Err(Refusal::Missing("pkg-0.1.0/README.md".to_string()))
    );
}

fn gunzip_len(gz: &[u8]) -> usize {
    tarball::gunzip(gz).expect("the archive").len()
}

#[test]
fn each_part_of_the_gzip_member_is_checked() {
    let good = gzip(&archive());
    let n = good.len();
    let mut crc = good.clone();
    crc[n - 8] ^= 1;
    assert_eq!(
        tarball::gunzip(&crc),
        Err(Refusal::Trailer("CRC-32 mismatch"))
    );
    let mut size = good.clone();
    size[n - 4] ^= 1;
    assert!(
        matches!(tarball::gunzip(&size), Err(Refusal::Deflate(_))),
        "{:?}",
        tarball::gunzip(&size)
    );
    let mut magic = good.clone();
    magic[1] = 0;
    assert_eq!(tarball::gunzip(&magic), Err(Refusal::Gzip("no gzip magic")));
    let mut body = good.clone();
    body[20] ^= 0xff;
    assert!(tarball::gunzip(&body).is_err());
    let mut tar = archive();
    tar[124] = b'9';
    assert_eq!(
        tarball::extract(&gzip(&tar), "pkg-0.1.0/Cargo.toml"),
        Err(Refusal::Tar("a size that is not octal"))
    );
}
