//! The Changelist store: the Changelists of a Repository, kept as versioned JSON in its git
//! directory (ADR-0005), and how they follow what happens in git.

use std::collections::BTreeSet;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Failure;
use crate::git::{GitRunner, path_from_bytes};
use crate::project::write_at_once;
use crate::status::{Change, ChangeStatus, Status};

/// The version of the format this gikli writes. Whatever changes the format raises it, and gikli
/// goes on reading every earlier version.
const FORMAT_VERSION: u32 = 2;

/// The identifier of the Changelist every Repository starts with. It is the same in each
/// Repository, so the initial Changelists of several Repositories are parts of one (ADR-0005).
const INITIAL_ID: &str = "changes";

/// The Changelists of one Repository, and which is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Changelists {
    version: u32,
    /// The identifier of the Active changelist.
    active: String,
    /// The commit HEAD was on the last time the Changelists followed git, or None when it was on no
    /// commit yet. It tells what was committed since then, even with gikli closed.
    head: Option<String>,
    changelists: Vec<StoredChangelist>,
}

/// One Changelist, with the changed files that belong to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredChangelist {
    pub id: String,
    pub name: String,
    /// What the user wrote about it, if anything. Version 1 of the format did not have it.
    pub description: Option<String>,
    /// Relative to the Repository root, as git writes them. A path that is not UTF-8 is kept in
    /// its lossy form: it still names the same file from one read to the next.
    files: BTreeSet<String>,
}

/// The part of the format every version has.
#[derive(Deserialize)]
struct FormatVersion {
    version: u32,
}

impl Changelists {
    /// What a Repository starts with: one Changelist named Changes, which is active.
    fn initial() -> Self {
        Self {
            version: FORMAT_VERSION,
            active: INITIAL_ID.to_owned(),
            head: None,
            changelists: vec![StoredChangelist {
                id: INITIAL_ID.to_owned(),
                name: "Changes".to_owned(),
                description: None,
                files: BTreeSet::new(),
            }],
        }
    }

    pub fn lists(&self) -> &[StoredChangelist] {
        &self.changelists
    }

    /// The identifier of the Active changelist.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Makes the Changelist `id` the Active changelist.
    pub fn set_active(&mut self, id: &str) -> Result<(), Failure> {
        self.place_with_id(id)?;
        id.clone_into(&mut self.active);
        Ok(())
    }

    /// Calls the Changelist `id` `name` from here on.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), Failure> {
        let place = self.place_with_id(id)?;
        let name = self.usable_name(name, Some(id))?.to_owned();
        self.changelists[place].name = name;
        Ok(())
    }

    /// `name` as a Changelist goes by it: without the spaces around it, and not already the name of
    /// a Changelist other than `renamed`, the identifier of the one getting it.
    fn usable_name<'n>(&self, name: &'n str, renamed: Option<&str>) -> Result<&'n str, Failure> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Failure::ChangelistNameEmpty);
        }
        if self
            .changelists
            .iter()
            .any(|list| list.name == name && Some(list.id.as_str()) != renamed)
        {
            return Err(Failure::ChangelistNameTaken {
                name: name.to_owned(),
            });
        }
        Ok(name)
    }

    fn place_with_id(&self, id: &str) -> Result<usize, Failure> {
        self.changelists
            .iter()
            .position(|list| list.id == id)
            .ok_or_else(|| Failure::ChangelistGone { id: id.to_owned() })
    }

    /// Removes the Changelist `id`. Its changes go to the Active changelist.
    pub fn delete(&mut self, id: &str) -> Result<(), Failure> {
        let place = self.place_with_id(id)?;
        if id == self.active {
            return Err(Failure::ActiveChangelistNotDeletable);
        }
        let deleted = self.changelists.remove(place);
        let active = self.active_place();
        self.changelists[active].files.extend(deleted.files);
        Ok(())
    }

    /// Adds a Changelist called `name`, after the others.
    pub fn create(&mut self, name: &str, description: Option<&str>) -> Result<(), Failure> {
        let name = self.usable_name(name, None)?.to_owned();
        self.changelists.push(StoredChangelist {
            id: self.new_id(),
            name,
            description: description
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .map(str::to_owned),
            files: BTreeSet::new(),
        });
        Ok(())
    }

    /// An identifier no Changelist has. It is random so that it is not used again later: a window
    /// still showing a deleted Changelist must not end up acting on a new one.
    fn new_id(&self) -> String {
        loop {
            let id = format!("{:016x}", RandomState::new().build_hasher().finish());
            if !self.changelists.iter().any(|list| list.id == id) {
                return id;
            }
        }
    }

    /// The place in [`Self::lists`] of the Changelist the changed file at `path` belongs to.
    pub fn place_of(&self, path: &Path) -> usize {
        self.place_holding(path)
            .unwrap_or_else(|| self.active_place())
    }

    fn place_holding(&self, path: &Path) -> Option<usize> {
        let file = path.to_string_lossy();
        self.changelists
            .iter()
            .position(|list| list.files.contains(file.as_ref()))
    }

    fn active_place(&self) -> usize {
        self.changelists
            .iter()
            .position(|list| list.id == self.active)
            .expect("the store checks that the Active changelist is one of the Changelists")
    }

    /// Follows `status`, what git reports now, running git with `runner` in the Repository at `root`
    /// when HEAD moved: a file that is no longer changed leaves its Changelist, and so does a file
    /// committed since the last time, even when it changed again. A changed file stays in the
    /// Changelist it is in, including when it moved, and a new change goes to the Active changelist.
    pub fn follow(&mut self, status: &Status, runner: &GitRunner, root: &Path) {
        if self.head != status.head {
            let committed = self.committed_since(status.head.as_deref(), runner, root);
            for list in &mut self.changelists {
                list.files.retain(|file| !committed.contains(file));
            }
            self.head.clone_from(&status.head);
        }
        let mut files = vec![BTreeSet::new(); self.changelists.len()];
        for change in status
            .changes
            .iter()
            .filter(|change| change.status.is_in_changelists())
        {
            files[self.place_for(change)].insert(change.path.to_string_lossy().into_owned());
        }
        for (list, files) in self.changelists.iter_mut().zip(files) {
            list.files = files;
        }
    }

    /// The place in [`Self::lists`] of the Changelist `change` goes into. A moved file that is not
    /// in one yet takes the place of the file it was. A copy is a new file.
    fn place_for(&self, change: &Change) -> usize {
        let was_in = change
            .original_path
            .as_deref()
            .filter(|_| change.status == ChangeStatus::Renamed)
            .and_then(|original| self.place_holding(original));
        self.place_holding(&change.path)
            .or(was_in)
            .unwrap_or_else(|| self.active_place())
    }

    /// The files committed since [`Self::head`], now that HEAD is on `new_head`: those that differ
    /// between the two commits. Going back in history commits nothing, and neither does rewording a
    /// commit. When git cannot say, for example because the commit is gone, there are none: the
    /// files stay where they are.
    fn committed_since(
        &self,
        new_head: Option<&str>,
        runner: &GitRunner,
        root: &Path,
    ) -> BTreeSet<String> {
        let (Some(new_head), true) = (new_head, self.holds_files()) else {
            return BTreeSet::new();
        };
        let invocation = match &self.head {
            Some(head) => {
                let went_back = runner
                    .run(root, &["merge-base", "--is-ancestor", new_head, head])
                    .is_ok_and(|invocation| invocation.success);
                if went_back {
                    return BTreeSet::new();
                }
                runner.run(
                    root,
                    &["diff", "--name-only", "-z", "--no-renames", head, new_head],
                )
            }
            // Everything in the first commit came into the history with it.
            None => runner.run(
                root,
                &[
                    "ls-tree",
                    "-r",
                    "--full-tree",
                    "--name-only",
                    "-z",
                    new_head,
                ],
            ),
        };
        match invocation {
            Ok(invocation) if invocation.success => invocation
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| path_from_bytes(path).to_string_lossy().into_owned())
                .collect(),
            _ => BTreeSet::new(),
        }
    }

    fn holds_files(&self) -> bool {
        self.changelists.iter().any(|list| !list.files.is_empty())
    }
}

/// Where the Changelists of one Repository, or of one linked worktree, are kept.
pub(crate) struct ChangelistStore {
    file: PathBuf,
}

impl ChangelistStore {
    /// The store in `git_dir`, which is the git directory of the working tree.
    pub fn in_git_dir(git_dir: &Path) -> Self {
        Self {
            file: git_dir.join("gikli").join("changelists.json"),
        }
    }

    /// Reads the Changelists. A Repository without any starts with [`Changelists::initial`]. When
    /// they cannot be used, the file stays as it is: the user may want what it holds.
    pub fn load(&self) -> Result<Changelists, Failure> {
        let json = match fs::read(&self.file) {
            Ok(json) => json,
            // Something in the way of the folder is no Changelists either. Saving comes up against it.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(Changelists::initial());
            }
            Err(error) => return Err(self.unreadable(error)),
        };
        let FormatVersion { version } =
            serde_json::from_slice(&json).map_err(|error| self.unreadable(error))?;
        if version > FORMAT_VERSION {
            return Err(Failure::ChangelistsFromNewerGikli {
                file: self.file.clone(),
                version,
            });
        }
        let mut lists: Changelists =
            serde_json::from_slice(&json).map_err(|error| self.unreadable(error))?;
        lists.version = FORMAT_VERSION;
        if !lists.changelists.iter().any(|list| list.id == lists.active) {
            return Err(Failure::ActiveChangelistMissing {
                file: self.file.clone(),
            });
        }
        Ok(lists)
    }

    /// Writes the Changelists, replacing the file at once.
    pub fn save(&self, lists: &Changelists) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(lists).map_err(io::Error::other)?;
        write_at_once(&self.file, &json)
    }

    /// The failure of not being able to save them because of `error`.
    pub fn not_saved(&self, error: &io::Error) -> Failure {
        Failure::ChangelistsNotSaved {
            file: self.file.clone(),
            message: error.to_string(),
        }
    }

    fn unreadable(&self, error: impl ToString) -> Failure {
        Failure::ChangelistsUnreadable {
            file: self.file.clone(),
            message: error.to_string(),
        }
    }
}
