//! Finding a kitty the user built or installed. Nothing of it is in this
//! repository, and nothing of it is named at build time.

use crate::Error;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// A kitty on disk: the directory Python imports the `kitty` package from, and
/// the extension inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub package_root: PathBuf,
    pub extension: PathBuf,
}

/// Where the package sits under a root, tried in order.
const LAYOUTS: &[&str] = &[
    // The root is the package root itself: a built source tree.
    "",
    // The nix package on macOS, and an app bundle given by its parent or itself.
    "Applications/kitty.app/Contents/Resources/kitty",
    "kitty.app/Contents/Resources/kitty",
    "Contents/Resources/kitty",
    // The nix package on Linux.
    "lib/kitty",
];

const EXTENSION: &str = "kitty/fast_data_types.so";

/// `KITTY_ROOT` when it is set, and then nothing else: a named kitty that is not
/// there never turns into a measurement of a different one. Otherwise `kitty` on
/// `path`, followed through its links, then each of `system`.
pub fn locate(
    kitty_root: Option<&OsStr>,
    path: Option<&OsStr>,
    system: &[PathBuf],
) -> Result<Location, Error> {
    let mut looked = Vec::new();
    if let Some(root) = kitty_root.filter(|r| !r.is_empty()) {
        return under(Path::new(root), &mut looked).ok_or_else(|| Error::NoKitty {
            looked: looked.clone(),
        });
    }
    for dir in path.into_iter().flat_map(std::env::split_paths) {
        let binary = dir.join("kitty");
        if !binary.is_file() {
            continue;
        }
        // Up from the binary, past the link: a package sits above it, not
        // beside it, in every layout.
        let real = binary.canonicalize().unwrap_or(binary);
        if let Some(found) = real.ancestors().skip(1).find_map(|a| under(a, &mut looked)) {
            return Ok(found);
        }
    }
    system
        .iter()
        .find_map(|root| under(root, &mut looked))
        .ok_or(Error::NoKitty { looked })
}

/// The first layout under `root` that holds the extension, as a file.
fn under(root: &Path, looked: &mut Vec<PathBuf>) -> Option<Location> {
    LAYOUTS.iter().find_map(|layout| {
        let package_root = root.join(layout);
        let extension = package_root.join(EXTENSION);
        if extension.is_file() {
            return Some(Location {
                package_root: strip_trailing(package_root),
                extension,
            });
        }
        looked.push(extension);
        None
    })
}

/// `root.join("")` ends in a separator; a package root compares without one.
fn strip_trailing(p: PathBuf) -> PathBuf {
    p.components().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    fn nothing_else() -> Vec<PathBuf> {
        Vec::new()
    }

    #[test]
    fn a_package_root_is_taken_as_it_is() {
        let t = TestDir::new("pkg");
        let so = t.write("kitty/fast_data_types.so", b"");
        let l = locate(Some(t.path().as_os_str()), None, &nothing_else()).unwrap();
        assert_eq!(
            l,
            Location {
                package_root: t.path().to_path_buf(),
                extension: so
            }
        );
    }

    #[test]
    fn each_install_layout_is_found_under_its_root() {
        for (name, layout) in [
            ("nixmac", "Applications/kitty.app/Contents/Resources/kitty"),
            ("parent", "kitty.app/Contents/Resources/kitty"),
            ("bundle", "Contents/Resources/kitty"),
            ("nixlinux", "lib/kitty"),
        ] {
            let t = TestDir::new(name);
            let so = t.write(&format!("{layout}/kitty/fast_data_types.so"), b"");
            let l = locate(Some(t.path().as_os_str()), None, &nothing_else()).unwrap();
            assert_eq!(l.extension, so, "{layout}");
            assert_eq!(l.package_root, t.path().join(layout), "{layout}");
        }
    }

    #[test]
    fn a_named_root_with_nothing_there_does_not_fall_through() {
        let empty = TestDir::new("named-empty");
        // A real kitty on PATH and in the system list, which must not be taken.
        let other = TestDir::new("named-other");
        other.write("lib/kitty/kitty/fast_data_types.so", b"");
        other.write("bin/kitty", b"");
        let path = other.path().join("bin");
        let e = locate(
            Some(empty.path().as_os_str()),
            Some(path.as_os_str()),
            &[other.path().to_path_buf()],
        )
        .unwrap_err();
        let Error::NoKitty { looked } = &e else {
            panic!("{e:?}")
        };
        assert_eq!(looked.len(), LAYOUTS.len(), "{looked:?}");
        assert!(
            looked.iter().all(|p| p.starts_with(empty.path())),
            "{looked:?}"
        );
    }

    #[test]
    fn an_empty_root_variable_is_not_a_root() {
        let t = TestDir::new("empty-var");
        t.write("lib/kitty/kitty/fast_data_types.so", b"");
        let l = locate(Some(OsStr::new("")), None, &[t.path().to_path_buf()]).unwrap();
        assert_eq!(l.package_root, t.path().join("lib/kitty"));
    }

    #[test]
    fn path_finds_the_package_above_the_binary() {
        let t = TestDir::new("on-path");
        let so = t.write("lib/kitty/kitty/fast_data_types.so", b"");
        t.write("bin/kitty", b"");
        let path = t.path().join("bin");
        let l = locate(None, Some(path.as_os_str()), &nothing_else()).unwrap();
        assert_eq!(
            l.extension.canonicalize().unwrap(),
            so.canonicalize().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_follows_a_link_into_the_bundle() {
        let t = TestDir::new("link");
        let so = t.write(
            "store/Applications/kitty.app/Contents/Resources/kitty/kitty/fast_data_types.so",
            b"",
        );
        let exe = t.write("store/Applications/kitty.app/Contents/MacOS/kitty", b"");
        let bin = t.path().join("profile/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&exe, bin.join("kitty")).unwrap();
        let l = locate(None, Some(bin.as_os_str()), &nothing_else()).unwrap();
        assert_eq!(
            l.extension.canonicalize().unwrap(),
            so.canonicalize().unwrap()
        );
    }

    #[test]
    fn the_first_path_entry_that_has_one_wins() {
        let a = TestDir::new("first-a");
        let b = TestDir::new("first-b");
        a.write("nothing", b"");
        let so = b.write("lib/kitty/kitty/fast_data_types.so", b"");
        b.write("bin/kitty", b"");
        let path = std::env::join_paths([a.path().to_path_buf(), b.path().join("bin")]).unwrap();
        let l = locate(None, Some(path.as_os_str()), &nothing_else()).unwrap();
        assert_eq!(
            l.extension.canonicalize().unwrap(),
            so.canonicalize().unwrap()
        );
    }

    #[test]
    fn the_system_list_comes_after_path() {
        let t = TestDir::new("system");
        let so = t.write(
            "kitty.app/Contents/Resources/kitty/kitty/fast_data_types.so",
            b"",
        );
        let l = locate(
            None,
            Some(OsStr::new("/nonexistent")),
            &[t.path().join("kitty.app")],
        )
        .unwrap();
        assert_eq!(l.extension, so);
    }

    #[test]
    fn nothing_found_says_where_it_looked_and_how_to_name_one() {
        let e = locate(None, None, &[PathBuf::from("/nonexistent/kitty.app")]).unwrap_err();
        let s = e.to_string();
        assert!(s.contains("KITTY_ROOT"), "{s}");
        assert!(s.contains("/nonexistent/kitty.app"), "{s}");
    }

    #[test]
    fn a_directory_where_the_extension_should_be_is_not_one() {
        let t = TestDir::new("dir");
        std::fs::create_dir_all(t.path().join("kitty/fast_data_types.so")).unwrap();
        assert!(locate(Some(t.path().as_os_str()), None, &nothing_else()).is_err());
    }
}
