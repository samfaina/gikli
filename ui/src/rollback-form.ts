// The form that lists what a rollback would discard, and discards it once the user confirms.

import { isFailure, rollbackFiles, rollbackPlan, stagingRollbackPlan, type Change, type RollbackPlan, type Repository, type StagingSide } from "./core";
import { t } from "./strings";

/** What the form needs from the view of the Repository around it. */
export interface RollbackFormHost {
  repository: Repository;
  /** What to say about a change besides its path. */
  describeChange(change: Change): string;
  /** The rollback ran, so the changes need reading again. */
  rolledBack(): void;
  /** What to say about an error from the core. */
  describeError(error: unknown): string;
}

/**
 * A form that confirms a rollback, hidden until something opens it. Only its button runs the rollback:
 * it is no submit button, so that Enter in the form cannot, and the focus starts on Cancel.
 */
export function createRollbackForm(host: RollbackFormHost): { view: HTMLElement; open: (paths: string[], side?: StagingSide | null) => void } {
  const { repository } = host;
  const view = document.createElement("div");
  view.className = "changelist-form";
  view.dataset.testid = "rollback-form";
  view.hidden = true;
  const title = document.createElement("h3");
  title.textContent = t("rollbackTitle");
  const explanation = document.createElement("p");
  explanation.textContent = t("rollbackExplanation");
  const files = document.createElement("ul");
  files.className = "rollback-files";

  const deleteAdded = document.createElement("input");
  deleteAdded.type = "checkbox";
  deleteAdded.dataset.testid = "rollback-delete-added";
  const deleteAddedLabel = document.createElement("label");
  deleteAddedLabel.className = "changelist-form-check";
  deleteAddedLabel.append(deleteAdded, document.createTextNode(t("rollbackDeleteAdded")));

  const message = document.createElement("p");
  message.dataset.testid = "rollback-message";
  const confirm = document.createElement("button");
  confirm.type = "button";
  confirm.dataset.testid = "rollback-confirm";
  confirm.textContent = t("rollbackConfirm");
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.dataset.testid = "rollback-cancel";
  cancel.textContent = t("changelistFormCancel");
  const buttons = document.createElement("div");
  buttons.className = "notification-actions";
  buttons.append(confirm, cancel);
  view.append(title, explanation, files, deleteAddedLabel, message, buttons);

  /** What the user is confirming, once the core has said. */
  let plan: RollbackPlan | null = null;
  /** The paths the user selected, which the list is about. */
  let selection: string[] = [];
  /** The side of the index the selection is on in Staging mode. */
  let selectionSide: StagingSide | null = null;
  /** Counts the times the list was asked for, so a late answer about an earlier selection is dropped. */
  let requests = 0;

  const show = (shown: RollbackPlan | null, text: string): void => {
    plan = shown;
    files.replaceChildren(
      ...(shown?.files ?? []).map((change) => {
        const item = document.createElement("li");
        item.dataset.testid = "rollback-file";
        item.dataset.path = change.path;
        item.textContent = t("rollbackFile", { path: change.path, description: host.describeChange(change) });
        return item;
      }),
    );
    deleteAddedLabel.hidden = !(shown?.hasAddedFiles ?? false);
    deleteAdded.checked = false;
    message.textContent = text;
    confirm.hidden = shown === null || shown.files.length === 0;
  };

  /** Shows what a rollback of the selection would discard now, with `note` when there is something to say. */
  const load = async (note: string): Promise<void> => {
    requests += 1;
    const thisRequest = requests;
    try {
      const found = await (selectionSide === null ? rollbackPlan(repository, selection) : stagingRollbackPlan(repository, selection, selectionSide));
      if (thisRequest === requests) {
        show(found, found.files.length === 0 ? t("rollbackNothing") : note);
      }
    } catch (error) {
      if (thisRequest === requests) {
        show(null, host.describeError(error));
      }
    }
  };

  const close = (): void => {
    requests += 1;
    plan = null;
    view.hidden = true;
  };
  cancel.addEventListener("click", close);
  confirm.addEventListener("click", async () => {
    if (plan === null) {
      return;
    }
    confirm.disabled = true;
    message.textContent = "";
    try {
      await rollbackFiles(repository, plan, deleteAdded.checked);
      close();
      host.rolledBack();
    } catch (error) {
      if (isFailure(error) && error.kind === "rollbackListOutdated") {
        await load(host.describeError(error));
      } else {
        message.textContent = host.describeError(error);
      }
    } finally {
      confirm.disabled = false;
    }
  });

  return {
    view,
    open: (paths, side = null) => {
      selection = paths;
      selectionSide = side;
      show(null, "");
      view.hidden = false;
      cancel.focus();
      void load("");
    },
  };
}
