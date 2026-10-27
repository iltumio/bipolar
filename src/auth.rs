use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const GH: &str = "/usr/bin/gh";

fn effective(key: &str, global: Option<&Path>) -> Result<String> {
    let mut command = Command::new("git");
    command.args(["config", "--get", key]);
    if let Some(path) = global {
        command.env("GIT_CONFIG_GLOBAL", path);
    }
    let output = command.output()?;
    ensure!(
        output.status.success() || output.status.code() == Some(1),
        "Cannot read effective Git configuration"
    );
    let val = String::from_utf8(output.stdout)?.trim().to_owned();
    if !val.is_empty() {
        return Ok(val);
    }
    // If current working directory is not inside a .git repository yet, Git's `includeIf.gitdir:`
    // does not activate. Fall back to matching the current directory against registered folder scopes.
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(store) = crate::config::Store::discover(global.map(Path::to_path_buf))
        && let Ok(Some(scope)) = store.matching_scope(&cwd)
    {
        return crate::config::value(&scope.file, key);
    }
    Ok(String::new())
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

const REAUTHORIZE: &str = "GitHub App user authentication unavailable. Run zyphe-gh-auth start, then zyphe-gh-auth finish; no installation or personal-token fallback was used.";

pub fn validate_app_profile(id: &str, installation: &str) -> Result<()> {
    ensure!(
        id == "5382086" && installation == "173056379",
        "App authentication requires the zyphe-gh-auth profile (App 5382086, Installation 173056379); refusing installation-token fallback"
    );
    Ok(())
}

fn app_token_for(id: &str, installation: &str, home: &Path) -> Result<String> {
    validate_app_profile(id, installation)?;
    // `token` refreshes, validates and reads the App's separate keyring entry.
    // It must never log into gh: that would overwrite the personal account token.
    // Old helpers reject this command, so migration fails closed.
    let output = Command::new(home.join(".local/bin/zyphe-gh-auth"))
        .arg("token")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_DEBUG")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| anyhow::anyhow!(REAUTHORIZE))?;
    ensure!(output.status.success(), REAUTHORIZE);
    let token = String::from_utf8(output.stdout).map_err(|_| anyhow::anyhow!(REAUTHORIZE))?;
    let token = token.trim();
    ensure!(
        token.starts_with("ghu_") && !token.contains(char::is_whitespace),
        REAUTHORIZE
    );
    Ok(token.to_owned())
}

fn app_token(global: Option<&Path>) -> Result<String> {
    let id = effective("github.appId", global)?;
    let installation = effective("github.installationId", global)?;
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is missing")?);
    app_token_for(&id, &installation, &home)
}

fn validate_personal_token(token: &str) -> Result<()> {
    ensure!(
        token.starts_with("gho_") || token.starts_with("ghp_") || token.starts_with("github_pat_"),
        "The saved gh login is not a personal token. Run /usr/bin/gh auth login --hostname github.com --web to restore Personal Login."
    );
    Ok(())
}

pub fn personal_token_for(user: &str) -> Result<String> {
    let user = user.trim();
    ensure!(
        !user.is_empty(),
        "Set github.user for personal authentication"
    );
    let out = Command::new(GH)
        .args(["auth", "token", "--hostname", "github.com", "--user", user])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_DEBUG")
        .output()
        .context("Cannot execute /usr/bin/gh")?;
    ensure!(
        out.status.success(),
        "Personal login '{}' unavailable in gh. Run: /usr/bin/gh auth login --hostname github.com",
        user
    );
    let token = String::from_utf8(out.stdout)
        .map_err(|_| anyhow::anyhow!("Cannot decode personal login credential"))?;
    let token = token.trim();
    validate_personal_token(token)?;
    Ok(token.to_owned())
}

pub fn verify_profile_auth(
    mode: usize,
    user: &str,
    app_id: &str,
    installation_id: &str,
    _private_key: &str,
    home: &Path,
) -> Result<String> {
    match mode {
        0 => {
            let out = Command::new(GH)
                .args(["auth", "status", "--hostname", "github.com"])
                .output()
                .context("Cannot execute /usr/bin/gh")?;
            ensure!(
                out.status.success(),
                "No active global gh login found for github.com"
            );
            Ok("Inherited global GitHub login is active.".into())
        }
        1 => {
            let _token = personal_token_for(user)?;
            Ok(format!(
                "Verified personal GitHub token for '{}' via /usr/bin/gh.",
                user.trim()
            ))
        }
        2 => {
            let _token = app_token_for(app_id.trim(), installation_id.trim(), home)?;
            Ok("Verified iltumio via the zyphe-agents GitHub App user authorization.".into())
        }
        _ => anyhow::bail!("Unsupported authentication mode"),
    }
}

pub fn shim_precedes_usr_bin(shim_dir: &Path) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let target = fs::canonicalize(shim_dir).unwrap_or_else(|_| shim_dir.to_path_buf());
    let usr_bin = Path::new("/usr/bin");
    for entry in std::env::split_paths(&path_var) {
        let canonical = fs::canonicalize(&entry).unwrap_or(entry);
        if canonical == target {
            return true;
        }
        if canonical == usr_bin {
            return false;
        }
    }
    false
}

pub fn run_gh(args: &[String]) -> Result<i32> {
    run_gh_with_config(args, None)
}

pub fn run_gh_with_config(args: &[String], global: Option<&Path>) -> Result<i32> {
    let mode = effective("github.authMode", global)?;
    let mut command = Command::new(GH);
    command.args(args);
    if let Some(path) = global {
        command.env("GIT_CONFIG_GLOBAL", path);
    }
    if !mode.is_empty()
        && !args.is_empty()
        && !matches!(
            args[0].as_str(),
            "help" | "--help" | "-h" | "version" | "--version" | "completion"
        )
    {
        command
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_DEBUG")
            .env("GH_HOST", "github.com");
        match mode.as_str() {
            "personal" => {
                if args[0] != "auth"
                    || args
                        .get(1)
                        .is_some_and(|s| s == "git-credential" || s == "status")
                {
                    let user = effective("github.user", global)?;
                    command.env("GH_TOKEN", personal_token_for(&user)?);
                }
            }
            "app" => {
                ensure!(
                    args[0] != "auth"
                        || args
                            .get(1)
                            .is_some_and(|s| s == "status" || s == "git-credential"),
                    "App user credentials are managed by zyphe-gh-auth. Run zyphe-gh-auth start, then zyphe-gh-auth finish to authorize."
                );
                command.env("GH_TOKEN", app_token(global)?);
            }
            _ => anyhow::bail!("Unknown github.authMode: {mode}; refusing authentication fallback"),
        }
    }
    let status = command.status().context("Cannot run /usr/bin/gh")?;
    Ok(status.code().unwrap_or(1))
}

pub fn install_wrapper(directory: &Path, replace: bool) -> Result<PathBuf> {
    use std::os::unix::fs::symlink;
    fs::create_dir_all(directory)?;
    ensure!(
        directory.canonicalize()?.join("gh") != Path::new(GH),
        "Cannot replace the underlying /usr/bin/gh executable"
    );
    let target = directory.join("gh");
    let mut backup = None;
    if target.symlink_metadata().is_ok() {
        ensure!(
            replace,
            "{} already exists. Use --replace to back it up and replace it.",
            target.display()
        );
        let mut name = target.as_os_str().to_owned();
        name.push(format!(".bipolar-backup-{}", now()?));
        ensure!(
            !Path::new(&name).exists(),
            "Backup already exists; retry in a second"
        );
        let path = PathBuf::from(name);
        fs::rename(&target, &path)?;
        backup = Some(path);
    }
    // The binary recognizes invocation as `gh`, avoiding shell quoting and recursion.
    if let Err(error) = symlink(std::env::current_exe()?, &target) {
        if let Some(path) = backup {
            fs::rename(path, &target)
                .context("Launcher installation failed, and restoring the backup failed")?;
        }
        return Err(error).context("Cannot install launcher");
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::validate_personal_token;

    #[test]
    fn personal_login_rejects_app_credentials() {
        for token in ["ghu_synthetic", "ghs_synthetic", ""] {
            assert!(validate_personal_token(token).is_err());
        }
        for token in ["gho_synthetic", "ghp_synthetic", "github_pat_synthetic"] {
            assert!(validate_personal_token(token).is_ok());
        }
    }
}
