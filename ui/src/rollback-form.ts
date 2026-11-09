// The form that lists what a rollback would discard, and discards it once the user confirms.

import { rollbackFiles, rollbackPlan, type Change, type RollbackPlan, type Repository } from "./core";
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
export function createRollbackForm(host: RollbackFormHost): { view: HTMLElement; open: (paths: string[]) => void } {
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
  /** Counts the times the form opened, so a late answer about an earlier selection is dropped. */
  let opened = 0;

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

  const close = (): void => {
    opened += 1;
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
      const confirmed = plan.files.map((change) => change.path);
      await rollbackFiles(repository, confirmed, deleteAdded.checked);
      close();
      host.rolledBack();
    } catch (error) {
      message.textContent = host.describeError(error);
    } finally {
      confirm.disabled = false;
    }
  });

  return {
    view,
    open: async (paths) => {
      opened += 1;
      const thisOpening = opened;
      show(null, "");
      view.hidden = false;
      cancel.focus();
      try {
        const found = await rollbackPlan(repository, paths);
        if (thisOpening === opened) {
          show(found, found.files.length === 0 ? t("rollbackNothing") : "");
        }
      } catch (error) {
        if (thisOpening === opened) {
          show(null, host.describeError(error));
        }
      }
    },
  };
}
