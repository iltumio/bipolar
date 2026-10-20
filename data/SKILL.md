---
name: bipolar
description: Use when managing folder-scoped Git identities, commit signing (GPG/SSH), dedicated SSH keys, or GitHub CLI (`gh`) / HTTPS authentication (personal accounts or GitHub Apps) with the `bipolar` CLI.
---

# Bipolar CLI Skill

`bipolar` manages folder-scoped Git identities (`includeIf.gitdir:<folder>/.path`) and GitHub authentication (`personal` login or short-lived `app` installation tokens) without overwriting unmanaged Git settings or comments.

## 1. Inspect Current Profiles & Repository State

Before creating or modifying a profile, check the existing folder profiles or inspect a target directory:

```bash
# List all registered folder profiles (human-readable or JSON)
bipolar list
bipolar list --json

# Check which settings and origins apply to a specific repository or folder
bipolar inspect /path/to/repo-or-folder
```

Note: `bipolar inspect` works both inside initialized Git repositories and inside plain directories covered by a registered folder scope.

## 2. Create or Update a Folder Profile (`bipolar set`)

Always preview changes first with `--dry-run` when modifying an existing profile or when unsure of the diff, then run without `--dry-run` to apply atomically (backups are created automatically).

### Basic Git Identity (Author Name & Email)

```bash
bipolar set ~/work \
  --name "Jane Doe" \
  --email "jane@company.com"
```

### Commit Signing (GPG or SSH) & Dedicated SSH Key

```bash
# Sign commits with an SSH public key and use a dedicated SSH private key for git push/fetch
bipolar set ~/work \
  --name "Jane Doe" \
  --email "jane@company.com" \
  --signing-key "~/.ssh/id_ed25519_work.pub" \
  --gpg-sign true \
  --set gpg.format=ssh \
  --ssh-key ~/.ssh/id_ed25519_work
```

### GitHub Authentication Modes (`--auth`)

Bipolar supports three authentication modes per folder:

1. **Inherited (`--auth inherit`)**: Uses the global Git / `gh` configuration.
2. **Personal Login (`--auth personal`)**: Switches `gh` and Git HTTPS to a specific GitHub user already logged in via `/usr/bin/gh auth login --hostname github.com`.
   ```bash
   bipolar set ~/personal \
     --name "Jane" \
     --email "jane@users.noreply.github.com" \
     --auth personal \
     --github-user octocat \
     --https-helper true
   ```
3. **GitHub App (`--auth app`)**: Automatically signs a RS256 JWT with OpenSSL and mints/caches short-lived installation tokens (`~/.cache/bipolar`). Never falls back to personal credentials on failure.
   - Note: The `.pem` private key file **must** exist at an absolute path (or `~/...`) and have owner-only permissions (`chmod 600`).
   ```bash
   chmod 600 ~/.keys/company-bot.pem
   bipolar set ~/company-bot \
     --name "company-bot[bot]" \
     --email "123456+company-bot[bot]@users.noreply.github.com" \
     --auth app \
     --app-id 123456 \
     --installation-id 78901234 \
     --private-key ~/.keys/company-bot.pem \
     --https-helper true
   ```

### Arbitrary Git Config Keys (`--set KEY=VALUE`)

Pass any additional Git setting for the folder's `.gitconfig` using repeatable `--set`:

```bash
bipolar set ~/work --set pull.rebase=true --set core.autocrlf=input
```

## 3. Portable Backup & Restore Across Machines

To export all folder profiles and their `.gitconfig` contents to a portable JSON file (paths under `$HOME` are normalized to `~/` and automatically relocated when restored onto a machine with a different `$HOME`):

```bash
# Export all profiles into a portable JSON backup file (alias: bipolar export)
bipolar backup ~/bipolar-backup.json

# Preview what restoring a backup file will create/update
bipolar restore ~/bipolar-backup.json --dry-run

# Apply the backup on a new or existing machine (alias: bipolar import)
bipolar restore ~/bipolar-backup.json
```

## 4. Install & Use the Native `gh` Shim

To make `gh` and Git HTTPS (`credential.helper`) automatically use the active folder's GitHub identity:

```bash
# Install the symlink ~/.local/bin/gh -> bipolar (add --replace to back up and replace an existing file)
bipolar install-gh

# Or run a one-off gh command under the current directory's profile without installing the shim
bipolar gh -- pr status
bipolar gh -- api user --jq .login
```

Ensure `~/.local/bin` precedes `/usr/bin` in `PATH`.

## 5. Remove a Folder Scope or Clean Backups

```bash
# Unregister the includeIf entry from global ~/.gitconfig (keeps the folder's .gitconfig intact)
bipolar remove ~/work

# Remove .bipolar-backup-* files across global and profile directories
bipolar clean-backups
```
