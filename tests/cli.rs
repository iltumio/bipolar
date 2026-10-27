use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(global: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bipolar"))
        .arg("--config")
        .arg(global)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_round_trip_and_dry_run() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    let folder = folder.to_str().unwrap();
    let args = [
        "set",
        folder,
        "--name",
        "Example",
        "--email",
        "dev@example.com",
        "--auth",
        "personal",
        "--github-user",
        "example",
        "--set",
        "commit.gpgSign=true",
    ];
    let mut dry = args.to_vec();
    dry.push("--dry-run");
    let output = cli(&global, &dry);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!global.exists());
    let output = cli(&global, &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = cli(&global, &["list", "--json"]);
    assert!(output.status.success());
    let scopes: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(scopes[0]["name"], "Example");
    assert_eq!(scopes[0]["auth"], "personal");
    let output = cli(&global, &["remove", folder]);
    assert!(output.status.success());
    assert!(Path::new(folder).join(".gitconfig").exists());
}

#[test]
fn incomplete_app_configuration_is_rejected_before_writing() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let output = cli(
        &global,
        &[
            "set",
            temp.path().to_str().unwrap(),
            "--auth",
            "app",
            "--app-id",
            "123",
        ],
    );
    assert!(!output.status.success());
    assert!(!global.exists());
    assert!(!temp.path().join(".gitconfig").exists());
}

#[test]
fn launcher_does_not_replace_existing_executable_without_explicit_flag() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("gh"), "existing launcher").unwrap();
    let output = cli(&global, &["install-gh", "--bin-dir", bin.to_str().unwrap()]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(bin.join("gh")).unwrap(),
        "existing launcher"
    );
}

#[test]
fn custom_config_is_used_by_inspect() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(&global, "[user]\n name = Isolated Identity\n").unwrap();
    let output = cli(&global, &["inspect", repo.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Isolated Identity"));
}

#[test]
fn unknown_auth_mode_never_falls_back_to_personal_login() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    fs::write(&global, "[github]\n authMode = invalid\n").unwrap();
    let output = cli(&global, &["gh", "--", "api", "user"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing authentication fallback"));
}

#[test]
fn installed_launcher_dispatches_to_real_gh() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let bin = temp.path().join("bin");
    let result = cli(&global, &["install-gh", "--bin-dir", bin.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(bin.join("gh").is_symlink());
    let output = Command::new(bin.join("gh"))
        .arg("--version")
        .env("GIT_CONFIG_GLOBAL", &global)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("gh version "));
}

#[test]
fn inspect_resolves_relative_global_path_before_changing_repository() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    assert!(
        Command::new("git")
            .arg("init")
            .arg(&repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(
        temp.path().join("global"),
        "[user]\n name = Relative Config\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bipolar"))
        .current_dir(temp.path())
        .args(["--config", "global", "inspect", "repo"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Relative Config"));
}

#[test]
fn install_skill_writes_skill_md_and_supports_print() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let skills_dir = temp.path().join("skills");

    let print_out = cli(&global, &["install-skill", "--print"]);
    assert!(print_out.status.success());
    let printed = String::from_utf8_lossy(&print_out.stdout);
    assert!(printed.contains("name: bipolar"));

    let install_out = cli(
        &global,
        &[
            "install-skill",
            "--skills-dir",
            skills_dir.to_str().unwrap(),
        ],
    );
    assert!(
        install_out.status.success(),
        "{}",
        String::from_utf8_lossy(&install_out.stderr)
    );
    let skill_md = skills_dir.join("bipolar/SKILL.md");
    assert!(skill_md.is_file());
    assert_eq!(fs::read_to_string(&skill_md).unwrap(), printed);
}

#[test]
fn backup_and_restore_relocate_paths_on_new_machine() {
    let machine_a = TempDir::new().unwrap();
    let home_a = machine_a.path().join("home-alice");
    let work_a = home_a.join("dev/work");
    let client_a = home_a.join("dev/work/client");
    fs::create_dir_all(&client_a).unwrap();
    let global_a = home_a.join(".gitconfig");

    for (folder, name, email, user) in [
        (&work_a, "Alice Work", "alice@work.example", "alice-work"),
        (
            &client_a,
            "Alice Client",
            "alice@client.example",
            "alice-client",
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_bipolar"))
            .env("HOME", &home_a)
            .args([
                "--config",
                global_a.to_str().unwrap(),
                "set",
                folder.to_str().unwrap(),
                "--name",
                name,
                "--email",
                email,
                "--auth",
                "personal",
                "--github-user",
                user,
                "--https-helper",
                "true",
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let backup_file = machine_a.path().join("bipolar-backup.json");
    let export_out = Command::new(env!("CARGO_BIN_EXE_bipolar"))
        .env("HOME", &home_a)
        .args([
            "--config",
            global_a.to_str().unwrap(),
            "backup",
            backup_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        export_out.status.success(),
        "{}",
        String::from_utf8_lossy(&export_out.stderr)
    );
    let raw_backup = fs::read_to_string(&backup_file).unwrap();
    assert!(raw_backup.contains("~/dev/work"));

    // Restore onto Machine B with a different $HOME and non-existent profile directories.
    let machine_b = TempDir::new().unwrap();
    let home_b = machine_b.path().join("home-bob");
    fs::create_dir_all(&home_b).unwrap();
    let global_b = home_b.join(".gitconfig");

    let dry_out = Command::new(env!("CARGO_BIN_EXE_bipolar"))
        .env("HOME", &home_b)
        .args([
            "--config",
            global_b.to_str().unwrap(),
            "restore",
            backup_file.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert!(dry_out.status.success());
    assert!(!global_b.exists());

    let restore_out = Command::new(env!("CARGO_BIN_EXE_bipolar"))
        .env("HOME", &home_b)
        .args([
            "--config",
            global_b.to_str().unwrap(),
            "restore",
            backup_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        restore_out.status.success(),
        "{}",
        String::from_utf8_lossy(&restore_out.stderr)
    );

    let work_b_cfg = home_b.join("dev/work/.gitconfig");
    let client_b_cfg = home_b.join("dev/work/client/.gitconfig");
    assert!(work_b_cfg.is_file());
    assert!(client_b_cfg.is_file());
    // Credential helper path inside .gitconfig should also be relocated from home_a to home_b.
    let work_b_text = fs::read_to_string(&work_b_cfg).unwrap();
    assert!(work_b_text.contains(&home_b.display().to_string()));
    assert!(!work_b_text.contains(&home_a.display().to_string()));
}

fn app_shim_fixture(helper: &str) -> (TempDir, Command) {
    use std::os::unix::fs::PermissionsExt;
    let temp = TempDir::new().unwrap();
    let home = temp.path();
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    let path = home.join(".local/bin/zyphe-gh-auth");
    fs::write(&path, helper).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        home.join("global"),
        "[github]\nauthMode = app\nappId = 5382086\ninstallationId = 173056379\n",
    )
    .unwrap();
    fs::create_dir(home.join("gh-config")).unwrap();
    fs::write(home.join("gh-config/config.yml"), "aliases:\n  bipolar-auth-test: '!test \"$GH_TOKEN\" = ghu_synthetic && test -z \"${GITHUB_TOKEN+x}\" && test -z \"${GH_DEBUG+x}\" && printf clean'\n").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_bipolar"));
    command
        .current_dir(home)
        .env("HOME", home)
        .env("GH_CONFIG_DIR", home.join("gh-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GH_TOKEN", "ghs_synthetic-inherited-token")
        .env("GITHUB_TOKEN", "ghs_synthetic-inherited-token")
        .env("GH_DEBUG", "api")
        .arg("--config")
        .arg(home.join("global"))
        .args(["gh", "--", "bipolar-auth-test"]);
    (temp, command)
}

#[test]
fn app_shim_reads_separate_user_token_on_each_call() {
    let (temp, mut command) = app_shim_fixture(
        r#"#!/bin/sh
[ -z "${GH_TOKEN+x}" ] && [ -z "${GITHUB_TOKEN+x}" ] && [ -z "${GH_DEBUG+x}" ] || exit 9
printf '%s\n' "$1" >> "$HOME/calls"
[ "$1" = token ] || exit 8
printf ghu_synthetic
"#,
    );
    for _ in 0..2 {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"clean");
    }
    assert_eq!(
        fs::read_to_string(temp.path().join("calls")).unwrap(),
        "token\ntoken\n"
    );
    assert!(!temp.path().join(".cache/bipolar").exists());
}

#[test]
fn app_shim_fails_closed_and_redacts_helper_output() {
    for action in ["token"] {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$HOME/calls\"\nif [ \"$1\" = {action} ]; then\n echo sensitive-helper-output\n echo sensitive-helper-error >&2\n exit 1\nfi\n"
        );
        let (temp, mut command) = app_shim_fixture(&script);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("zyphe-gh-auth start"));
        assert!(error.contains("zyphe-gh-auth finish"));
        assert!(!error.contains("sensitive-helper"));
        assert!(!error.contains("ghs_"));
        let calls = fs::read_to_string(temp.path().join("calls")).unwrap();
        assert_eq!(calls, "token\n");
    }
}

#[test]
fn app_shim_missing_helper_requires_reauthorization() {
    let (temp, mut command) = app_shim_fixture("#!/bin/sh\nexit 0\n");
    fs::remove_file(temp.path().join(".local/bin/zyphe-gh-auth")).unwrap();
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("zyphe-gh-auth start"));
}

#[test]
fn user_app_profile_needs_no_private_key() {
    let temp = TempDir::new().unwrap();
    let global = temp.path().join("global");
    let output = cli(
        &global,
        &[
            "set",
            temp.path().to_str().unwrap(),
            "--auth",
            "app",
            "--app-id",
            "5382086",
            "--installation-id",
            "173056379",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn app_shim_rejects_non_user_tokens_without_exposing_them() {
    for token in ["ghs_synthetic", "ghp_synthetic", "ghu_bad token", ""] {
        let script = format!("#!/bin/sh\nprintf '%s' '{token}'\n");
        let (_temp, mut command) = app_shim_fixture(&script);
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("zyphe-gh-auth start"));
        assert!(!error.contains("synthetic"));
    }
}
