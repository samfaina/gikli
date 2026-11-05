//! git versions.

use std::fmt;

use serde::Serialize;

/// The version of a git installation, as `major.minor.patch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl GitVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Reads the output of `git --version`, such as `git version 2.39.3 (Apple Git-145)` or
    /// `git version 2.51.1.windows.1`.
    pub(crate) fn parse(output: &str) -> Option<Self> {
        let number = output.trim().strip_prefix("git version ")?;
        let mut parts = number
            .split(|c: char| !c.is_ascii_digit())
            .map(|part| part.parse::<u32>());
        let major = parts.next()?.ok()?;
        let minor = parts.next()?.ok()?;
        let patch = parts.next().and_then(Result::ok).unwrap_or(0);
        Some(Self::new(major, minor, patch))
    }
}

impl fmt::Display for GitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
