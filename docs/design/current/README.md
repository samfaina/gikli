# Capturas de la interfaz actual

Estado de la UI en la rama `redesign-ui`, antes del rediseño (26/11/2026). Sirven de referencia para el brief (`../BRIEF.md`).

- **Cómo se hicieron:** build de depuración (`tauri build --debug --no-bundle`) en Windows 11 (WebView2), conducido con Playwright por CDP. Viewport de 1440×900 a 1×.
- **Temas:** cada pantalla está en claro (`.light.png`) y oscuro (`.dark.png`). La app sigue el tema del sistema, y aquí se forzó con `prefers-color-scheme`.
- **Datos:** repositorios de ejemplo generados para la ocasión:
  - `acme-web`: ramas, un merge, tags, un remoto con un commit por delante y dos por detrás, un stash y cambios locales.
  - `conflict-demo`: un merge y un rebase con conflictos.
  - `workspace`: un Project con tres repositorios.
- **Fuera de las capturas:** los diálogos nativos del sistema (Open Folder…, New Repository…, Browse…).

## Arranque y Project

| Captura | Qué muestra |
|---|---|
| `00-no-project` | Ventana sin Project abierto |
| `09-clone-form-empty` | Formulario de clonado, vacío |
| `10-clone-form` | Formulario de clonado con una URL (HTTPS/SSH) |
| `11-settings`, `11-settings-git-tested` | Ajustes, y el resultado de probar la ruta de git |
| `12-operation-log` | Registro de operaciones (comandos git ejecutados) |
| `80-project-repositories-not-added` | Project con repositorios encontrados sin añadir, con sus notificaciones |
| `81-project-one-added` | Uno añadido y dos pendientes |
| `82-project-several-repositories` (+ `-full`, página entera) | Varios repositorios apilados en la misma vista; barra de estado con uno por repositorio |
| `83-git-prompt` | Pregunta de git al usuario (askpass: contraseña) |
| `84-commit-identity-form` | Commit sin nombre ni email configurados |
| `85-clone-running` | Operación larga en curso (clonado esperando un hook), con progreso y Cancel |

## Cambios locales y commit

| Captura | Qué muestra |
|---|---|
| `01-changes` | Vista principal: Changelist activa, Unversioned Files y panel de commit |
| `02-changes-flat` | La misma lista sin agrupar por carpeta |
| `03-change-context-menu` | Menú contextual de un cambio |
| `04-move-to-changelist-form` | Mover a otra Changelist |
| `05-rollback-form` | Confirmación de Rollback |
| `06-unversioned-context-menu` | Menú de un archivo sin versionar |
| `07-new-changelist-form` | Nueva Changelist |
| `08-changes-ignored-files` | Con los archivos ignorados visibles |
| `62-commit-panel-message` | Panel de commit con mensaje |
| `63-commit-panel-amend` | Amend del último commit |
| `64-staging-mode` | Modo Staging (Staged / Unstaged) en lugar de Changelists |
| `65-staging-mode-staged-menu`, `66-staging-mode-unstaged-menu` | Menús en modo Staging |
| `67-two-changelists` | Dos Changelists |
| `70-clean-repository` | Repositorio sin cambios |

## Diff

| Captura | Qué muestra |
|---|---|
| `20-diff-modified-file` | Diff lado a lado (HEAD / Local) con líneas sin cambios plegadas |
| `21-diff-with-unsaved-edit` | Lado local editado sin guardar |
| `22-diff-next-file` | Siguiente archivo |
| `23-diff-new-file`, `24-diff-deleted-file` | Archivo nuevo y archivo borrado |
| `73-diff-conflicted-file` | Archivo en conflicto con sus marcadores |

## Log (historial)

| Captura | Qué muestra |
|---|---|
| `30-log` | Log con grafo, panel de ramas, archivos cambiados y detalles |
| `31-log-merge-commit-selected` | Commit de merge seleccionado (cambios contra cada padre) |
| `32-log-commit-selected` | Commit normal seleccionado |
| `33-log-commit-changes-flat` | Archivos cambiados en lista plana |
| `34-log-commit-menu` | Menú contextual de un commit |
| `35-log-long-edges` | Opción "Show long edges" |
| `36-log-filtered-by-branch` | Log filtrado por una rama |
| `37-log-text-search` | Búsqueda por texto |
| `38-log-go-to` | Campo "Go to hash, branch or tag" |
| `74-log-during-merge` | Log con un merge en curso |

## Ramas

| Captura | Qué muestra |
|---|---|
| `13-branch-selector` | Selector de ramas (desde la barra de estado) |
| `14-branch-selector-menu` | Menú de una rama: Check Out / Merge into Current |
| `15-branch-selector-search` | Búsqueda en el selector |
| `40-new-branch-dialog` | Nueva rama |
| `41-rename-branch-dialog` | Renombrar rama |
| `42-delete-branches-dialog` | Borrar ramas |
| `43-branch-deleted-notification` | Notificación tras borrar, con Restore |
| `44-unmerged-commits-dialog` | Commits que se llevó la rama borrada |
| `45-file-history`, `46-file-history-first-revision` | Historial de un archivo |
| `60-blocked-checkout-dialog` | Checkout bloqueado por cambios locales |
| `61-local-commits-dialog` | Checkout de rama remota que perdería commits locales |
| `68-blocked-merge-dialog` | Merge bloqueado por cambios locales |

## Stash y remotos

| Captura | Qué muestra |
|---|---|
| `50-stash-dialog` | Crear un stash |
| `51-stashes` | Vista de stashes con archivos y diff |
| `52-unstash-dialog` | Aplicar o hacer pop de un stash |
| `53-pull-dialog` | Pull |
| `54-push-dialog` | Push: commits que se enviarán, archivos y diff |
| `55-rejected-push-dialog` | Push rechazado: Merge / Rebase |

## Conflictos y operaciones a medias

| Captura | Qué muestra |
|---|---|
| `71-merge-conflicts-dialog` | Diálogo de conflictos (Accept Yours / Theirs / Mark Resolved) |
| `72-merge-in-progress` | Merge a medias: notificación, grupo Conflicts y barra de estado con Commit Merge / Abort |
| `75-rebase-stopped` | Rebase detenido: notificaciones y Continue / Skip / Abort |
