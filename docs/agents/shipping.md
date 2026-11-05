# Shipping

Read by the `ship-tickets` skill (samfaina/agent-skills).

- **Base branch:** `main`
- **Merge method:** `merge`
- **Merge approval:** `auto`
- **Branch naming:** issue number followed by the issue title in kebab-case, e.g. `12-add-login-form`. Orca prefixes the branch with `samfaina/`, so the branch becomes `samfaina/12-add-login-form`.
- **After merge:** delete the remote branch with `git push origin --delete <branch>`.
- **CI:** `none`
- **Workers:** `claude` (all roles)

## PR format

- **Title:** the issue title, unchanged.
- **Body:**
  1. `## Summary`: what changes and why.
  2. `## Testing`: how the change was verified.

  End with `Closes #<n>`.
