use bipolar::config::{Store, value};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn fixture() -> (TempDir, Store) {
    let temp = TempDir::new().unwrap();
    let store = Store {
        global: temp.path().join("global.gitconfig"),
        home: temp.path().to_path_buf(),
    };
    (temp, store)
}
fn changes(name: &str) -> Vec<(String, Option<String>)> {
    vec![("user.name".into(), Some(name.into()))]
}
fn git(path: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn effective(store: &Store, repo: &Path, key: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", key])
        .env("GIT_CONFIG_GLOBAL", &store.global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn registration_is_idempotent_and_repositories_inherit_it() {
    let (temp, store) = fixture();
    let folder = temp.path().join("personal projects");
    fs::create_dir(&folder).unwrap();
    let plan = store.plan(&folder, &changes("Personal")).unwrap();
    assert!(!store.global.exists());
    assert!(!folder.join(".gitconfig").exists());
    store.apply(&plan).unwrap();
    let original = fs::read(&store.global).unwrap();
    store
        .apply(&store.plan(&folder, &changes("Personal")).unwrap())
        .unwrap();
    assert_eq!(original, fs::read(&store.global).unwrap());
    assert_eq!(store.scopes().unwrap().len(), 1);
    let repo = folder.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    assert_eq!(effective(&store, &repo, "user.name"), "Personal");
}

#[test]
fn existing_tilde_includes_are_reused_and_unknown_settings_survive() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    fs::write(&store.global, "# keep this comment\n[alias]\n  hi = status\n[includeIf \"gitdir:~/work/\"]\n  path = ~/work/.gitconfig\n").unwrap();
    fs::write(
        folder.join(".gitconfig"),
        "# profile comment\n[user]\n name = Old\n[custom]\n value = untouched\n",
    )
    .unwrap();
    let original = fs::read(&store.global).unwrap();
    let plan = store.plan(&folder, &changes("New")).unwrap();
    let backups = store.apply(&plan).unwrap();
    assert_eq!(backups.len(), 2);
    assert_eq!(original, fs::read(&store.global).unwrap());
    let profile = folder.join(".gitconfig");
    assert_eq!(value(&profile, "custom.value").unwrap(), "untouched");
    assert_eq!(value(&profile, "user.name").unwrap(), "New");
    assert!(
        fs::read_to_string(profile)
            .unwrap()
            .contains("# profile comment")
    );
}

#[test]
fn remove_only_unregisters_the_include_and_creates_backup() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    store
        .apply(&store.plan(&folder, &changes("Work")).unwrap())
        .unwrap();
    let profile = fs::read(folder.join(".gitconfig")).unwrap();
    let backups = store.remove(&folder).unwrap();
    assert_eq!(backups.len(), 1);
    assert!(backups[0].exists());
    assert!(store.scopes().unwrap().is_empty());
    assert_eq!(profile, fs::read(folder.join(".gitconfig")).unwrap());
}

#[test]
fn stale_preview_never_overwrites_external_edits() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    let plan = store.plan(&folder, &changes("Work")).unwrap();
    fs::write(&store.global, "[user]\n name = External\n").unwrap();
    assert!(store.apply(&plan).is_err());
    assert!(!folder.join(".gitconfig").exists());
    assert_eq!(value(&store.global, "user.name").unwrap(), "External");
}

#[test]
fn malformed_config_is_never_overwritten() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    fs::write(&store.global, "[broken\n").unwrap();
    assert!(store.plan(&folder, &changes("Work")).is_err());
    assert_eq!(fs::read_to_string(&store.global).unwrap(), "[broken\n");
}

#[test]
fn symlinked_profiles_keep_the_symlink() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    let target = temp.path().join("dotfiles.gitconfig");
    fs::write(&target, "[user]\n name = Original\n").unwrap();
    std::os::unix::fs::symlink(&target, folder.join(".gitconfig")).unwrap();
    store
        .apply(&store.plan(&folder, &changes("Updated")).unwrap())
        .unwrap();
    assert!(folder.join(".gitconfig").is_symlink());
    assert_eq!(value(&target, "user.name").unwrap(), "Updated");
}

#[test]
fn scoped_repository_and_its_external_worktree_inherit_main_gitdir_identity() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work");
    fs::create_dir(&folder).unwrap();
    store
        .apply(&store.plan(&folder, &changes("Work")).unwrap())
        .unwrap();
    let repo = folder.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--allow-empty",
            "-m",
            "Initial",
        ],
    );
    let worktree = temp.path().join("external-worktree");
    git(
        &repo,
        &["worktree", "add", "-b", "topic", worktree.to_str().unwrap()],
    );
    assert_eq!(effective(&store, &worktree, "user.name"), "Work");
}

#[test]
fn unsafe_globs_and_ambiguous_include_duplicates_are_rejected() {
    let (temp, store) = fixture();
    let folder = temp.path().join("work[1]");
    fs::create_dir(&folder).unwrap();
    assert!(store.plan(&folder, &changes("Work")).is_err());
    let normal = temp.path().join("work");
    fs::create_dir(&normal).unwrap();
    fs::write(
        &store.global,
        format!(
            "[includeIf \"gitdir:{}/\"]\n path = a\n path = b\n",
            normal.display()
        ),
    )
    .unwrap();
    assert!(store.plan(&normal, &changes("Work")).is_err());
}

#[test]
fn nested_scope_precedence_is_automatic_even_when_parent_added_after_child() {
    let (temp, store) = fixture();
    let parent = temp.path().join("work");
    let child = parent.join("special");
    fs::create_dir_all(&child).unwrap();
    // Register child first, then parent: Store::plan automatically places parent before child.
    store
        .apply(&store.plan(&child, &changes("Child")).unwrap())
        .unwrap();
    store
        .apply(&store.plan(&parent, &changes("Parent")).unwrap())
        .unwrap();
    let child_repo = child.join("repo");
    fs::create_dir(&child_repo).unwrap();
    git(&child_repo, &["init"]);
    assert_eq!(effective(&store, &child_repo, "user.name"), "Child");

    let parent_repo = parent.join("other-repo");
    fs::create_dir(&parent_repo).unwrap();
    git(&parent_repo, &["init"]);
    assert_eq!(effective(&store, &parent_repo, "user.name"), "Parent");
}
