// The core's application interface, as the UI sees it. Types mirror crates/gikli-core.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface GitVersion {
  major: number;
  minor: number;
  patch: number;
}

/** Where gikli found git: the path in the settings, PATH, or WSL (then `executable` is `wsl.exe`). */
export type GitSource = "configured" | "path" | "wsl";

export type GitCheck =
  | { status: "ready"; executable: string; source: GitSource; version: GitVersion }
  | { status: "tooOld"; executable: string; source: GitSource; version: GitVersion; minimum: GitVersion }
  | { status: "unrecognized"; executable: string; source: GitSource }
  | { status: "missing"; executable: string }
  | { status: "notFound"; minimum: GitVersion };

export interface Settings {
  /** The git executable to use; null when gikli looks for git itself. */
  gitPath: string | null;
}

/** A git feature that needs a newer git than the minimum. */
export type Capability = "rebaseUpdateRefs";

export interface CapabilityState {
  capability: Capability;
  /** The first git version that has it. */
  required: GitVersion;
  available: boolean;
}

export interface Repository {
  root: string;
}

export interface Project {
  folder: string;
  repositories: Repository[];
}

export type ChangeStatus = "modified" | "added" | "deleted" | "untracked";

export interface Change {
  /** Relative to the Repository root, with `/` between folders. */
  path: string;
  status: ChangeStatus;
}

export interface ChangeSet {
  repository: Repository;
  /** Ordered by path. */
  changes: Change[];
}

export type Failure =
  | { kind: "gitNotFound" }
  | { kind: "gitTooOld"; version: GitVersion; minimum: GitVersion }
  | { kind: "gitUnrecognized" }
  | { kind: "gitFailedToStart"; message: string }
  | { kind: "settingsNotSaved"; message: string }
  | { kind: "folderNotFound"; folder: string }
  | { kind: "notInRepository"; folder: string }
  | { kind: "changesUnreadable"; folder: string };

export interface OperationLogEntry {
  folder: string;
  args: string[];
  exitCode: number | null;
  stdout: string;
  stderr: string;
}

export type CoreEvent = { event: "operationLogged"; payload: OperationLogEntry };

export function checkGit(): Promise<GitCheck> {
  return invoke("check_git");
}

/** Checks the git at `executable` without using it. */
export function testGit(executable: string): Promise<GitCheck> {
  return invoke("test_git", { executable });
}

/** Sets the path to git, or clears it with null, and looks for git again. Rejects with a {@link Failure}. */
export function setGitPath(path: string | null): Promise<GitCheck> {
  return invoke("set_git_path", { path });
}

/** The application settings. */
export function settings(): Promise<Settings> {
  return invoke("settings");
}

/** What the git that runs in `repository` can do beyond the minimum. Rejects with a {@link Failure}. */
export function capabilities(repository: Repository): Promise<CapabilityState[]> {
  return invoke("capabilities", { repository });
}

/** Rejects with a {@link Failure}. */
export function openProject(folder: string): Promise<Project> {
  return invoke("open_project", { folder });
}

/** Reads the uncommitted changes of `repository`; calling it again refreshes them. Rejects with a {@link Failure}. */
export function readChanges(repository: Repository): Promise<ChangeSet> {
  return invoke("read_changes", { repository });
}

/** The folder passed on the command line, if any. */
export function startupFolder(): Promise<string | null> {
  return invoke("startup_folder");
}

export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("gikli://event", (message) => handler(message.payload));
}

export function isFailure(value: unknown): value is Failure {
  return typeof value === "object" && value !== null && "kind" in value;
}

export function formatVersion(version: GitVersion): string {
  return `${version.major}.${version.minor}.${version.patch}`;
}
