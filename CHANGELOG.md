# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com), this project
adheres to [Semantic Versioning](https://semver.org), and the changelog is maintained
automatically by [Knope](https://knope.tech) from
[Conventional Commits](https://www.conventionalcommits.org).
## 0.2.0 (2026-10-27)

### Changed

- GitHub App authentication now acts as `iltumio` through the existing
  `zyphe-gh-auth` user authorization instead of minting installation tokens.
- Each `gh` and Git HTTPS helper call refreshes and validates the keyring login,
  removes inherited token variables, and stops on authentication errors without
  falling back to bot or personal credentials.
- App profiles no longer require a private key. This is a breaking change:
  App mode requires the zyphe-agents profile (App `5382086`, installation
  `173056379`) and `~/.local/bin/zyphe-gh-auth`.

## 0.1.1 (2026-10-21)

### Features

- add release installer and shell installation instructions

## 0.1.0 (2026-10-20)

### Features

- migrate desktop UI from egui/eframe to gpui
