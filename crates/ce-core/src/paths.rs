use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Platform configuration directory for Complexity Eater.
///
/// Workspace metadata does not live here. It stays beside the substrate file.
pub fn config_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("ce");
        }
    }
    platform_config_dir()
}

/// Platform data directory for Complexity Eater.
pub fn data_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("ce");
        }
    }
    platform_data_dir()
}

#[cfg(target_os = "macos")]
fn platform_config_dir() -> PathBuf {
    home_dir().join("Library/Application Support/ce")
}

#[cfg(target_os = "windows")]
fn platform_config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|path| PathBuf::from(path).join("ce"))
        .unwrap_or_else(|| home_dir().join("ce"))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_config_dir() -> PathBuf {
    home_dir().join(".config/ce")
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> PathBuf {
    home_dir().join("Library/Application Support/ce")
}

#[cfg(target_os = "windows")]
fn platform_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("ce"))
        .unwrap_or_else(|| home_dir().join("ce"))
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn platform_data_dir() -> PathBuf {
    home_dir().join(".local/share/ce")
}

pub fn resolve_user_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let expanded = if text == "~" {
        home_dir()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        path.to_path_buf()
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(expanded)
    };
    resolve_existing_parents(&normalize(&absolute))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve_existing_parents(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cursor = path.to_path_buf();
    while !cursor.as_os_str().is_empty() && !cursor.exists() {
        match cursor.file_name() {
            Some(name) => {
                missing.push(name.to_os_string());
                if !cursor.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    let mut resolved = std::fs::canonicalize(&cursor).unwrap_or(cursor);
    for part in missing.iter().rev() {
        resolved.push(part);
    }
    resolved
}
