// The core's application interface, as the UI sees it. Types mirror crates/gikli-core.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface GitVersion {
  major: number;
  minor: number;
  patch: number;
}

export type GitCheck =
  | { status: "ready"; executable: string; version: GitVersion }
  | { status: "tooOld"; executable: string; version: GitVersion; minimum: GitVersion }
  | { status: "unrecognized"; executable: string }
  | { status: "notFound"; minimum: GitVersion };

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
