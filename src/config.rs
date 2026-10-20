use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::NamedTempFile;

const MAX_BACKUPS_PER_DIR: usize = 5;
const BACKUP_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct Store {
    pub global: PathBuf,
    pub home: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    pub folder: PathBuf,
    pub file: PathBuf,
    pub condition: String,
    pub name: String,
    pub email: String,
    pub auth: String,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub folder: PathBuf,
    pub file: PathBuf,
    pub global: PathBuf,
    pub before_profile: Option<Vec<u8>>,
    pub after_profile: Vec<u8>,
    pub before_global: Option<Vec<u8>>,
    pub after_global: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupBundle {
    pub version: u32,
    pub source_home: String,
    pub profiles: Vec<BackupProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupProfile {
    pub folder: String,
    pub file: String,
    #[serde(default)]
    pub case_insensitive: bool,
    pub config_content: String,
}

#[derive(Clone, Debug)]
pub struct RestoreFilePlan {
    pub folder: PathBuf,
    pub file: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RestorePlan {
    pub global: PathBuf,
    pub before_global: Option<Vec<u8>>,
    pub after_global: Vec<u8>,
    pub files: Vec<RestoreFilePlan>,
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Cannot read {}", path.display())),
    }
}

fn git_config(file: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("config")
        .arg("--no-includes")
        .arg("--file")
        .arg(file)
        .args(args)
        .output()
        .context("Cannot run git config")
}

pub fn value(file: &Path, key: &str) -> Result<String> {
    if !file.exists() {
        return Ok(String::new());
    }
    let out = git_config(file, &["--get", key])?;
    if out.status.code() == Some(1) {
        return Ok(String::new());
    }
    ensure!(
        out.status.success(),
        "Invalid Git config {}: {}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout)?
        .trim_end_matches('\n')
        .to_owned())
}

fn validate_git_config_bytes(bytes: &[u8]) -> Result<()> {
    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(bytes)?;
    let out = git_config(tmp.path(), &["--list"])?;
    ensure!(
        out.status.success(),
        "Invalid Git configuration: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

fn edit(original: Option<&[u8]>, changes: &[(String, Option<String>)]) -> Result<Vec<u8>> {
    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(original.unwrap_or_default())?;
    for (key, val) in changes {
        let out = match val {
            Some(v) => git_config(tmp.path(), &["--replace-all", key, v])?,
            None => git_config(tmp.path(), &["--unset-all", key])?,
        };
        let missing = val.is_none() && matches!(out.status.code(), Some(1 | 5));
        ensure!(
            out.status.success() || missing,
            "Cannot set {key}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = git_config(tmp.path(), &["--list"])?;
    ensure!(
        out.status.success(),
        "Invalid Git configuration: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(fs::read(tmp.path())?)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    let mode = fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o600);
    tmp.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn prune_backups_in_dir(dir: &Path, keep: usize) -> Result<usize> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut backups: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_str().is_some_and(|n| {
                n.starts_with(".bipolar-backup-") || n.starts_with(".bipolar-git-backup-")
            })
        })
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, e.path()))
        })
        .collect();
    if backups.len() <= keep {
        return Ok(0);
    }
    backups.sort_by_key(|b| std::cmp::Reverse(b.0));
    let mut removed = 0;
    for (_, path) in backups.into_iter().skip(keep) {
        if fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn backup(path: &Path, data: &Option<Vec<u8>>) -> Result<Option<PathBuf>> {
    let Some(data) = data else {
        return Ok(None);
    };
    let parent = path.parent().context("Path has no parent")?;
    let mut temp = tempfile::Builder::new()
        .prefix(".bipolar-backup-")
        .tempfile_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    let (_, path) = temp.keep()?;
    let _ = prune_backups_in_dir(parent, MAX_BACKUPS_PER_DIR);
    Ok(Some(path))
}

pub fn ssh_command_for_key(key_path: &str) -> String {
    let trimmed = key_path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let quoted = format!("'{}'", trimmed.replace('\'', "'\\''"));
    format!("ssh -i {quoted} -o IdentitiesOnly=yes")
}

pub fn extract_ssh_key(ssh_command: &str) -> Option<String> {
    let rest = ssh_command.trim().strip_prefix("ssh -i ")?;
    let rest = rest.strip_suffix(" -o IdentitiesOnly=yes")?.trim();
    if let Some(inner) = rest.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        Some(inner.replace("'\\''", "'"))
    } else {
        Some(rest.to_owned())
    }
}

fn to_portable_path(path: &Path, home: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(home)
        && !rel.as_os_str().is_empty()
    {
        return format!("~/{}", rel.display());
    }
    path.to_string_lossy().into_owned()
}

fn same_folder(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

impl Store {
    pub fn discover(override_path: Option<PathBuf>) -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
        let global = override_path
            .or_else(|| std::env::var_os("GIT_CONFIG_GLOBAL").map(PathBuf::from))
            .unwrap_or_else(|| {
                let classic = home.join(".gitconfig");
                let xdg = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".config"))
                    .join("git/config");
                if classic.exists() || !xdg.exists() {
                    classic
                } else {
                    xdg
                }
            });
        let global = if global.is_absolute() {
            global
        } else {
            std::env::current_dir()?.join(global)
        };
        let global = if global.is_symlink() {
            fs::canonicalize(global)?
        } else {
            global
        };
        Ok(Self { global, home })
    }

    pub fn expand(&self, path: &str, relative_to: &Path) -> PathBuf {
        if let Some(rest) = path.strip_prefix("~/") {
            self.home.join(rest)
        } else if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            relative_to.join(path)
        }
    }

    fn relocate_path(&self, raw: &str, source_home: &str) -> PathBuf {
        if let Some(rest) = raw.strip_prefix("~/") {
            return self.home.join(rest);
        }
        if !source_home.is_empty()
            && let Ok(rel) = Path::new(raw).strip_prefix(source_home)
            && !rel.as_os_str().is_empty()
        {
            return self.home.join(rel);
        }
        let base = self.global.parent().unwrap_or(&self.home);
        self.expand(raw, base)
    }

    fn relocate_content(&self, content: &str, source_home: &str) -> String {
        let src = source_home.trim_end_matches('/');
        let dst = self.home.to_string_lossy();
        let dst = dst.trim_end_matches('/');
        if !src.is_empty() && src != dst {
            content.replace(&format!("{src}/"), &format!("{dst}/"))
        } else {
            content.to_owned()
        }
    }

    pub fn scopes(&self) -> Result<Vec<Scope>> {
        if !self.global.exists() {
            return Ok(Vec::new());
        }
        let out = git_config(
            &self.global,
            &["--null", "--get-regexp", r"^includeif\..*\.path$"],
        )?;
        if out.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        ensure!(
            out.status.success(),
            "Cannot read includes: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut scopes = Vec::new();
        for record in out.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let record = std::str::from_utf8(record)?;
            let (key, path) = record
                .split_once('\n')
                .context("Invalid git config output")?;
            let Some(condition) = key
                .strip_prefix("includeif.")
                .and_then(|k| k.strip_suffix(".path"))
            else {
                continue;
            };
            let Some(folder) = condition
                .strip_prefix("gitdir:")
                .or_else(|| condition.strip_prefix("gitdir/i:"))
            else {
                continue;
            };
            let folder = folder
                .strip_suffix("/**")
                .unwrap_or(folder)
                .trim_end_matches('/');
            // Branch, remote and arbitrary glob conditions remain untouched.
            if folder.contains(['*', '?', '[', '\\'])
                || !(folder.starts_with('/') || folder.starts_with("~/"))
            {
                continue;
            }
            let file = self.expand(path, self.global.parent().unwrap());
            scopes.push(Scope {
                folder: self.expand(folder, self.global.parent().unwrap()),
                name: value(&file, "user.name")?,
                email: value(&file, "user.email")?,
                auth: value(&file, "github.authMode")?,
                file,
                condition: condition.to_owned(),
            });
        }
        Ok(scopes)
    }

    pub fn matching_scope(&self, path: &Path) -> Result<Option<Scope>> {
        let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut best: Option<Scope> = None;
        for scope in self.scopes()? {
            let folder = fs::canonicalize(&scope.folder).unwrap_or_else(|_| scope.folder.clone());
            if target.starts_with(&folder) {
                let replace = best.as_ref().is_none_or(|current| {
                    let cur_folder = fs::canonicalize(&current.folder)
                        .unwrap_or_else(|_| current.folder.clone());
                    folder.components().count() >= cur_folder.components().count()
                });
                if replace {
                    best = Some(scope);
                }
            }
        }
        Ok(best)
    }

    pub fn clean_backups(&self) -> Result<usize> {
        let mut removed = 0;
        if let Some(parent) = self.global.parent() {
            removed += prune_backups_in_dir(parent, 0)?;
        }
        for scope in self.scopes().unwrap_or_default() {
            if let Some(parent) = scope.file.parent() {
                removed += prune_backups_in_dir(parent, 0)?;
            }
        }
        Ok(removed)
    }

    pub fn export_backup(&self, destination: &Path) -> Result<BackupBundle> {
        let scopes = self.scopes()?;
        let mut profiles = Vec::with_capacity(scopes.len());
        for scope in scopes {
            let bytes = read_optional(&scope.file)?.unwrap_or_default();
            let config_content = String::from_utf8(bytes)
                .with_context(|| format!("{} is not valid UTF-8", scope.file.display()))?;
            profiles.push(BackupProfile {
                folder: to_portable_path(&scope.folder, &self.home),
                file: to_portable_path(&scope.file, &self.home),
                case_insensitive: scope.condition.starts_with("gitdir/i:"),
                config_content,
            });
        }
        let bundle = BackupBundle {
            version: BACKUP_FORMAT_VERSION,
            source_home: self.home.to_string_lossy().into_owned(),
            profiles,
        };
        let mut json = serde_json::to_vec_pretty(&bundle)?;
        json.push(b'\n');
        let target = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            std::env::current_dir()?.join(destination)
        };
        atomic_write(&target, &json)?;
        Ok(bundle)
    }

    pub fn plan_restore(&self, source: &Path) -> Result<RestorePlan> {
        let raw = fs::read(source)
            .with_context(|| format!("Cannot read backup file {}", source.display()))?;
        let bundle: BackupBundle = serde_json::from_slice(&raw)
            .with_context(|| format!("Invalid backup JSON in {}", source.display()))?;
        ensure!(
            bundle.version == BACKUP_FORMAT_VERSION,
            "Unsupported backup format version {} (expected {})",
            bundle.version,
            BACKUP_FORMAT_VERSION
        );

        let existing_scopes = self.scopes()?;
        let before_global = read_optional(&self.global)?;

        let mut resolved = Vec::with_capacity(bundle.profiles.len());
        for profile in &bundle.profiles {
            let folder = self.relocate_path(&profile.folder, &bundle.source_home);
            let folder = fs::canonicalize(&folder).unwrap_or(folder);
            let folder_text = folder.to_str().context("Folder path must be valid UTF-8")?;
            ensure!(
                !folder_text.contains(['*', '?', '[', '\\', '\n', '\r']),
                "Folder contains characters with special meaning in Git include patterns: {}",
                folder.display()
            );

            let matches: Vec<_> = existing_scopes
                .iter()
                .filter(|s| same_folder(&s.folder, &folder))
                .collect();
            ensure!(
                matches.len() <= 1,
                "Multiple existing includes match {}; resolve duplicate includes first",
                folder.display()
            );
            let existing = matches.first().copied();

            let file = if let Some(scope) = existing {
                scope.file.clone()
            } else if profile.file.trim().is_empty() {
                folder.join(".gitconfig")
            } else {
                self.relocate_path(&profile.file, &bundle.source_home)
            };
            let file = if file.is_symlink() {
                fs::canonicalize(&file)?
            } else {
                file
            };
            ensure!(
                file != self.global,
                "The scoped configuration cannot be the global configuration ({})",
                file.display()
            );

            let relocated_content =
                self.relocate_content(&profile.config_content, &bundle.source_home);
            let after_bytes = relocated_content.into_bytes();
            validate_git_config_bytes(&after_bytes)?;

            let condition = if let Some(scope) = existing {
                scope.condition.clone()
            } else if profile.case_insensitive {
                format!("gitdir/i:{}/", folder.display())
            } else {
                format!("gitdir:{}/", folder.display())
            };

            resolved.push((folder, file, condition, existing.is_some(), after_bytes));
        }

        // Order from shallowest to deepest folder so parent scopes always precede child scopes.
        resolved.sort_by_key(|(folder, _, _, _, _)| folder.components().count());

        for i in 0..resolved.len() {
            for j in (i + 1)..resolved.len() {
                ensure!(
                    !same_folder(&resolved[i].0, &resolved[j].0),
                    "Backup contains duplicate entries for folder {}",
                    resolved[i].0.display()
                );
                ensure!(
                    resolved[i].1 != resolved[j].1,
                    "Backup maps multiple folders to the same config file {}",
                    resolved[i].1.display()
                );
            }
        }

        let mut current_global = before_global.clone();
        let mut tracked_scopes = existing_scopes;
        let mut files = Vec::with_capacity(resolved.len());

        for (folder, file, condition, already_registered, after) in resolved {
            let before = read_optional(&file)?;
            if !already_registered {
                let descendants: Vec<_> = tracked_scopes
                    .iter()
                    .filter(|s| {
                        let s_folder =
                            fs::canonicalize(&s.folder).unwrap_or_else(|_| s.folder.clone());
                        s_folder != folder && s_folder.starts_with(&folder)
                    })
                    .cloned()
                    .collect();
                let key = format!("includeIf.{condition}.path");
                let next_global = if descendants.is_empty() {
                    edit(
                        current_global.as_deref(),
                        &[(key, Some(file.to_string_lossy().into_owned()))],
                    )?
                } else {
                    let mut global_changes = Vec::new();
                    for desc in &descendants {
                        global_changes.push((format!("includeIf.{}.path", desc.condition), None));
                    }
                    global_changes.push((key, Some(file.to_string_lossy().into_owned())));
                    for desc in &descendants {
                        global_changes.push((
                            format!("includeIf.{}.path", desc.condition),
                            Some(desc.file.to_string_lossy().into_owned()),
                        ));
                    }
                    edit(current_global.as_deref(), &global_changes)?
                };
                current_global = Some(next_global);
                tracked_scopes.push(Scope {
                    folder: folder.clone(),
                    file: file.clone(),
                    condition,
                    name: String::new(),
                    email: String::new(),
                    auth: String::new(),
                });
            }
            files.push(RestoreFilePlan {
                folder,
                file,
                before,
                after,
            });
        }

        Ok(RestorePlan {
            global: self.global.clone(),
            before_global,
            after_global: current_global.unwrap_or_default(),
            files,
        })
    }

    pub fn apply_restore(&self, plan: &RestorePlan) -> Result<Vec<PathBuf>> {
        ensure!(
            plan.global == self.global,
            "Restore plan belongs to another global configuration"
        );
        let _lock = self.lock()?;
        ensure!(
            read_optional(&self.global)? == plan.before_global,
            "Global configuration changed since preview; reload and retry"
        );
        for item in &plan.files {
            ensure!(
                read_optional(&item.file)? == item.before,
                "Configuration {} changed since preview; reload and retry",
                item.file.display()
            );
        }

        let mut backups = Vec::new();
        for item in &plan.files {
            if item.before.as_deref() != Some(item.after.as_slice())
                && let Some(path) = backup(&item.file, &item.before)?
            {
                backups.push(path);
            }
        }
        if plan.before_global.as_deref() != Some(plan.after_global.as_slice())
            && let Some(path) = backup(&self.global, &plan.before_global)?
        {
            backups.push(path);
        }

        for item in &plan.files {
            fs::create_dir_all(&item.folder).with_context(|| {
                format!("Cannot create profile directory {}", item.folder.display())
            })?;
            if item.before.as_deref() != Some(item.after.as_slice()) {
                atomic_write(&item.file, &item.after)?;
            }
        }
        if plan.before_global.as_deref() != Some(plan.after_global.as_slice()) {
            atomic_write(&self.global, &plan.after_global)?;
        }
        Ok(backups)
    }

    pub fn plan(&self, folder: &Path, changes: &[(String, Option<String>)]) -> Result<Plan> {
        let folder = fs::canonicalize(folder)
            .with_context(|| format!("Folder does not exist: {}", folder.display()))?;
        ensure!(folder.is_dir(), "Select a directory");
        let text = folder.to_str().context("Folder path must be valid UTF-8")?;
        ensure!(
            !text.contains(['*', '?', '[', '\\', '\n', '\r']),
            "Folder contains characters with special meaning in Git include patterns"
        );
        let scopes = self.scopes()?;
        let matches: Vec<_> = scopes
            .iter()
            .filter(|s| fs::canonicalize(&s.folder).ok().as_ref() == Some(&folder))
            .collect();
        ensure!(
            matches.len() <= 1,
            "Multiple includes match this folder; resolve the duplicate includes first"
        );
        let existing = matches.first();
        let file = existing
            .map(|s| s.file.clone())
            .unwrap_or_else(|| folder.join(".gitconfig"));
        let file = if file.is_symlink() {
            fs::canonicalize(file)?
        } else {
            file
        };
        ensure!(
            file != self.global,
            "The scoped configuration cannot be the global configuration"
        );
        let before_profile = read_optional(&file)?;
        let before_global = read_optional(&self.global)?;
        let after_profile = edit(before_profile.as_deref(), changes)?;
        validate_auth(&after_profile, &self.home)?;
        let after_global = if existing.is_some() {
            before_global.clone().unwrap_or_default()
        } else {
            let descendants: Vec<_> = scopes
                .iter()
                .filter(|s| {
                    let s_folder = fs::canonicalize(&s.folder).unwrap_or_else(|_| s.folder.clone());
                    s_folder != folder && s_folder.starts_with(&folder)
                })
                .collect();
            let key = format!("includeIf.gitdir:{}/.path", folder.display());
            if descendants.is_empty() {
                edit(
                    before_global.as_deref(),
                    &[(key, Some(file.to_string_lossy().into_owned()))],
                )?
            } else {
                // Reorder so parent scope precedes child scopes, preserving Git override precedence.
                let mut global_changes = Vec::new();
                for desc in &descendants {
                    global_changes.push((format!("includeIf.{}.path", desc.condition), None));
                }
                global_changes.push((key, Some(file.to_string_lossy().into_owned())));
                for desc in &descendants {
                    global_changes.push((
                        format!("includeIf.{}.path", desc.condition),
                        Some(desc.file.to_string_lossy().into_owned()),
                    ));
                }
                edit(before_global.as_deref(), &global_changes)?
            }
        };
        Ok(Plan {
            folder,
            file,
            global: self.global.clone(),
            before_profile,
            after_profile,
            before_global,
            after_global,
        })
    }

    pub fn remove(&self, folder: &Path) -> Result<Vec<PathBuf>> {
        let requested = fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let scopes = self.scopes()?;
        let found: Vec<_> = scopes
            .iter()
            .filter(|s| {
                fs::canonicalize(&s.folder).unwrap_or_else(|_| s.folder.clone()) == requested
            })
            .collect();
        ensure!(
            !found.is_empty(),
            "No include found for {}",
            folder.display()
        );
        let changes = found
            .iter()
            .map(|s| (format!("includeIf.{}.path", s.condition), None))
            .collect::<Vec<_>>();
        let before = read_optional(&self.global)?;
        let after = edit(before.as_deref(), &changes)?;
        let _lock = self.lock()?;
        ensure!(
            read_optional(&self.global)? == before,
            "Configuration changed; reload and retry"
        );
        let backups = backup(&self.global, &before)?.into_iter().collect();
        atomic_write(&self.global, &after)?;
        Ok(backups)
    }

    fn lock(&self) -> Result<fs::File> {
        fs::create_dir_all(self.global.parent().context("Invalid global path")?)?;
        let mut path = self.global.as_os_str().to_owned();
        path.push(".bipolar-lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(path))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    pub fn apply(&self, plan: &Plan) -> Result<Vec<PathBuf>> {
        ensure!(
            plan.global == self.global,
            "Plan belongs to another global configuration"
        );
        let _lock = self.lock()?;
        ensure!(
            read_optional(&plan.file)? == plan.before_profile
                && read_optional(&self.global)? == plan.before_global,
            "Configuration changed since preview; reload and retry"
        );
        if plan.before_profile.as_deref() == Some(&plan.after_profile)
            && plan.before_global.as_deref() == Some(&plan.after_global)
        {
            return Ok(Vec::new());
        }
        let mut backups = Vec::new();
        for (path, before) in [
            (&plan.file, &plan.before_profile),
            (&self.global, &plan.before_global),
        ] {
            if let Some(path) = backup(path, before)? {
                backups.push(path);
            }
        }
        atomic_write(&plan.file, &plan.after_profile)?;
        if let Err(error) = atomic_write(&self.global, &plan.after_global) {
            let rollback = match &plan.before_profile {
                Some(data) => atomic_write(&plan.file, data),
                None => fs::remove_file(&plan.file).map_err(Into::into),
            };
            if let Err(rollback) = rollback {
                bail!(
                    "Global write failed: {error}; profile rollback failed: {rollback}. Recover from the backups."
                );
            }
            return Err(error).context("Global write failed; profile was restored");
        }
        Ok(backups)
    }
}

fn validate_auth(bytes: &[u8], home: &Path) -> Result<()> {
    let mut file = NamedTempFile::new()?;
    file.write_all(bytes)?;
    match value(file.path(), "github.authMode")?.as_str() {
        "" => (),
        "personal" => ensure!(
            !value(file.path(), "github.user")?.is_empty(),
            "Personal authentication requires a GitHub username"
        ),
        "app" => {
            for key in ["github.appId", "github.installationId"] {
                let id = value(file.path(), key)?;
                ensure!(
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
                    "{key} must be a numeric ID"
                );
            }
            let key = value(file.path(), "github.privateKey")?;
            let key = key
                .strip_prefix("~/")
                .map(|p| home.join(p))
                .unwrap_or_else(|| PathBuf::from(key));
            ensure!(
                key.is_absolute() && key.is_file(),
                "Select an existing absolute private-key file path"
            );
            ensure!(
                fs::metadata(key)?.permissions().mode() & 0o077 == 0,
                "Private key must be readable only by its owner (chmod 600)"
            );
        }
        other => bail!("Unsupported authentication mode: {other}"),
    }
    Ok(())
}

fn append_diff_section(output: &mut String, path: &Path, before: &Option<Vec<u8>>, after: &[u8]) {
    if before.as_deref() == Some(after) {
        return;
    }
    output.push_str(&format!(
        "--- {} (current)\n+++ {} (proposed)\n",
        path.display(),
        path.display()
    ));
    // Show exact before/after rather than claiming this is a unified patch.
    output.push_str("CURRENT:\n");
    output.push_str(&String::from_utf8_lossy(
        before.as_deref().unwrap_or_default(),
    ));
    output.push_str("\nPROPOSED:\n");
    output.push_str(&String::from_utf8_lossy(after));
    output.push('\n');
}

impl Plan {
    pub fn preview(&self) -> String {
        let mut output = String::new();
        append_diff_section(
            &mut output,
            &self.file,
            &self.before_profile,
            &self.after_profile,
        );
        append_diff_section(
            &mut output,
            &self.global,
            &self.before_global,
            &self.after_global,
        );
        if output.is_empty() {
            output.push_str("No changes.\n");
        }
        output
    }
}

impl RestorePlan {
    pub fn preview(&self) -> String {
        let mut output = String::new();
        for item in &self.files {
            append_diff_section(&mut output, &item.file, &item.before, &item.after);
        }
        append_diff_section(
            &mut output,
            &self.global,
            &self.before_global,
            &self.after_global,
        );
        if output.is_empty() {
            output.push_str("No changes.\n");
        }
        output
    }
}
