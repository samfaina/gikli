//! What a git version can do beyond the minimum gikli needs.

use serde::Serialize;

use crate::GitVersion;

/// A git feature that needs a newer git than [`crate::MINIMUM_GIT_VERSION`]. A gikli feature
/// that uses one asks [`crate::Gikli::capabilities`] first, and is shown disabled, with the
/// required version, when the git in use is older.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// `git rebase --update-refs`: a rebase also moves the branches that point into the
    /// rebased commits.
    RebaseUpdateRefs,
}

impl Capability {
    /// Every capability gikli knows.
    pub const ALL: &[Capability] = &[Capability::RebaseUpdateRefs];

    /// The first git version that has this capability.
    pub const fn required(self) -> GitVersion {
        match self {
            Capability::RebaseUpdateRefs => GitVersion::new(2, 38, 0),
        }
    }
}

/// Whether the git in use has a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityState {
    pub capability: Capability,
    /// The first git version that has it.
    pub required: GitVersion,
    pub available: bool,
}

/// What git `version` can do, for every capability gikli knows.
pub(crate) fn of(version: GitVersion) -> Vec<CapabilityState> {
    Capability::ALL
        .iter()
        .map(|&capability| CapabilityState {
            capability,
            required: capability.required(),
            available: version >= capability.required(),
        })
        .collect()
}
