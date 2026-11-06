//! The command line: one payload directory, and anything after it ignored.

use crate::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The directory `multibench` dumped into. An argument that looks like a flag
/// is a directory name like any other, and fails when the payloads do not read.
pub fn payload_dir(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, Error> {
    let mut args = args.into_iter();
    let argv0 = args.next();
    match args.next() {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Err(Error::Usage {
            argv0: argv0.map_or_else(
                || "kitty-bench".to_string(),
                |a| a.to_string_lossy().into_owned(),
            ),
        }),
    }
}

/// The two payloads, whole, as `multibench` wrote them.
#[derive(Debug)]
pub struct Payloads {
    pub stream: Vec<u8>,
    pub tui: Vec<u8>,
}

impl Payloads {
    pub fn read(dir: &Path) -> Result<Payloads, Error> {
        Ok(Payloads {
            stream: one(&dir.join("stream.bin"))?,
            tui: one(&dir.join("tui.bin"))?,
        })
    }
}

fn one(path: &Path) -> Result<Vec<u8>, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::Payload {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.is_empty() {
        return Err(Error::EmptyPayload {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    fn os(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_argument_is_a_usage_error_naming_the_program() {
        let e = payload_dir(os(&["./bench"])).unwrap_err();
        assert!(
            matches!(&e, Error::Usage { argv0 } if argv0 == "./bench"),
            "{e:?}"
        );
        assert!(
            e.to_string().starts_with("usage: ./bench <payload-dir>"),
            "{e}"
        );
    }

    #[test]
    fn no_argv_at_all_is_a_usage_error() {
        assert!(matches!(payload_dir(os(&[])), Err(Error::Usage { .. })));
    }

    #[test]
    fn the_first_argument_is_the_directory_and_the_rest_are_ignored() {
        let d = payload_dir(os(&["b", "/p", "extra", "more"])).unwrap();
        assert_eq!(d, PathBuf::from("/p"));
    }

    #[test]
    fn a_flag_is_a_directory_name() {
        assert_eq!(
            payload_dir(os(&["b", "--help"])).unwrap(),
            PathBuf::from("--help")
        );
    }

    #[test]
    fn an_empty_argument_is_the_current_directory() {
        let d = payload_dir(os(&["b", ""])).unwrap();
        assert_eq!(d.join("stream.bin"), PathBuf::from("stream.bin"));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_survives() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![b'/', 0xff, b'x']);
        let d = payload_dir(vec![OsString::from("b"), raw.clone()]).unwrap();
        assert_eq!(d.as_os_str(), raw);
    }

    #[test]
    fn both_payloads_are_read_whole() {
        let t = TestDir::new("both");
        t.write("stream.bin", b"abc");
        t.write("tui.bin", b"\x1b[H");
        let p = Payloads::read(t.path()).unwrap();
        assert_eq!((&p.stream[..], &p.tui[..]), (&b"abc"[..], &b"\x1b[H"[..]));
    }

    #[test]
    fn a_missing_payload_names_its_file() {
        let t = TestDir::new("missing");
        t.write("stream.bin", b"abc");
        let e = Payloads::read(t.path()).unwrap_err();
        assert!(
            matches!(&e, Error::Payload { path, .. } if path.ends_with("tui.bin")),
            "{e:?}"
        );
    }

    #[test]
    fn an_empty_payload_is_refused() {
        let t = TestDir::new("empty");
        t.write("stream.bin", b"");
        t.write("tui.bin", b"x");
        let e = Payloads::read(t.path()).unwrap_err();
        assert!(
            matches!(&e, Error::EmptyPayload { path } if path.ends_with("stream.bin")),
            "{e:?}"
        );
    }
}
