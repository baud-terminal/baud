//! **The environment a child is given, one entry a name**, as `getenv`
//! reads it: the first entry of a name is the one a program sees, so a
//! parent's `LANG=` then `LANG=en_US.UTF-8` is an empty `LANG` to the
//! child, and asking whether "any" entry names a locale answered for a
//! value the child never reads. [`ChildEnv::of`] keeps the first entry of
//! each name and drops the rest, and an entry whose name is empty or holds
//! `=` or a NUL, which no environment can carry; every later change is
//! [`ChildEnv::set`] or [`ChildEnv::remove`] of a name, so the rule is kept
//! in one place rather than by each caller filtering a list by hand. The
//! list becomes what `execve` takes once, in the parent before the fork
//! ([`ChildEnv::into_envp`]).
#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;

/// **The name of an environment variable**: not empty, and no `=` or NUL,
/// so `name=value` splits back at its first `=` into the same name. Made
/// at compile time for the names baud writes ([`EnvName::fixed`], in a
/// `const`), and checked for a parent's ([`EnvName::of`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnvName(Cow<'static, [u8]>);

impl EnvName {
    /// A name baud writes, for a `const`: one that is empty or holds `=`
    /// or a NUL fails the build there.
    pub(crate) const fn fixed(name: &'static str) -> EnvName {
        let b = name.as_bytes();
        assert!(!b.is_empty(), "an environment name is not empty");
        let mut i = 0;
        while i < b.len() {
            assert!(
                b[i] != b'=' && b[i] != 0,
                "an environment name has no = and no NUL"
            );
            i += 1;
        }
        EnvName(Cow::Borrowed(b))
    }

    /// `name` as a name, none when it is empty or holds `=` or a NUL.
    pub(crate) fn of(name: &OsStr) -> Option<EnvName> {
        let b = name.as_bytes();
        (!b.is_empty() && !b.iter().any(|c| *c == b'=' || *c == 0))
            .then(|| EnvName(Cow::Owned(b.to_vec())))
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(&self.0)
    }
}

/// `LANG`, which a locale is filled into.
pub(crate) const LANG: EnvName = EnvName::fixed("LANG");
/// `LC_ALL`, read before every category.
pub(crate) const LC_ALL: EnvName = EnvName::fixed("LC_ALL");
/// `LC_CTYPE`, the category that decides how bytes become characters.
pub(crate) const LC_CTYPE: EnvName = EnvName::fixed("LC_CTYPE");
/// `PWD`, set to where a child sent home starts.
pub(crate) const PWD: EnvName = EnvName::fixed("PWD");
/// `SHELL`, set to a configured program.
pub(crate) const SHELL: EnvName = EnvName::fixed("SHELL");

/// **A child's environment**: one entry a name, in the order the names
/// first came or were last set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChildEnv(Vec<(EnvName, OsString)>);

impl ChildEnv {
    /// `parent` as a child would read it: the first entry of each name,
    /// the rest dropped as `getenv` never sees them, and an entry with an
    /// empty name, a name holding `=` or a NUL, or a value holding a NUL,
    /// dropped as no environment can carry it.
    pub(crate) fn of(parent: Vec<(OsString, OsString)>) -> ChildEnv {
        let mut env: Vec<(EnvName, OsString)> = Vec::with_capacity(parent.len());
        for (k, v) in parent {
            let Some(name) = EnvName::of(&k) else {
                continue;
            };
            if v.as_bytes().contains(&0) || env.iter().any(|(n, _)| *n == name) {
                continue;
            }
            env.push((name, v));
        }
        ChildEnv(env)
    }

    /// The value of `name`.
    pub(crate) fn get(&self, name: &EnvName) -> Option<&OsStr> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_os_str())
    }

    /// `name` set to `value`, in place of any entry of its name, at the
    /// end. A value holding a NUL, which no environment can carry, leaves
    /// the name unset.
    pub(crate) fn set(&mut self, name: &EnvName, value: impl Into<OsString>) {
        self.remove(name);
        let value = value.into();
        if !value.as_bytes().contains(&0) {
            self.0.push((name.clone(), value));
        }
    }

    /// `name` unset.
    pub(crate) fn remove(&mut self, name: &EnvName) {
        self.0.retain(|(n, _)| n != name);
    }

    /// Every entry whose name `keep` admits, the rest unset.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&EnvName) -> bool) {
        self.0.retain(|(n, _)| keep(n));
    }

    /// The `name=value` strings `execve` takes, in order: built in the
    /// parent, so the child allocates nothing between `fork` and `exec`.
    pub(crate) fn into_envp(self) -> Vec<CString> {
        self.0
            .into_iter()
            .filter_map(|(n, v)| {
                let mut entry = n.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(v.as_bytes());
                CString::new(entry).ok()
            })
            .collect()
    }
}
