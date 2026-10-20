use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::PermissionsExt,
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

fn base64url(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = *chunk.get(1).unwrap_or(&0) as u32;
        let c = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (a << 16) | (b << 8) | c;
        for i in 0..(chunk.len() + 1) {
            result.push(TABLE[((n >> (18 - i * 6)) & 63) as usize] as char);
        }
    }
    result
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

#[derive(Serialize, Deserialize)]
struct CachedToken {
    token: String,
    valid_until: u64,
}

pub fn app_token_for(id: &str, installation: &str, key_input: &str, home: &Path) -> Result<String> {
    ensure!(
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
        "Missing or invalid GitHub App ID"
    );
    ensure!(
        !installation.is_empty() && installation.bytes().all(|b| b.is_ascii_digit()),
        "Missing or invalid installation ID"
    );
    let key = key_input
        .strip_prefix("~/")
        .map(|p| home.join(p))
        .unwrap_or_else(|| PathBuf::from(key_input));
    ensure!(
        key.is_absolute() && key.is_file(),
        "App private key must point to an existing absolute path"
    );
    ensure!(
        fs::metadata(&key)?.permissions().mode() & 0o077 == 0,
        "Private key must be readable only by its owner (chmod 600)"
    );
    let cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("bipolar");
    fs::create_dir_all(&cache_dir)?;
    fs::set_permissions(&cache_dir, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache_dir.join("token.lock"))?;
    lock.lock_exclusive()?;
    // Include the key's identity so a rotated key never reuses an old cached token.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    fs::read(&key)?.hash(&mut hash);
    let cache = cache_dir.join(format!("{id}-{installation}-{:x}.json", hash.finish()));
    if let Ok(bytes) = fs::read(&cache)
        && let Ok(saved) = serde_json::from_slice::<CachedToken>(&bytes)
        && saved.valid_until > now()? + 300
    {
        return Ok(saved.token);
    }
    let issued = now()?;
    let payload = serde_json::to_vec(
        &serde_json::json!({"iat": issued.saturating_sub(60), "exp": issued + 540, "iss": id}),
    )?;
    let message = format!(
        "{}.{}",
        base64url(br#"{"alg":"RS256","typ":"JWT"}"#),
        base64url(&payload)
    );
    let mut signer = Command::new("openssl")
        .args(["dgst", "-sha256", "-sign"])
        .arg(&key)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("OpenSSL is required for GitHub App authentication")?;
    signer
        .stdin
        .take()
        .context("Cannot open signer input")?
        .write_all(message.as_bytes())?;
    let signed = signer.wait_with_output()?;
    ensure!(
        signed.status.success(),
        "Cannot sign App JWT; verify the private key"
    );
    let jwt = format!("{message}.{}", base64url(&signed.stdout));
    // JWTs require Bearer authentication; gh's normal token authentication is unsuitable.
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("bipolar/0.1")
        .build()?
        .post(format!(
            "https://api.github.com/app/installations/{installation}/access_tokens"
        ))
        .bearer_auth(jwt)
        .header("Accept", "application/vnd.github+json")
        .json(&serde_json::json!({}))
        .send()
        .context("Cannot reach GitHub for an App token; personal credentials were not used")?;
    ensure!(
        response.status().is_success(),
        "Cannot obtain App token (HTTP {}); check installation and private key. Personal credentials were not used.",
        response.status()
    );
    let result: serde_json::Value = response.json()?;
    let token = result["token"]
        .as_str()
        .context("GitHub returned no installation token")?
        .to_owned();
    // Installation tokens last one hour; refresh ten minutes early.
    let saved = CachedToken {
        token: token.clone(),
        valid_until: issued + 3000,
    };
    let mut temp = tempfile::NamedTempFile::new_in(&cache_dir)?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer(&mut temp, &saved)?;
    temp.as_file().sync_all()?;
    temp.persist(cache).map_err(|e| e.error)?;
    Ok(token)
}

fn app_token(global: Option<&Path>) -> Result<String> {
    let id = effective("github.appId", global)?;
    let installation = effective("github.installationId", global)?;
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is missing")?);
    let key = effective("github.privateKey", global)?;
    app_token_for(&id, &installation, &key, &home)
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
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

pub fn verify_profile_auth(
    mode: usize,
    user: &str,
    app_id: &str,
    installation_id: &str,
    private_key: &str,
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
            let _token = app_token_for(
                app_id.trim(),
                installation_id.trim(),
                private_key.trim(),
                home,
            )?;
            Ok(format!(
                "Successfully signed JWT and minted GitHub App token (App {}, Installation {}).",
                app_id.trim(),
                installation_id.trim()
            ))
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
                if args[0] != "auth" || args.get(1).is_some_and(|s| s == "git-credential") {
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
                    "App tokens are managed automatically. Use /usr/bin/gh to manage personal logins."
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
    use super::base64url;
    #[test]
    fn jwt_encoding_has_no_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[255, 255, 255]), "____");
    }
}
