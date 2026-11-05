//! The process environment gikli runs in.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// The environment variables gikli sees, and passes on to every git it runs.
///
/// gikli never reads its own process environment directly: the application builds one with
/// [`HostEnvironment::current`], and tests build their own to control what gikli finds.
#[derive(Debug, Clone, Default)]
pub struct HostEnvironment {
    vars: BTreeMap<OsString, OsString>,
}

impl HostEnvironment {
    /// The environment of the running process.
    pub fn current() -> Self {
        let mut host = Self::default();
        for (name, value) in std::env::vars_os() {
            host.set(name, value);
        }
        host
    }

    /// Sets a variable, replacing any previous value.
    pub fn set(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) {
        self.vars.insert(normalize(name.into()), value.into());
    }

    /// Removes a variable.
    pub fn remove(&mut self, name: impl Into<OsString>) {
        self.vars.remove(&normalize(name.into()));
    }

    pub fn get(&self, name: impl Into<OsString>) -> Option<&OsStr> {
        self.vars
            .get(&normalize(name.into()))
            .map(OsString::as_os_str)
    }

    /// The folders listed in PATH, in order.
    pub(crate) fn path_folders(&self) -> Vec<PathBuf> {
        self.get("PATH")
            .map(|path| std::env::split_paths(path).collect())
            .unwrap_or_default()
    }

    pub(crate) fn vars(&self) -> impl Iterator<Item = (&OsString, &OsString)> {
        self.vars.iter()
    }
}

/// Windows treats variable names case-insensitively (`Path` and `PATH` are one variable).
fn normalize(name: OsString) -> OsString {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name
    }
}
