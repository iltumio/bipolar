use anyhow::{Context, Result};
use bipolar::{
    auth,
    config::{Store, ssh_command_for_key},
    skill,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Git identities and GitHub authentication, scoped by folder"
)]
struct Cli {
    /// Global Git config to manage (defaults to Git's user configuration)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Clone, ValueEnum)]
enum AuthMode {
    Inherit,
    Personal,
    App,
}

#[derive(Args)]
struct SetArgs {
    folder: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    email: Option<String>,
    #[arg(long)]
    signing_key: Option<String>,
    #[arg(long)]
    gpg_sign: Option<bool>,
    #[arg(long)]
    ssh_key: Option<PathBuf>,
    #[arg(long)]
    https_helper: Option<bool>,
    #[arg(long, value_enum)]
    auth: Option<AuthMode>,
    #[arg(long)]
    github_user: Option<String>,
    #[arg(long)]
    app_id: Option<String>,
    #[arg(long)]
    installation_id: Option<String>,
    #[arg(long)]
    private_key: Option<PathBuf>,
    /// Additional Git setting, e.g. --set commit.gpgSign=true
    #[arg(long = "set", value_name = "KEY=VALUE")]
    settings: Vec<String>,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Action {
    /// Open the native Linux desktop app
    Gui,
    /// List existing folder includes, including configurations created outside this app
    List {
        #[arg(long)]
        json: bool,
    },
    /// Create or update a folder profile and register its includeIf
    Set(Box<SetArgs>),
    /// Unregister a folder; keep its .gitconfig intact
    Remove { folder: PathBuf },
    /// Show the effective configuration and its origins for a repository or folder
    Inspect { repository: PathBuf },
    /// Save all folder profiles and their configurations to a portable backup file (.json)
    #[command(visible_alias = "export")]
    Backup {
        /// Destination backup file path (e.g. bipolar-backup.json)
        file: PathBuf,
    },
    /// Restore folder profiles and global includes from a portable backup file (.json)
    #[command(visible_alias = "import")]
    Restore {
        /// Backup file path to restore from
        file: PathBuf,
        /// Preview the exact configuration changes without writing files
        #[arg(long)]
        dry_run: bool,
    },
    /// Run GitHub CLI with the current repository's authentication profile
    Gh {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Install a gh launcher into a directory on PATH (existing files require --replace)
    InstallGh {
        #[arg(long)]
        bin_dir: Option<PathBuf>,
        #[arg(long)]
        replace: bool,
    },
    /// Install the Bipolar AI agent skill (SKILL.md) to use the CLI and manage profiles
    InstallSkill {
        /// Custom base skills directory (defaults to ~/.agents/skills)
        #[arg(long)]
        skills_dir: Option<PathBuf>,
        /// Overwrite an existing SKILL.md or skill link if modified
        #[arg(long)]
        replace: bool,
        /// Do not create symlinks in detected agent directories (~/.claude, ~/.codex, ~/.cursor, ~/.gemini)
        #[arg(long)]
        no_link: bool,
        /// Print the embedded SKILL.md to stdout without writing files
        #[arg(long)]
        print: bool,
    },
    /// Remove backup files (.bipolar-backup-*) across global and profile directories
    CleanBackups,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("bipolar: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if std::env::args_os()
        .next()
        .and_then(|p| PathBuf::from(p).file_name().map(|s| s.to_owned()))
        .is_some_and(|s| s == "gh")
    {
        std::process::exit(auth::run_gh(&std::env::args().skip(1).collect::<Vec<_>>())?);
    }
    let cli = Cli::parse();
    let override_config = cli.config.clone();
    let store = Store::discover(cli.config)?;
    let override_config = override_config.map(|_| store.global.clone());
    match cli.command.unwrap_or(Action::Gui) {
        Action::Gui => {
            #[cfg(feature = "desktop")]
            {
                bipolar::desktop::run(store)?;
            }
            #[cfg(not(feature = "desktop"))]
            {
                anyhow::bail!("Desktop support was disabled at build time");
            }
        }
        Action::List { json } => {
            let scopes = store.scopes()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&scopes)?);
            } else if scopes.is_empty() {
                println!("No folder profiles in {}", store.global.display());
            } else {
                for scope in scopes {
                    println!(
                        "{}\n  {} <{}> · {}\n  {}",
                        scope.folder.display(),
                        scope.name,
                        scope.email,
                        if scope.auth.is_empty() {
                            "inherited"
                        } else {
                            &scope.auth
                        },
                        scope.file.display()
                    );
                }
            }
        }
        Action::Set(args) => {
            let SetArgs {
                folder,
                name,
                email,
                signing_key,
                gpg_sign,
                ssh_key,
                https_helper,
                auth,
                github_user,
                app_id,
                installation_id,
                private_key,
                settings,
                dry_run,
            } = *args;
            let mut changes = Vec::new();
            for (key, val) in [
                ("user.name", name),
                ("user.email", email),
                ("user.signingKey", signing_key),
                ("github.user", github_user),
                ("github.appId", app_id),
                ("github.installationId", installation_id),
                (
                    "github.privateKey",
                    private_key.map(|p| p.to_string_lossy().into_owned()),
                ),
            ] {
                if let Some(val) = val {
                    changes.push((key.to_owned(), Some(val)));
                }
            }
            if let Some(sign) = gpg_sign {
                changes.push(("commit.gpgSign".into(), sign.then(|| "true".to_owned())));
            }
            if let Some(ssh) = ssh_key {
                let s = ssh.to_string_lossy();
                changes.push((
                    "core.sshCommand".into(),
                    (!s.is_empty()).then(|| ssh_command_for_key(&s)),
                ));
            }
            if let Some(helper) = https_helper {
                let shim = store.home.join(".local/bin/gh");
                changes.push((
                    "credential.helper".into(),
                    helper.then(|| format!("!{} auth git-credential", shim.display())),
                ));
            }
            if let Some(auth) = auth {
                changes.push((
                    "github.authMode".into(),
                    match auth {
                        AuthMode::Inherit => None,
                        AuthMode::Personal => Some("personal".into()),
                        AuthMode::App => Some("app".into()),
                    },
                ));
            }
            for item in settings {
                let (key, val) = item.split_once('=').context("--set requires KEY=VALUE")?;
                changes.push((key.to_owned(), Some(val.to_owned())));
            }
            let plan = store.plan(&folder, &changes)?;
            println!("{}", plan.preview());
            if !dry_run {
                let backups = store.apply(&plan)?;
                println!("Saved. {} backup(s).", backups.len());
                for backup in backups {
                    println!("  {}", backup.display());
                }
            }
        }
        Action::Remove { folder } => {
            let backups = store.remove(&folder)?;
            println!(
                "Include removed; profile file retained. {} backup(s).",
                backups.len()
            );
        }
        Action::Inspect { repository } => {
            let is_git_repo = std::process::Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(["rev-parse", "--git-dir"])
                .output()
                .is_ok_and(|o| o.status.success());
            let mut command = std::process::Command::new("git");
            command.arg("-C").arg(&repository).args([
                "config",
                "--show-origin",
                "--show-scope",
                "--get-regexp",
                r"^(user\.|github\.|commit\.gpgsign|gpg\.format|core\.sshcommand|credential\.helper|includeif\.)",
            ]);
            if let Some(path) = override_config {
                command.env("GIT_CONFIG_GLOBAL", path);
            }
            let status = command.status()?;
            if !is_git_repo && let Some(scope) = store.matching_scope(&repository)? {
                println!(
                    "# Note: {} is not a Git repository yet (includeIf.gitdir activates inside repositories).\n# Matching folder scope: {} ({})",
                    repository.display(),
                    scope.folder.display(),
                    scope.file.display()
                );
                let _ = std::process::Command::new("git")
                    .args([
                        "config",
                        "--show-origin",
                        "--file",
                        &scope.file.to_string_lossy(),
                        "--get-regexp",
                        r"^(user\.|github\.|commit\.gpgsign|gpg\.format|core\.sshcommand|credential\.helper)",
                    ])
                    .status();
                return Ok(());
            }
            anyhow::ensure!(status.success(), "Cannot inspect repository configuration");
        }
        Action::Backup { file } => {
            let bundle = store.export_backup(&file)?;
            println!(
                "Saved {} profile(s) to {}",
                bundle.profiles.len(),
                file.display()
            );
        }
        Action::Restore { file, dry_run } => {
            let plan = store.plan_restore(&file)?;
            println!("{}", plan.preview());
            if !dry_run {
                let backups = store.apply_restore(&plan)?;
                println!(
                    "Restored {} profile(s). {} safety backup(s) created.",
                    plan.files.len(),
                    backups.len()
                );
                for backup in backups {
                    println!("  {}", backup.display());
                }
            }
        }
        Action::Gh { args } => {
            std::process::exit(auth::run_gh_with_config(&args, override_config.as_deref())?);
        }
        Action::InstallGh { bin_dir, replace } => {
            let dir = bin_dir.unwrap_or_else(|| store.home.join(".local/bin"));
            let path = auth::install_wrapper(&dir, replace)?;
            println!(
                "Installed {}. Ensure {} comes before /usr/bin on PATH.",
                path.display(),
                dir.display()
            );
            println!(
                "For Git HTTPS, set your credential helper to: !{} auth git-credential",
                path.display()
            );
        }
        Action::InstallSkill {
            skills_dir,
            replace,
            no_link,
            print,
        } => {
            if print {
                print!("{}", skill::SKILL_CONTENT);
                return Ok(());
            }
            let installed = skill::install(&store.home, skills_dir.as_deref(), replace, no_link)?;
            println!("Installed skill: {}", installed.skill_file.display());
            for link in installed.linked {
                println!("Linked skill:    {}", link.display());
            }
        }
        Action::CleanBackups => {
            let removed = store.clean_backups()?;
            println!("Removed {removed} backup file(s).");
        }
    }
    Ok(())
}
