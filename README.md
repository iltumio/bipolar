<p align="center">
  <img src="data/dev.iltumio.Bipolar.svg" alt="Bipolar logo" width="112" height="112">
</p>

<h1 align="center">Bipolar</h1>

<p align="center">
  A native Rust / GPUI desktop app and CLI for Linux. Give personal and work folders their own Git identity and GitHub authentication, with Git's <code>includeIf</code> doing the inheritance.
</p>

<p align="center">
  <a href="https://github.com/iltumio/bipolar/releases/latest"><img src="https://img.shields.io/github/v/release/iltumio/bipolar?label=version" alt="Latest release"></a>
  <a href="https://github.com/iltumio/bipolar/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/iltumio/bipolar/release.yml?label=build" alt="Release build status"></a>
</p>

Existing folder `.gitconfig` files and direct `gitdir:` includes are discovered automatically. Opening the app or listing profiles does not change your configuration.

## Install

### Quick install (Linux x86_64)

Run this command from **Bash**, **Zsh**, or **Fish** (the installer runs with `sh`):

```sh
curl -fsSL https://github.com/iltumio/bipolar/releases/latest/download/install.sh | sh
```

The installer downloads the latest portable release, verifies its SHA-256 checksum,
and installs the binary, desktop entry, and icon into `~/.local` without `sudo`.
It requires `curl`, `tar`, and standard Linux utilities, including `sha256sum`.
To choose another prefix, use `curl -fsSL https://github.com/iltumio/bipolar/releases/latest/download/install.sh | env BIPOLAR_INSTALL_DIR=/your/prefix sh`.

If `~/.local/bin` is not already on your `PATH`, add it to your shell configuration:

- **Bash** (`~/.bashrc`): `export PATH="$HOME/.local/bin:$PATH"`
- **Zsh** (`~/.zshrc`): `export PATH="$HOME/.local/bin:$PATH"`
- **Fish** (run once): `fish_add_path ~/.local/bin`

Launch **Bipolar** from your application menu or run `bipolar`. The desktop app
requires a Wayland or X11 session with Vulkan support and the runtime libraries
`libxcb`, `libxkbcommon`, and `libxkbcommon-x11`.

### Prebuilt packages (Linux x86_64)

Download the latest release from [GitHub Releases](https://github.com/iltumio/bipolar/releases/latest):

- **Arch Linux**:
  ```sh
  sudo pacman -U bipolar-*-x86_64.pkg.tar.zst
  ```
- **Debian / Ubuntu**:
  ```sh
  sudo dpkg -i bipolar_*_amd64.deb
  ```
- **Portable tarball** (installs binary, desktop entry, and icon into `~/.local`):
  ```sh
  tar -xzf bipolar-x86_64-unknown-linux-gnu.tar.gz -C ~/.local
  ```
- **Standalone binaries**: `bipolar-x86_64-unknown-linux-gnu` (desktop GUI + CLI) and `bipolar-cli-x86_64-unknown-linux-gnu` (headless CLI-only).

### From source

Requires a current stable Rust toolchain (verified with **1.95**), Git, and a Wayland or X11 desktop with Vulkan support. The desktop UI is built with [GPUI](https://www.gpui.rs/) and links only against `libxcb`, `libxkbcommon`, and `libxkbcommon-x11`; GTK development libraries are not required. Native folder/file pickers use XDG Desktop Portal and need a portal backend implementing file selection (for example GTK, GNOME, or KDE). Paths can also be entered manually.

GitHub authentication additionally requires GitHub CLI at `/usr/bin/gh` and, for App profiles, the existing `~/.local/bin/zyphe-gh-auth` helper. On Arch Linux install `git` and `github-cli`, plus the portal backend for your desktop.

```sh
cargo run -- gui
cargo run -- list
cargo run -- list --json
```

For a CLI-only build without desktop dependencies:

```sh
cargo build --release --no-default-features
```

Install the desktop app and launcher with `make install` (default prefix: `~/.local`). This adds `bipolar`, its desktop entry, and its icon; it does not replace `gh` or change Git settings. Launch **Bipolar** from your application menu, or run `bipolar`.

## Folder profiles

```sh
# Inspect the exact proposed files without writing anything.
bipolar set ~/projects/personal \
  --name 'Your Name' --email you@example.com --dry-run

# Register the folder, creating/updating its .gitconfig.
bipolar set ~/projects/personal \
  --name 'Your Name' --email you@example.com \
  --auth personal --github-user your-github-login

# Configure commit signing (GPG or SSH) and a dedicated SSH private key.
bipolar set ~/projects/work \
  --name 'Your Name' --email you@company.com \
  --signing-key ~/.ssh/id_ed25519_work.pub --gpg-sign true \
  --set gpg.format=ssh --ssh-key ~/.ssh/id_ed25519_work

# Any additional Git setting can be added through the CLI.
bipolar set ~/projects/work --set pull.rebase=true

# See the effective settings and their source files inside a repository or folder.
bipolar inspect ~/projects/work/my-repository

# Remove only the include; retain the profile file.
bipolar remove ~/projects/work
```

A new profile produces a folder `.gitconfig` and a global entry like:

```gitconfig
[includeIf "gitdir:/home/you/projects/work/"]
    path = /home/you/projects/work/.gitconfig
```

Existing matching includes (including `~/` paths and relative profile paths) are reused, so there is no parallel profile database. Unknown Git settings and comments are retained. An existing symlinked profile is updated at its target without replacing the symlink.

The GPUI desktop app provides:

- A responsive sidebar and editor for browsing and creating folder profiles
- Native folder and key pickers (plus manual path input)
- Commit identity (`user.name`, `user.email`) and commit signing (`commit.gpgSign`, GPG `openpgp` or SSH `ssh` format, `user.signingKey`)
- Dedicated SSH key configuration (`core.sshCommand`) and Git HTTPS credential helper toggle (`credential.helper`)
- GitHub authentication mode selection (`Inherit`, `Personal Login`, `GitHub App`)
- One-click `gh` shim installation and conflict replacement (`~/.local/bin/gh`)
- Portable **Backup…** and **Restore…** workflows
- An exact before/after configuration diff preview before saving, unsaved-change protection when switching profiles, and safe `includeIf` removal

Arbitrary additional Git settings can be added via CLI `--set KEY=VALUE`.

## Portable backup and restore

Save all registered folder profiles and their `.gitconfig` contents to a portable `.json` bundle that can be restored onto a new machine (even when `$HOME` differs between machines):

```sh
# Save all folder profiles into a portable JSON file (alias: bipolar export)
bipolar backup ~/bipolar-backup.json

# Preview the exact files and global includes that would be restored
bipolar restore ~/bipolar-backup.json --dry-run

# Restore profiles and register global includes (alias: bipolar import)
bipolar restore ~/bipolar-backup.json
```

In the desktop app, use the **Backup…** and **Restore…** buttons in the header bar. Restoring always displays the before/after configuration diff preview before writing any files.

## Personal login and GitHub Apps

Personal profiles use the selected account already saved by `/usr/bin/gh auth login --hostname github.com`:

```sh
bipolar set ~/projects/personal --auth personal --github-user your-login
```

For work folders, use the existing zyphe-agents user authorization:

```sh
zyphe-gh-auth start
zyphe-gh-auth finish
bipolar set ~/projects/work --auth app \
  --app-id 5382086 --installation-id 173056379 --https-helper true
```

App mode currently integrates the machine's `zyphe-gh-auth` profile for `iltumio`.
Every shim invocation runs `refresh`, then `apply` to validate the `ghu_` token,
user and installation and select the keyring login. Bipolar removes inherited
`GH_TOKEN` / `GITHUB_TOKEN` and does not inject a token or cache one on disk.
Long-lived agent sessions therefore use the refreshed login on their next call.
Keep the existing `zyphe-gh-refresh.timer` enabled. Missing authorization or any
helper failure stops the command and asks for `start` + `finish`; there is no
installation-token or personal-login fallback. Other App IDs are rejected.
Existing `github.privateKey` settings are ignored and no private key is required.
Use the Bipolar shim for both `gh` and Git HTTPS; SSH remotes still use SSH keys.
Bipolar does not launch agent processes or change an already running agent's environment.

Run from inside a repository (or any folder covered by a registered scope):

```sh
bipolar gh -- pr list
bipolar gh -- api user/installations
```

Or install the native binary as a `gh` launcher:

```sh
bipolar install-gh
# If gh already exists, explicitly back it up and replace it:
bipolar install-gh --replace
```

Ensure `~/.local/bin` comes before `/usr/bin` on the PATH inherited by your terminal or agent. The launcher uses `/usr/bin/gh` internally, so it does not recurse. Outside configured folders it preserves normal GitHub CLI behavior. Inside configured folders, profile credentials take precedence over inherited `GH_TOKEN` / `GITHUB_TOKEN`. Manage personal logins directly with `/usr/bin/gh`.

For Git HTTPS, enable the HTTPS credential helper in the desktop UI, pass `--https-helper true` to `bipolar set`, or configure the command printed by `bipolar install-gh`. Existing SSH remotes continue to use their SSH credentials; changing API authentication does not change Git remote URLs.

This also works for commands issued by agents such as T3 Code when they resolve `gh` through that PATH. T3 Code's built-in GitHub buttons use its own authentication settings and are outside this launcher's control.

## AI Agent Skill

Install the built-in `SKILL.md` so coding agents (Claude Code, Codex, Cursor, Gemini, etc.) know how to inspect and create Bipolar folder profiles via the CLI:

```sh
# Install into ~/.agents/skills/bipolar/SKILL.md and symlink into detected agent skill folders
bipolar install-skill

# Install into a custom skills directory (e.g. a repository's .agents/skills)
bipolar install-skill --skills-dir .agents/skills

# Print the embedded SKILL.md without writing files
bipolar install-skill --print
```

## Configuration and recovery

The app honors an explicit `--config FILE`, then `$GIT_CONFIG_GLOBAL`; otherwise it manages `~/.gitconfig`, or `$XDG_CONFIG_HOME/git/config` when no classic file exists. If both global files exist, it manages the classic file; Git may still read the XDG file independently. Run `inspect` to see effective settings.

```sh
bipolar --config /tmp/demo.gitconfig set /tmp/example --name Demo --dry-run
```

Before writes, existing files get private `.bipolar-backup-*` copies beside them. Preview detects changes made before saving. Writes use temporary files and atomic renames under an application lock; if global registration fails, the profile is restored. This is not a crash-atomic transaction across two files. External tools do not participate in the application's lock. Keep backups for recovery and remove them when no longer needed:

```sh
bipolar clean-backups
```

Current scope notes:

- Only direct global folder `gitdir:` / `gitdir/i:` includes are listed. Branch conditions, arbitrary globs, and includes declared by another included file are left untouched.
- `gitdir:` follows Git's actual Git directory: linked worktrees inherit their main repository's folder profile even when checked out elsewhere.
- Paths containing Git glob syntax are rejected. GitHub authentication currently targets `github.com`.

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
# Headless GPUI rendering and profile workflow tests.
cargo test desktop::tests
```

Tests use temporary configuration files, including the GPUI desktop editor preview/save/remove workflow and headless rendering of its responsive layouts. They cover inheritance in real repositories and linked worktrees, existing includes, repeated registration, arbitrary setting preservation, symlinks, backups, stale previews, and malformed input. Native window creation and portal file pickers require a desktop session for manual verification.

Git conditional include behavior follows the [Git configuration documentation](https://git-scm.com/docs/git-config). GitHub App authentication follows [GitHub's user-token documentation](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user).

## License

MIT OR Apache-2.0.
