//! Folders of WSL distributions as Windows sees them.
//!
//! Windows sees the files of a distribution under `\\wsl$\<distribution>` or
//! `\\wsl.localhost\<distribution>`. git inside WSL reads Linux paths, so gikli hands it the
//! Linux form of such a folder.

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
        let Some(Component::Prefix(prefix)) = components.next() else {
            return None;
        };
        let (Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share)) = prefix.kind()
        else {
            return None;
        };
        let server = server.to_string_lossy();
        let is_wsl =
            server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost");
        if !is_wsl {
            return None;
        }
        let mut names: Vec<String> = Vec::new();
        for component in components {
            match component {
                Component::Normal(name) => names.push(name.to_string_lossy().into_owned()),
                Component::ParentDir => {
                    names.pop();
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            }
        }
        let share = Self {
            server: server.into_owned(),
            distribution: share.to_string_lossy().into_owned(),
        };
        Some((share, format!("/{}", names.join("/"))))
    }

    /// `linux_path` inside the distribution, as Windows sees it through this share.
    pub fn windows_path(&self, linux_path: &str) -> PathBuf {
        let mut path = format!(r"\\{}\{}\", self.server, self.distribution);
        let names: Vec<&str> = linux_path
            .split('/')
            .filter(|name| !name.is_empty())
            .collect();
        path.push_str(&names.join(r"\"));
        PathBuf::from(path)
    }
}
