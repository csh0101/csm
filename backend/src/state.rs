use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use tokio::sync::RwLock;

use crate::{
    collaboration,
    config::Config,
    discovery::{self, LanDiscoveryHandle},
    error::AppError,
    models::{CollaborationStore, MetadataFile, PeerPresence, Session},
    scanner, storage,
};

pub type SharedState = Arc<AppState>;

pub struct AppState {
    pub config: Config,
    pub inner: RwLock<AppData>,
    pub lan_discovery: Mutex<Option<LanDiscoveryHandle>>,
}

#[derive(Debug, Default)]
pub struct AppData {
    pub metadata: MetadataFile,
    pub collaboration: CollaborationStore,
    pub peer_presence: HashMap<String, PeerPresence>,
    pub sessions: HashMap<String, Session>,
    pub workspace_path: Option<String>,
    pub stale_after_days: i64,
}

impl AppState {
    pub fn new(config: Config) -> Result<SharedState, AppError> {
        let metadata = storage::load_metadata(&config.metadata_path)?;
        let mut collaboration = storage::load_collaboration_store(&config.collaboration_path)?;
        let peer_display_name = env::var("CSM_PEER_DISPLAY_NAME")
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .or_else(|| {
                collaboration
                    .local_peer
                    .as_ref()
                    .map(|peer| peer.display_name.clone())
            })
            .unwrap_or_else(|| config.peer_display_name.clone());
        let local_peer = collaboration::ensure_local_peer(
            &mut collaboration,
            peer_display_name,
            format!("http://{}", config.bind_addr),
        );
        storage::save_collaboration_store(&config.collaboration_path, &collaboration)?;
        let stale_after_days = metadata
            .stale_after_days
            .filter(|days| *days > 0)
            .unwrap_or(config.stale_after_days);
        let mut sessions = HashMap::new();
        let mut workspace_path = metadata.workspace_path.clone();

        if let Some(path) = metadata.workspace_path.as_deref() {
            match scanner::scan_workspace(
                path,
                &metadata,
                config.max_preview_bytes,
                stale_after_days,
            ) {
                Ok(scan) => {
                    workspace_path = Some(scan.workspace_path);
                    sessions = scan
                        .sessions
                        .into_iter()
                        .map(|session| (session.id.clone(), session))
                        .collect();
                }
                Err(error) => {
                    tracing::warn!("failed to restore sessions from last workspace path: {error}");
                }
            }
        }

        let state = Arc::new(Self {
            config,
            lan_discovery: Mutex::new(None),
            inner: RwLock::new(AppData {
                metadata,
                collaboration,
                peer_presence: HashMap::new(),
                sessions,
                workspace_path,
                stale_after_days,
            }),
        });

        if let Some(discovery) =
            discovery::start(state.clone(), local_peer.peer_id, local_peer.display_name)?
        {
            *state
                .lan_discovery
                .lock()
                .expect("LAN discovery lock poisoned") = Some(discovery);
        }

        Ok(state)
    }
}
