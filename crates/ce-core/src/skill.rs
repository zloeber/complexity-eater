use std::path::{Path, PathBuf};

const FALLBACK: &str = "\
# Complexity Eater

The skill file was not installed with this package.
Discover the live protocol at `/.well-known/agent-workspace`, `/protocol`, and `/skills`.
Join with the invitation, then observe, checkout, propose, and commit only after review.
";

pub fn load_skill() -> String {
    for path in candidates() {
        if path.is_file() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                return text;
            }
        }
    }
    include_str!("../../../skills/commonroom/SKILL.md").to_string()
}

fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(override_path) =
        std::env::var_os("CE_SKILL").or_else(|| std::env::var_os("COMMONROOM_SKILL"))
    {
        paths.push(PathBuf::from(override_path));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cursor = exe.parent().map(Path::to_path_buf);
        while let Some(parent) = cursor {
            paths.push(parent.join("skills/commonroom/SKILL.md"));
            paths.push(parent.join("data/SKILL.md"));
            cursor = parent.parent().map(Path::to_path_buf);
        }
    }
    paths.push(PathBuf::from("skills/commonroom/SKILL.md"));
    paths
}

pub fn fallback_skill() -> &'static str {
    FALLBACK
}
