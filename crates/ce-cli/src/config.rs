use std::path::{Path, PathBuf};

use ce_core::config_dir;
use ce_protocol::{invalid_request, CommonroomError};
use serde::Deserialize;

/// Room settings loaded from TOML. A workspace file overrides the user file,
/// and `--config` overrides both. Explicit CLI flags win after this merge.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct RoomFile {
    pub mode: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub transport: Option<String>,
    pub baton: BatonFile,
}

/// Settings for the baton collaboration addon. Omitted `max_renewals` means a
/// holder may renew until the grant expires. `0` refuses every renewal.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct BatonFile {
    pub max_minutes: Option<i64>,
    pub default_minutes: Option<i64>,
    pub max_renewals: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct ServeOverrides {
    pub mode: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub transport: Option<String>,
    pub baton_max_minutes: Option<i64>,
    pub baton_default_minutes: Option<i64>,
    pub baton_max_renewals: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRoom {
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub name: String,
    pub transport: Option<String>,
    pub baton_max_minutes: i64,
    pub baton_default_minutes: i64,
    pub baton_max_renewals: Option<i64>,
}

pub fn workspace_config_path(substrate: &Path) -> PathBuf {
    let mut name = substrate.file_name().unwrap_or_default().to_os_string();
    name.push(".commonroom.toml");
    substrate.with_file_name(name)
}

pub fn user_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_room_file(
    substrate: &Path,
    explicit: Option<&Path>,
) -> Result<RoomFile, CommonroomError> {
    let user = user_config_path();
    let user = user.is_file().then_some(user);
    let workspace = workspace_config_path(substrate);
    let workspace = workspace.is_file().then_some(workspace);
    load_layered(user.as_deref(), workspace.as_deref(), explicit)
}

pub fn load_layered(
    user: Option<&Path>,
    workspace: Option<&Path>,
    explicit: Option<&Path>,
) -> Result<RoomFile, CommonroomError> {
    let mut file = RoomFile::default();
    if let Some(path) = user {
        file = merge(file, read_room_file(path)?);
    }
    if let Some(path) = workspace {
        file = merge(file, read_room_file(path)?);
    }
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(invalid_request(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        file = merge(file, read_room_file(path)?);
    }
    validate_file(&file)?;
    Ok(file)
}

pub fn resolve_room(
    file: RoomFile,
    over: ServeOverrides,
    fallback_name: &str,
) -> Result<ResolvedRoom, CommonroomError> {
    let baton_max_minutes = over
        .baton_max_minutes
        .or(file.baton.max_minutes)
        .unwrap_or(15);
    let baton_default_minutes = over
        .baton_default_minutes
        .or(file.baton.default_minutes)
        .unwrap_or(baton_max_minutes.min(15));
    let resolved = ResolvedRoom {
        mode: over.mode.or(file.mode).unwrap_or_else(|| "review".into()),
        host: over
            .host
            .or(file.host)
            .unwrap_or_else(|| "127.0.0.1".into()),
        port: over.port.or(file.port).unwrap_or(8000),
        name: over
            .name
            .or(file.name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| fallback_name.to_string()),
        transport: over
            .transport
            .or(file.transport)
            .filter(|value| !value.is_empty()),
        baton_max_minutes,
        baton_default_minutes,
        baton_max_renewals: over.baton_max_renewals.or(file.baton.max_renewals),
    };
    if resolved.mode != "review" && resolved.mode != "baton" {
        return Err(invalid_request("mode must be review or baton"));
    }
    if resolved.baton_max_minutes <= 0 {
        return Err(invalid_request("baton max minutes must be positive"));
    }
    if resolved.baton_default_minutes <= 0
        || resolved.baton_default_minutes > resolved.baton_max_minutes
    {
        return Err(invalid_request(
            "baton default minutes must be positive and no greater than the max",
        ));
    }
    if resolved.baton_max_renewals.is_some_and(|value| value < 0) {
        return Err(invalid_request(
            "baton max renewals must be zero or positive",
        ));
    }
    Ok(resolved)
}

fn merge(base: RoomFile, over: RoomFile) -> RoomFile {
    RoomFile {
        mode: over.mode.or(base.mode),
        host: over.host.or(base.host),
        port: over.port.or(base.port),
        name: over.name.or(base.name),
        transport: over.transport.or(base.transport),
        baton: BatonFile {
            max_minutes: over.baton.max_minutes.or(base.baton.max_minutes),
            default_minutes: over.baton.default_minutes.or(base.baton.default_minutes),
            max_renewals: over.baton.max_renewals.or(base.baton.max_renewals),
        },
    }
}

fn read_room_file(path: &Path) -> Result<RoomFile, CommonroomError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| invalid_request(format!("cannot read config {}: {err}", path.display())))?;
    toml::from_str(&text)
        .map_err(|err| invalid_request(format!("invalid config {}: {err}", path.display())))
}

fn validate_file(file: &RoomFile) -> Result<(), CommonroomError> {
    if let Some(mode) = &file.mode {
        if mode != "review" && mode != "baton" {
            return Err(invalid_request("mode must be review or baton"));
        }
    }
    if file.baton.max_minutes.is_some_and(|value| value <= 0) {
        return Err(invalid_request("baton max minutes must be positive"));
    }
    if file.baton.default_minutes.is_some_and(|value| value <= 0) {
        return Err(invalid_request("baton default minutes must be positive"));
    }
    if let (Some(default), Some(max)) = (file.baton.default_minutes, file.baton.max_minutes) {
        if default > max {
            return Err(invalid_request(
                "baton default minutes must be no greater than the max",
            ));
        }
    }
    if file.baton.max_renewals.is_some_and(|value| value < 0) {
        return Err(invalid_request(
            "baton max renewals must be zero or positive",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_settings_override_the_user_file() {
        let dir = std::env::temp_dir().join(format!(
            "ce-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let user = dir.join("config.toml");
        let workspace = dir.join("notes.md.commonroom.toml");
        std::fs::write(
            &user,
            "mode = \"review\"\nhost = \"localhost\"\n\n[baton]\nmax_minutes = 20\ndefault_minutes = 5\n",
        )
        .unwrap();
        std::fs::write(
            &workspace,
            "mode = \"baton\"\n\n[baton]\nmax_renewals = 2\n",
        )
        .unwrap();
        let file = load_layered(Some(&user), Some(&workspace), None).unwrap();
        assert_eq!(file.mode.as_deref(), Some("baton"));
        assert_eq!(file.host.as_deref(), Some("localhost"));
        assert_eq!(file.baton.max_minutes, Some(20));
        assert_eq!(file.baton.default_minutes, Some(5));
        assert_eq!(file.baton.max_renewals, Some(2));
        let resolved = resolve_room(
            file,
            ServeOverrides {
                baton_max_minutes: Some(8),
                ..ServeOverrides::default()
            },
            "Ada",
        )
        .unwrap();
        assert_eq!(resolved.mode, "baton");
        assert_eq!(resolved.baton_max_minutes, 8);
        assert_eq!(resolved.baton_default_minutes, 5);
        assert_eq!(resolved.baton_max_renewals, Some(2));
        assert_eq!(resolved.name, "Ada");
        std::fs::write(&workspace, "addon = \"queue\"\n").unwrap();
        let err = load_layered(None, Some(&workspace), None).unwrap_err();
        assert_eq!(err.code, "invalid_request");
        let missing = dir.join("missing.toml");
        let err = load_layered(None, None, Some(&missing)).unwrap_err();
        assert!(err.message.contains("not found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn substrate_config_sits_beside_the_markdown_file() {
        let path = workspace_config_path(Path::new("/rooms/notes.md"));
        assert_eq!(path, PathBuf::from("/rooms/notes.md.commonroom.toml"));
    }
}
