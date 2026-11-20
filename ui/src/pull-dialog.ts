// The pull dialog: the Repository and its current branch, the remote and branch to pull from, and how
// to integrate it. The core picks the source the dialog starts from, remembers another one the user
// pulls from, and pulls; this draws the dialog and keeps contradicting options from being ticked
// together.

import { pull, pullPlan, type PullOptions, type PullPlan, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t, type StringKey } from "./strings";

/** What the pull dialog needs from the window around it. */
export interface PullDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project in view, or null when there is none. */
  project(): string | null;
  /** The pull moved HEAD of `repository`, or left it half done. */
  branchesMoved(repository: Repository): void;
}

export interface PullDialog {
  /** Opens the dialog on the current branch of `repository`. */
  open(repository: Repository): void;
  /** Closes the dialog. */
  close(): void;
}

type OptionName = keyof PullOptions;

/** The options in the order the dialog shows them, each with its label and what it does. */
const OPTIONS: { name: OptionName; label: StringKey; hint: StringKey }[] = [
  { name: "rebase", label: "pullRebase", hint: "pullRebaseHint" },
  { name: "ffOnly", label: "pullFfOnly", hint: "pullFfOnlyHint" },
  { name: "noFf", label: "pullNoFf", hint: "pullNoFfHint" },
  { name: "squash", label: "pullSquash", hint: "pullSquashHint" },
  { name: "noCommit", label: "pullNoCommit", hint: "pullNoCommitHint" },
  { name: "noVerify", label: "pullNoVerify", hint: "pullNoVerifyHint" },
];

/** The options that contradict each other, as the core refuses them: one ticked keeps the other from being ticked. */
const CONTRADICTIONS: [OptionName, OptionName][] = [
  ["rebase", "ffOnly"],
  ["rebase", "noFf"],
  ["rebase", "squash"],
  ["rebase", "noCommit"],
  ["ffOnly", "noFf"],
  ["noFf", "squash"],
];

export function createPullDialog(host: PullDialogHost): PullDialog {
  const view = element<HTMLElement>("pull-dialog");
  const title = element<HTMLElement>("pull-title");
  const remoteSelect = element<HTMLSelectElement>("pull-remote");
  const branchInput = element<HTMLInputElement>("pull-branch");
  const branchList = element<HTMLDataListElement>("pull-branches");
  const optionsView = element<HTMLElement>("pull-options");
  const message = element<HTMLElement>("pull-message");
  const pullButton = element<HTMLButtonElement>("pull-start");
  const cancelButton = element<HTMLButtonElement>("pull-cancel");

  let repository: Repository | null = null;
  let plan: PullPlan | null = null;
  /** Tells the answer to the latest read from one that a later one overtook. */
  let latestRead = 0;

  element<HTMLElement>("pull-remote-label").textContent = t("pullRemote");
  element<HTMLElement>("pull-branch-label").textContent = t("pullBranch");
  pullButton.textContent = t("pullStart");
  cancelButton.textContent = t("pullCancel");
  const checks = new Map<OptionName, HTMLInputElement>();
  optionsView.replaceChildren(
    ...OPTIONS.map(({ name, label, hint }) => {
      const check = document.createElement("input");
      check.type = "checkbox";
      check.dataset.testid = "pull-option";
      check.dataset.option = name;
      check.addEventListener("change", allowOptions);
      checks.set(name, check);
      const text = document.createElement("span");
      text.textContent = t(label);
      const option = document.createElement("label");
      option.className = "pull-option";
      option.title = t(hint);
      option.append(check, text);
      return option;
    }),
  );

  cancelButton.addEventListener("click", close);
  pullButton.addEventListener("click", () => void startPull());
  remoteSelect.addEventListener("change", () => {
    listBranches();
    allowPull();
  });
  branchInput.addEventListener("input", allowPull);
  branchInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void startPull();
    }
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(toShow: Repository): void {
    repository = toShow;
    plan = null;
    title.textContent = t("pullTitle", { branch: "…", name: folderName(toShow.root) });
    title.title = toShow.root;
    remoteSelect.replaceChildren();
    branchInput.value = "";
    branchList.replaceChildren();
    for (const check of checks.values()) {
      check.checked = false;
    }
    allowOptions();
    message.textContent = t("pullReading");
    pullButton.disabled = true;
    view.hidden = false;
    void read(toShow);
  }

  function close(): void {
    repository = null;
    plan = null;
    latestRead += 1;
    view.hidden = true;
  }

  async function read(shown: Repository): Promise<void> {
    const thisRead = ++latestRead;
    try {
      const read = await pullPlan(shown);
      if (thisRead !== latestRead) {
        return;
      }
      plan = read;
      title.textContent = t("pullTitle", { branch: read.branch, name: folderName(shown.root) });
      remoteSelect.replaceChildren(...read.remotes.map((remote) => new Option(remote.name, remote.name)));
      remoteSelect.value = read.remote ?? "";
      branchInput.value = read.sourceBranch ?? "";
      listBranches();
      message.textContent = read.remotes.length === 0 ? t("pullNoRemote") : "";
      allowPull();
      branchInput.focus();
    } catch (error) {
      if (thisRead === latestRead) {
        message.replaceChildren(host.errorView(error));
      }
    }
  }

  /** Offers the branches of the remote chosen, as it had them when it was last fetched. */
  function listBranches(): void {
    const remote = plan?.remotes.find((candidate) => candidate.name === remoteSelect.value);
    branchList.replaceChildren(...(remote?.branches ?? []).map((branch) => new Option(branch)));
  }

  /** Keeps each option that contradicts one ticked from being ticked too. */
  function allowOptions(): void {
    for (const [name, check] of checks) {
      check.disabled = CONTRADICTIONS.some(
        ([first, second]) => (first === name && checks.get(second)?.checked) || (second === name && checks.get(first)?.checked),
      );
    }
  }

  function allowPull(): void {
    pullButton.disabled = plan === null || remoteSelect.value === "" || branchInput.value.trim() === "";
  }

  function chosenOptions(): PullOptions {
    const ticked = (name: OptionName): boolean => checks.get(name)?.checked ?? false;
    return {
      rebase: ticked("rebase"),
      ffOnly: ticked("ffOnly"),
      noFf: ticked("noFf"),
      squash: ticked("squash"),
      noCommit: ticked("noCommit"),
      noVerify: ticked("noVerify"),
    };
  }

  async function startPull(): Promise<void> {
    const shown = repository;
    const project = host.project();
    if (shown === null || project === null || pullButton.disabled) {
      return;
    }
    const source = { remote: remoteSelect.value, branch: branchInput.value.trim() };
    pullButton.disabled = true;
    message.textContent = t("pullPulling", { source: `${source.remote}/${source.branch}` });
    try {
      // A pull that stops on a conflict opens the conflicts dialog, and one left half done shows in the status area.
      await pull(project, shown, source, chosenOptions());
      host.branchesMoved(shown);
      if (repository === shown) {
        close();
      }
    } catch (error) {
      host.branchesMoved(shown);
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
        allowPull();
      }
    }
  }

  return { open, close };
}
