//! Paths between Windows and WSL.
//!
//! git inside WSL reads and prints Linux paths. A Windows drive appears under `/mnt/<letter>`,
//! and the files of a distribution appear in Windows under `\\wsl$\<distribution>` or
//! `\\wsl.localhost\<distribution>`.

use std::path::{Component, Path, PathBuf, Prefix};

/// The share through which Windows sees the files of a WSL distribution:
/// `\\<server>\<distribution>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WslShare {
    /// `wsl$` or `wsl.localhost`, kept as the user wrote it.
    pub server: String,
    pub distribution: String,
}

impl WslShare {
    /// The share `folder` is under, with the same folder inside the distribution (such as
    /// `/home/user/app`), if `folder` is under `\\wsl$` or `\\wsl.localhost`.
    pub fn of(folder: &Path) -> Option<(Self, String)> {
        let mut components = folder.components();
        let (server, share) = match components.next()? {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    (server.to_str()?, share.to_str()?)
                }
                _ => return None,
            },
            _ => return None,
        };
        let is_wsl =
            server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost");
        if !is_wsl {
            return None;
        }
        let mut linux_path = String::new();
        for component in components {
            if let Component::Normal(name) = component {
                linux_path.push('/');
                linux_path.push_str(name.to_str()?);
            }
        }
        if linux_path.is_empty() {
            linux_path.push('/');
        }
        let share = Self {
            server: server.to_owned(),
            distribution: share.to_owned(),
        };
        Some((share, linux_path))
    }
}

/// The Windows path of `linux_path`, a path printed by git inside WSL. A path under
/// `/mnt/<letter>` is on that Windows drive; any other is inside the distribution of `share`
/// when known, and is returned unchanged otherwise.
pub(crate) fn windows_path(linux_path: &str, share: Option<&WslShare>) -> PathBuf {
    if let Some(drive_path) = drive_path(linux_path) {
        return drive_path;
    }
    match share {
        Some(share) => {
            let mut path = format!(r"\\{}\{}", share.server, share.distribution);
            for name in linux_path.split('/').filter(|name| !name.is_empty()) {
                path.push('\\');
                path.push_str(name);
            }
            PathBuf::from(path)
        }
        None => PathBuf::from(linux_path),
    }
}

/// `/mnt/c/Users/app` as `C:\Users\app`.
fn drive_path(linux_path: &str) -> Option<PathBuf> {
    let rest = linux_path.strip_prefix("/mnt/")?;
    let (letter, rest) = match rest.split_once('/') {
        Some((letter, rest)) => (letter, rest),
        None => (rest, ""),
    };
    let mut chars = letter.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next().is_some() {
        return None;
    }
    let mut path = format!("{}:\\", drive.to_ascii_uppercase());
    path.push_str(&rest.replace('/', "\\"));
    Some(PathBuf::from(path))
}
