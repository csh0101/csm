use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub metadata_path: PathBuf,
    pub collaboration_path: PathBuf,
    pub peer_token: Option<String>,
    pub lan_discovery_enabled: bool,
    pub peer_display_name: String,
    pub archive_dir: PathBuf,
    pub max_preview_bytes: usize,
    pub stale_after_days: i64,
}

impl Config {
    pub fn from_env() -> Self {
        let bind_addr = env::var("CSM_BIND_ADDR")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 4000)));

        let data_dir = env::var_os("CSM_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);

        let metadata_path = data_dir.join("metadata.json");
        let collaboration_path = data_dir.join("collaboration.json");
        let peer_token = env::var("CSM_PEER_TOKEN")
            .ok()
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        let lan_discovery_enabled = env_flag("CSM_LAN_DISCOVERY", false);
        let peer_display_name = env::var("CSM_PEER_DISPLAY_NAME")
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(default_peer_display_name);
        let archive_dir = env::var_os("CSM_ARCHIVE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("archive"));

        let max_preview_bytes = env::var("CSM_MAX_PREVIEW_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(512 * 1024);

        let stale_after_days = env::var("CSM_STALE_AFTER_DAYS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|days| *days > 0)
            .unwrap_or(15);

        Self {
            bind_addr,
            data_dir,
            metadata_path,
            collaboration_path,
            peer_token,
            lan_discovery_enabled,
            peer_display_name,
            archive_dir,
            max_preview_bytes,
            stale_after_days,
        }
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn default_peer_display_name() -> String {
    env::var("HOSTNAME")
        .or_else(|_| env::var("COMPUTERNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Codex Session Manager".to_string())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }

    Path::new(path).to_path_buf()
}

fn default_data_dir() -> PathBuf {
    home_dir().join(".codex-session-manager")
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
