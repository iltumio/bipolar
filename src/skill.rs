use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

pub const SKILL_NAME: &str = "bipolar";
pub const SKILL_CONTENT: &str = include_str!("../data/SKILL.md");

const LINKED_AGENT_DIRS: [&str; 4] = [".claude", ".codex", ".cursor", ".gemini"];

#[derive(Debug, Clone)]
pub struct InstalledSkill {
    pub skill_file: PathBuf,
    pub linked: Vec<PathBuf>,
}

fn write_skill_file(skill_dir: &Path, replace: bool) -> Result<PathBuf> {
    fs::create_dir_all(skill_dir)
        .with_context(|| format!("Cannot create skill directory {}", skill_dir.display()))?;
    let skill_file = skill_dir.join("SKILL.md");
    if skill_file.exists() && !replace {
        let existing = fs::read_to_string(&skill_file).unwrap_or_default();
        if existing == SKILL_CONTENT {
            return Ok(skill_file);
        }
        anyhow::bail!(
            "{} already exists with different content. Use --replace to overwrite it.",
            skill_file.display()
        );
    }
    let mut tmp = NamedTempFile::new_in(skill_dir)?;
    tmp.write_all(SKILL_CONTENT.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(&skill_file).map_err(|e| e.error)?;
    Ok(skill_file)
}

pub fn install(
    home: &Path,
    skills_dir: Option<&Path>,
    replace: bool,
    no_link: bool,
) -> Result<InstalledSkill> {
    let base_skills_dir = skills_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".agents/skills"));
    let canonical_skill_dir = base_skills_dir.join(SKILL_NAME);
    ensure!(
        !canonical_skill_dir.is_symlink(),
        "{} is a symlink; remove it or use a non-symlink target directory",
        canonical_skill_dir.display()
    );
    let skill_file = write_skill_file(&canonical_skill_dir, replace)?;
    let resolved_skill_dir =
        fs::canonicalize(&canonical_skill_dir).unwrap_or(canonical_skill_dir.clone());

    let mut linked = Vec::new();
    if !no_link && skills_dir.is_none() {
        for agent_dir in LINKED_AGENT_DIRS {
            let root = home.join(agent_dir);
            if !root.is_dir() {
                continue;
            }
            let target_skills = root.join("skills");
            fs::create_dir_all(&target_skills)?;
            let link_path = target_skills.join(SKILL_NAME);
            if let Ok(meta) = link_path.symlink_metadata() {
                if meta.file_type().is_symlink()
                    && fs::canonicalize(&link_path).ok().as_ref() == Some(&resolved_skill_dir)
                {
                    linked.push(link_path);
                    continue;
                }
                if !replace {
                    continue;
                }
                if meta.is_dir() {
                    fs::remove_dir_all(&link_path)?;
                } else {
                    fs::remove_file(&link_path)?;
                }
            }
            symlink(&resolved_skill_dir, &link_path)
                .with_context(|| format!("Cannot link skill into {}", link_path.display()))?;
            linked.push(link_path);
        }
    }

    Ok(InstalledSkill { skill_file, linked })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn installs_skill_and_symlinks_existing_agent_directories() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".codex/skills")).unwrap();

        let installed = install(home, None, false, false).unwrap();
        assert!(installed.skill_file.is_file());
        assert_eq!(
            fs::read_to_string(&installed.skill_file).unwrap(),
            SKILL_CONTENT
        );
        assert_eq!(installed.linked.len(), 2);
        assert!(home.join(".claude/skills/bipolar").is_symlink());
        assert!(home.join(".codex/skills/bipolar").is_symlink());
        assert!(!home.join(".cursor/skills/bipolar").exists());
    }

    #[test]
    fn refuses_to_overwrite_modified_skill_without_replace() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();
        let skill_dir = home.join(".agents/skills/bipolar");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "custom content").unwrap();

        assert!(install(home, None, false, false).is_err());
        assert_eq!(
            fs::read_to_string(skill_dir.join("SKILL.md")).unwrap(),
            "custom content"
        );

        let installed = install(home, None, true, false).unwrap();
        assert_eq!(
            fs::read_to_string(installed.skill_file).unwrap(),
            SKILL_CONTENT
        );
    }
}
